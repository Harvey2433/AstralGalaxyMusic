import { defineStore } from 'pinia';
import { ref, computed, watch, onMounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

const DEFAULT_COVER = 'https://images.unsplash.com/photo-1614728853913-6591d801d643?q=80&w=400&auto=format&fit=crop';

export interface Track {
  id: string; title: string; artist: string; album: string; cover: string; duration: number; path: string; isAvailable?: boolean; 
}
export type PlayMode = 'sequence' | 'loop' | 'shuffle';
type NotificationCallback = (msg: string, type?: 'info' | 'error') => void;

export const usePlayerStore = defineStore('player', () => {
  // --- 1. 核心状态 ---
  const isPlaying = ref(false);
  const isPaused = ref(false);
  const hasStarted = ref(false);
  const volume = ref(80);
  const progress = ref(0);
  const currentTime = ref(0);
  const playMode = ref<PlayMode>('sequence');
  const showPlaylist = ref(false);
  
  // --- 2. 交互锁 ---
  const isDragging = ref(false);   
  const isBuffering = ref(false);  
  const isSeeking = ref(false);    
  const playSessionId = ref(0);    
  
  // 🔥 引擎与下载状态
  const activeEngine = ref('galaxy'); 
  const isDownloadingFFmpeg = ref(false);
  const ffmpegProgress = ref(0);
  
  // 🔥 冷启动标志
  const hasAudioInitialized = ref(false);

  // --- 3. 辅助状态 ---
  let actionTimeoutId: any = null;
  const likedTracks = ref<Set<string>>(new Set(JSON.parse(localStorage.getItem('liked_tracks') || '[]')));
  const availableDevices = ref<string[]>([]);
  const activeDevice = ref('Default');
  const notifyUI = ref<NotificationCallback | null>(null);
  const setNotifier = (fn: NotificationCallback) => { notifyUI.value = fn; };

  const queue = ref<Track[]>([]);
  const currentIndex = ref(0);
  const currentTrack = computed(() => {
      if (queue.value.length === 0 || currentIndex.value < 0 || currentIndex.value >= queue.value.length) return null;
      return queue.value[currentIndex.value];
  });
  const likedQueue = computed(() => queue.value.filter(t => likedTracks.value.has(t.id)));

  // --- 4. 初始化与事件监听 ---
  const syncEngine = async () => {
      try {
          const realEngine = await invoke<string>('get_current_engine');
          activeEngine.value = realEngine;
          console.log("Current Engine synced:", realEngine);
      } catch (e) { console.error("Sync Engine Failed:", e); }
  };

  onMounted(async () => {
      await syncEngine();
      
      // 监听 FFmpeg 状态事件
      await listen('ffmpeg-status', async (e: any) => {
          const status = e.payload;
          if (status === 'downloading') {
              isDownloadingFFmpeg.value = true;
              ffmpegProgress.value = 0;
              notifyUI.value?.('正在下载组件...', 'info');
          } else if (status === 'extracting') { // 🔥 新增：解压状态处理
              isDownloadingFFmpeg.value = true;
              ffmpegProgress.value = 99;
              notifyUI.value?.('下载完成，正在解压...', 'info');
          } else if (status === 'ready') { 
              isDownloadingFFmpeg.value = false;
              ffmpegProgress.value = 100;
              notifyUI.value?.('组件安装完成，正在应用...');
              // 自动完成未竟的切换
              await switchEngine('ffmpeg');
          } else if (status === 'error') {
              isDownloadingFFmpeg.value = false;
              notifyUI.value?.('组件下载失败，请检查网络', 'error');
          }
      });

      await listen('ffmpeg-progress', (e: any) => {
          ffmpegProgress.value = e.payload as number;
      });
      
      await listen('download-success', async (e: any) => {
          if (e.payload === 'ffmpeg') {
              await switchEngine('ffmpeg');
          }
      });

      await setupEventListeners();
  });

  // --- 5. 引擎切换核心逻辑 ---
  const switchEngine = async (engineId: string) => {
      // 🔥 修改：如果正在下载，严格禁止切换任何引擎
      if (isDownloadingFFmpeg.value) {
          notifyUI.value?.('后台任务进行中，请等待安装完成', 'error');
          return;
      }
      
      // 乐观更新
      activeEngine.value = engineId;
      
      notifyUI.value?.(`正在切换至 ${engineId.toUpperCase()}...`);
      const savedTime = currentTime.value;
      const wasPlaying = isPlaying.value;

      try {
          const res = await invoke<string>('init_audio_engine', { engineId });
          
          // Case A: 需要下载
          if (res === "DOWNLOADING") {
              isDownloadingFFmpeg.value = true;
              notifyUI.value?.("正在下载必要组件 FFmpeg...");
              // 保持 activeEngine 为 ffmpeg 以显示加载态
              return;
          }
          
          // Case B: 切换成功 (READY)
          if (res.includes("READY") || res === "SUCCESS") {
              hasAudioInitialized.value = true;
              notifyUI.value?.(`${engineId.toUpperCase()} 就绪`);
              
              // 恢复播放状态
              if (wasPlaying && currentTrack.value) {
                  await invoke('player_load_track', { path: currentTrack.value.path });
                  await invoke('player_seek', { time: savedTime });
                  await executePlayLogic(false); 
              }
          }
      } catch (e: any) {
          notifyUI.value?.(`切换失败: ${e}`, 'error');
          // 失败回滚
          await syncEngine();
          isDownloadingFFmpeg.value = false;
      }
  };

  // --- 6. 基础功能 ---
  const toggleLike = (track: Track) => {
    if (likedTracks.value.has(track.id)) { likedTracks.value.delete(track.id); } 
    else { likedTracks.value.add(track.id); }
    localStorage.setItem('liked_tracks', JSON.stringify(Array.from(likedTracks.value)));
  };
  const isLiked = (track: Track) => likedTracks.value.has(track.id);
  const togglePlaylist = () => { showPlaylist.value = !showPlaylist.value; };
  const fetchDevices = async () => { 
    try { 
      const realDevices = await invoke<string[]>('get_output_devices');
      availableDevices.value = ['Default', ...realDevices];
    } catch (e) { availableDevices.value = ['Default']; } 
  };

  // --- 7. 播放控制核心 ---
  const executePlayLogic = async (isNewTrack: boolean) => {
      try {
        if (isNewTrack) await invoke('player_set_volume', { vol: volume.value / 100.0 });
        if (!isNewTrack) await invoke('player_play');

        isPlaying.value = true;
        isPaused.value = false;
        if (!hasStarted.value) hasStarted.value = true;
        startProgressLoop(); 
        setTimeout(() => { invoke('player_set_volume', { vol: volume.value / 100.0 }); }, 100);
      } catch (e) { console.error(e); }
  };

  const executePauseLogic = async () => {
      try {
          await invoke('player_pause');
          isPlaying.value = false;
          isPaused.value = true;
          stopProgressLoop();
      } catch (e) { console.error(e); }
  };

  const togglePlay = () => {
    if (!currentTrack.value) return;
    const intentToPlay = !isPlaying.value; 
    isPlaying.value = intentToPlay;
    isPaused.value = !intentToPlay; 
    
    if (actionTimeoutId) clearTimeout(actionTimeoutId);
    actionTimeoutId = setTimeout(async () => {
        if (intentToPlay) await executePlayLogic(false);
        else await executePauseLogic();
    }, 50);
  };

  const loadAndPlay = async () => {
    if (!currentTrack.value) return;
    playSessionId.value++;
    
    isPlaying.value = true;
    isPaused.value = false;
    isBuffering.value = true;
    currentTime.value = 0;
    progress.value = 0;
    stopProgressLoop();

    const mySession = playSessionId.value;

    if (actionTimeoutId) clearTimeout(actionTimeoutId);
    actionTimeoutId = setTimeout(async () => {
        try {
            if (!hasAudioInitialized.value) {
                console.log("🔥 Cold Start: Forcing Audio Device Wakeup...");
                await invoke('set_output_device', { device: activeDevice.value });
                hasAudioInitialized.value = true;
            }

            const duration = await invoke<number>('player_load_track', { path: currentTrack.value!.path });
            if (mySession !== playSessionId.value) return;
            if (duration > 0.1) currentTrack.value!.duration = duration;
            isBuffering.value = false;
            await executePlayLogic(true);
        } catch (e) {
            if (mySession === playSessionId.value) {
                isPlaying.value = false;
                isBuffering.value = false;
                notifyUI.value?.("PLAY FAILED", "error");
            }
        }
    }, 50);
  };

  const nextTrack = async () => { 
      if(queue.value.length === 0) return; 
      if (playMode.value === 'shuffle') {
          const total = queue.value.length;
          if (total > 1) {
              const seed = (Date.now() ^ (currentIndex.value * 123456789));
              const chaos = Math.abs(Math.sin(seed) * 100000.0);
              let targetIndex = Math.floor((chaos - Math.floor(chaos)) * total);
              if (targetIndex === currentIndex.value) targetIndex = (targetIndex + 1) % total;
              currentIndex.value = targetIndex;
          }
      } else {
          currentIndex.value = (currentIndex.value + 1) % queue.value.length; 
      }
      await loadAndPlay(); 
  };

  const prevTrack = async () => { 
      if(queue.value.length === 0) return; 
      currentIndex.value = currentIndex.value > 0 ? currentIndex.value - 1 : queue.value.length - 1; 
      await loadAndPlay(); 
  };

  const playTrack = (track: Track) => { const idx = queue.value.indexOf(track); if (idx !== -1) { currentIndex.value = idx; loadAndPlay(); } };
  const toggleMode = () => { const modes: PlayMode[] = ['sequence', 'loop', 'shuffle']; playMode.value = modes[(modes.indexOf(playMode.value) + 1) % modes.length]; };

  const performWithStateCheck = async (action: () => Promise<void>) => {
      const wasPaused = isPaused.value || !isPlaying.value;
      await action();
      if (wasPaused) { await invoke('player_pause'); } 
      else { await invoke('player_set_volume', { vol: volume.value / 100.0 }); }
  };

  const setOutputDevice = async (device: string) => {
    await performWithStateCheck(async () => {
        try {
            await invoke('set_output_device', { device });
            activeDevice.value = device;
            hasAudioInitialized.value = true;
            notifyUI.value?.(`OUTPUT: ${device}`);
            if (currentTrack.value) await invoke('player_seek', { time: currentTime.value });
        } catch (e) { notifyUI.value?.('DEVICE ERROR', 'error'); }
    });
  };

  const setChannelMode = async (mode: number) => {
      await performWithStateCheck(async () => {
          await invoke('player_set_channels', { mode });
          if (currentTrack.value) await invoke('player_seek', { time: currentTime.value });
      });
  };

  const seekTo = async (percent: number) => {
    if (!currentTrack.value || currentTrack.value.duration <= 0) return;
    const wasPlaying = isPlaying.value && !isPaused.value;
    stopProgressLoop(); 
    isSeeking.value = true; 
    const targetTime = (percent / 100) * currentTrack.value.duration;
    progress.value = percent; 
    currentTime.value = targetTime;
    try {
      await invoke('player_seek', { time: targetTime });
      isSeeking.value = false; 
      if (wasPlaying) startProgressLoop();
    } catch (e) { isSeeking.value = false; }
  };

  const setupEventListeners = async () => {
    if (listenersBound) return;
    listenersBound = true;
    await listen<Track>('import-track', (event) => {
      const t = event.payload;
      if (!queue.value.some(track => track.path === t.path)) {
        queue.value.push({ ...t, id: Date.now().toString() + Math.random().toString(36).substr(2, 6), cover: t.cover === 'DEFAULT_COVER' ? DEFAULT_COVER : t.cover, isAvailable: true });
      }
    });
    await listen('import-finish', () => { notifyUI.value?.('LIBRARY UPDATED'); });
    await listen<number>('seek-end', (e) => {
        if (isSeeking.value || isDragging.value || isBuffering.value) return; 
        if (Math.abs(currentTime.value - e.payload) > 0.5) currentTime.value = e.payload;
    });
  };

  const importTracks = async () => { await setupEventListeners(); try { await invoke('import_music'); } catch(e){} };
  const initCheck = async () => { await setupEventListeners(); for (const track of queue.value) { try { await invoke('check_file_exists', { path: track.path }); track.isAvailable = true; } catch(e){ track.isAvailable = false; } } };

  let rafId: number | null = null;
  let lastFrameTime = 0;
  const startProgressLoop = () => {
    stopProgressLoop();
    lastFrameTime = performance.now();
    const loop = (timestamp: number) => {
      if (!isPlaying.value || isPaused.value || !currentTrack.value) return; 
      const deltaTime = (timestamp - lastFrameTime) / 1000; 
      lastFrameTime = timestamp;
      if (!isDragging.value && !isBuffering.value && !isSeeking.value) {
          currentTime.value += deltaTime;
          if (currentTime.value >= currentTrack.value.duration) {
             if (playMode.value === 'loop') { currentTime.value = 0; invoke('player_seek', { time: 0.0 }); } 
             else { nextTrack(); return; }
          }
          if (currentTrack.value.duration > 0) progress.value = (currentTime.value / currentTrack.value.duration) * 100;
      }
      rafId = requestAnimationFrame(loop);
    };
    rafId = requestAnimationFrame(loop);
  };
  const stopProgressLoop = () => { if (rafId !== null) { cancelAnimationFrame(rafId); rafId = null; } };

  // 防止重复绑定
  let listenersBound = false;

  watch(volume, (v) => { invoke('player_set_volume', { vol: v / 100.0 }); });

  return { 
    isPlaying, isPaused, hasStarted, volume, progress, currentTime, playMode, queue, currentIndex, currentTrack, activeEngine, showPlaylist, 
    isDragging, isBuffering, isSeeking, 
    isDownloadingFFmpeg, ffmpegProgress, 
    hasAudioInitialized, 
    likedTracks, likedQueue, availableDevices, activeDevice, 
    togglePlay, nextTrack, prevTrack, seekTo, switchEngine, loadAndPlay, initCheck, setNotifier, importTracks, 
    togglePlaylist, toggleMode, toggleLike, isLiked, fetchDevices, setOutputDevice, playTrack, setChannelMode 
  };
});