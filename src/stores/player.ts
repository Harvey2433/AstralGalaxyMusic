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
  const isPlaying = ref(false);
  const isPaused = ref(false);
  const hasStarted = ref(false);
  const volume = ref(80);
  const progress = ref(0);
  const currentTime = ref(0);
  const playMode = ref<PlayMode>('sequence');
  const showPlaylist = ref(false);
  
  const isDragging = ref(false);   
  const isBuffering = ref(false);  
  const isSeeking = ref(false);    
  const playSessionId = ref(0);    
  
  const activeEngine = ref('galaxy'); 
  const isDownloadingFFmpeg = ref(false);
  const ffmpegProgress = ref(0);
  
  // 🔥 新增：并发切换防抖锁与冷启动标志
  const isEngineSwitching = ref(false);
  const hasAudioInitialized = ref(false);

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

  const syncEngine = async () => {
      try {
          const realEngine = await invoke<string>('get_current_engine');
          activeEngine.value = realEngine;
      } catch (e) { console.error("Sync Engine Failed:", e); }
  };

  onMounted(async () => {
      await syncEngine();
      
      await listen('ffmpeg-status', async (e: any) => {
          const status = e.payload;
          if (status === 'downloading') {
              isDownloadingFFmpeg.value = true;
              ffmpegProgress.value = 0;
              notifyUI.value?.('FETCHING ENGINE...', 'info');
          } else if (status === 'extracting') { 
              isDownloadingFFmpeg.value = true;
              ffmpegProgress.value = 99;
              notifyUI.value?.('EXTRACTING COMPONENTS...', 'info');
          } else if (status === 'ready') { 
              isDownloadingFFmpeg.value = false;
              ffmpegProgress.value = 100;
              notifyUI.value?.('FFMPEG READY. INITIALIZING...');
              
              const savedTime = currentTime.value;
              const wasPlaying = isPlaying.value;
              
              if (wasPlaying) {
                  await invoke('player_pause');
              }

              try {
                  isEngineSwitching.value = true; // 加锁
                  const res = await invoke<string>('init_audio_engine', { engineId: 'ffmpeg' });
                  
                  if (res.includes("READY")) {
                      activeEngine.value = 'ffmpeg';
                      if (currentTrack.value) {
                          // 🔥 魔术修复：静音 -> 加载 -> Seek -> 恢复音量
                          await invoke('player_set_volume', { vol: 0.0 });
                          await invoke('player_load_track', { path: currentTrack.value.path });
                          await invoke('player_seek', { time: savedTime });
                          
                          if (wasPlaying) {
                              await executePlayLogic(false); 
                              notifyUI.value?.('FFMPEG ONLINE. RESUMING.');
                          } else {
                              await invoke('player_set_volume', { vol: volume.value / 100.0 });
                              await invoke('player_pause');
                          }
                      }
                  } else {
                      notifyUI.value?.('FFMPEG ENGINE CRASHED', 'error');
                  }
              } catch (err) {
                  notifyUI.value?.('FFMPEG FAILED TO LOAD', 'error');
              } finally {
                  isEngineSwitching.value = false; // 解锁
              }
          } else if (status === 'error') {
              isDownloadingFFmpeg.value = false;
              notifyUI.value?.('DOWNLOAD FAILED. CHECK NETWORK.', 'error');
          }
      });

      await listen('ffmpeg-progress', (e: any) => {
          ffmpegProgress.value = e.payload as number;
      });

      await setupEventListeners();
  });

  const switchEngine = async (engineId: string): Promise<'SUCCESS' | 'DOWNLOADING' | 'FAILED'> => {
      if (isDownloadingFFmpeg.value) {
          notifyUI.value?.('PLEASE WAIT FOR DOWNLOAD', 'error');
          return 'FAILED';
      }
      // 🔥 强行打断你的手残连点
      if (isEngineSwitching.value) {
          notifyUI.value?.('系统切换中，请勿频繁点击！', 'error');
          return 'FAILED';
      }
      
      const previousEngine = activeEngine.value;
      if (previousEngine === engineId) return 'SUCCESS';
      
      isEngineSwitching.value = true; // 上锁
      notifyUI.value?.(`正在切换至 ${engineId.toUpperCase()}...`);
      
      try {
          // 先暂停当前播放，释放设备并防止爆音
          const savedTime = currentTime.value;
          const wasPlaying = isPlaying.value;
          if (wasPlaying) {
              await invoke('player_pause');
          }

          const res = await invoke<string>('init_audio_engine', { engineId });
          
          if (res === "DOWNLOADING") {
              isDownloadingFFmpeg.value = true;
              activeEngine.value = previousEngine; // 状态回滚给 UI
              if (wasPlaying) await executePlayLogic(false); // 恢复原引擎播放
              isEngineSwitching.value = false; // 解锁
              return 'DOWNLOADING';
          }
          
          if (res.includes("READY") || res === "SUCCESS") {
              hasAudioInitialized.value = true;
              activeEngine.value = engineId;
              
              if (currentTrack.value) {
                  // 🔥 魔术修复：强行把音频按在水底，等接回进度条再拉上来
                  await invoke('player_set_volume', { vol: 0.0 });
                  await invoke('player_load_track', { path: currentTrack.value.path });
                  await invoke('player_seek', { time: savedTime });
                  
                  if (wasPlaying) {
                      await executePlayLogic(false); // 内部含有延迟的 set_volume 和 play
                  } else {
                      await invoke('player_set_volume', { vol: volume.value / 100.0 });
                      await invoke('player_pause');
                  }
              }
              isEngineSwitching.value = false;
              return 'SUCCESS';
          }
          throw new Error("Invalid response");
      } catch (e: any) {
          console.error(e);
          notifyUI.value?.(`切换失败: ${e}`, 'error');
          await syncEngine();
          isEngineSwitching.value = false;
          return 'FAILED';
      }
  };

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

  const executePlayLogic = async (isNewTrack: boolean) => {
      try {
        if (isNewTrack) await invoke('player_set_volume', { vol: volume.value / 100.0 });
        if (!isNewTrack) await invoke('player_play');

        isPlaying.value = true;
        isPaused.value = false;
        if (!hasStarted.value) hasStarted.value = true;
        startProgressLoop(); 
        // 延迟执行音量推子，完美掩盖加载噪音
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
        queue.value.push({ ...t, id: Date.now().toString() + Math.random().toString(36).substring(2, 8), cover: t.cover === 'DEFAULT_COVER' ? DEFAULT_COVER : t.cover, isAvailable: true });
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