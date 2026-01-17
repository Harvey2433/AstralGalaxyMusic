import { defineStore } from 'pinia';
import { ref, computed, watch } from 'vue';
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
  const activeEngine = ref('galaxy');
  const showPlaylist = ref(false);
  
  // --- 2. 交互锁 ---
  const isDragging = ref(false);   
  const isBuffering = ref(false);  
  const isSeeking = ref(false);    
  const playSessionId = ref(0);    

  // --- 3. 内部状态 ---
  let internalRealVolume = 0.0; 
  let fadeRafId: number | null = null;
  let actionTimeoutId: any = null;
  let isProgrammaticVolumeControl = false;

  // --- 4. 辅助状态 ---
  const likedTracks = ref<Set<string>>(new Set(JSON.parse(localStorage.getItem('liked_tracks') || '[]')));
  const availableDevices = ref<string[]>([]);
  const activeDevice = ref('Default');
  const notifyUI = ref<NotificationCallback | null>(null);
  const setNotifier = (fn: NotificationCallback) => { notifyUI.value = fn; };

  const queue = ref<Track[]>([]);
  const currentIndex = ref(0);
  // 确保 currentTrack 始终响应 queue 和 currentIndex 的变化
  const currentTrack = computed(() => {
      if (queue.value.length === 0 || currentIndex.value < 0 || currentIndex.value >= queue.value.length) return null;
      return queue.value[currentIndex.value];
  });
  const likedQueue = computed(() => queue.value.filter(t => likedTracks.value.has(t.id)));

  // --- 5. 基础功能 ---
  const toggleLike = (track: Track) => {
    if (likedTracks.value.has(track.id)) { likedTracks.value.delete(track.id); } 
    else { likedTracks.value.add(track.id); }
    localStorage.setItem('liked_tracks', JSON.stringify(Array.from(likedTracks.value)));
  };
  const isLiked = (track: Track) => likedTracks.value.has(track.id);
  const togglePlaylist = () => { showPlaylist.value = !showPlaylist.value; };
  const fetchDevices = async () => { 
    try { 
      // 获取后端真实设备列表
      const realDevices = await invoke<string[]>('get_output_devices');
      // 🔥 修复：手动添加 'Default' 到列表首位，确保与 activeDevice 初始值匹配
      availableDevices.value = ['Default', ...realDevices];
    } catch (e) { 
      console.error(e);
      availableDevices.value = ['Default']; // 即使失败也保留 Default
    } 
  };

  // --- 6. 淡入淡出控制器 ---
  const abortCurrentTransition = () => {
    if (fadeRafId !== null) { cancelAnimationFrame(fadeRafId); fadeRafId = null; }
    if (actionTimeoutId !== null) { clearTimeout(actionTimeoutId); actionTimeoutId = null; }
    isProgrammaticVolumeControl = false;
  };

  const transitionVolume = (targetVol0to1: number, durationSec: number) => {
    return new Promise<void>((resolve) => {
      const startVol = internalRealVolume;
      const endVol = targetVol0to1;
      const startTime = performance.now();
      isProgrammaticVolumeControl = true;

      const tick = () => {
        const now = performance.now();
        const p = Math.min((now - startTime) / (durationSec * 1000), 1.0);
        const ease = Math.sin(p * Math.PI / 2);
        const current = startVol + (endVol - startVol) * ease;
        
        internalRealVolume = current;
        invoke('player_set_volume', { vol: current });

        if (p < 1.0) {
          fadeRafId = requestAnimationFrame(tick);
        } else {
          fadeRafId = null;
          isProgrammaticVolumeControl = false;
          resolve();
        }
      };
      fadeRafId = requestAnimationFrame(tick);
    });
  };

  // --- 7. 播放控制核心 ---
  const switchEngine = async (engineId: string) => {
    try { await invoke('init_audio_engine', { engineId }); activeEngine.value = engineId; return true; } 
    catch (e: any) { return false; }
  };

  const executePlayLogic = async (isNewTrack: boolean) => {
      try {
        if (isNewTrack) {
             internalRealVolume = 0.0;
             await invoke('player_set_volume', { vol: 0.0 });
        }
        if (!isNewTrack) await invoke('player_play');

        isPlaying.value = true;
        isPaused.value = false;
        if (!hasStarted.value) hasStarted.value = true;
        startProgressLoop(); 

        const target = volume.value / 100.0;
        await transitionVolume(target, 0.45);
      } catch (e) { console.error(e); }
  };

  const executePauseLogic = async () => {
      try {
          await transitionVolume(0.0, 0.45);
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
    abortCurrentTransition();

    actionTimeoutId = setTimeout(async () => {
        if (intentToPlay) await executePlayLogic(false);
        else await executePauseLogic();
    }, 100);
  };

  const loadAndPlay = async () => {
    if (!currentTrack.value) return;
    abortCurrentTransition();
    playSessionId.value++;
    
    isPlaying.value = true;
    isPaused.value = false;
    isBuffering.value = true;
    currentTime.value = 0;
    progress.value = 0;
    stopProgressLoop();

    const mySession = playSessionId.value;

    actionTimeoutId = setTimeout(async () => {
        try {
            internalRealVolume = 0.0;
            await invoke('player_set_volume', { vol: 0.0 });
            const duration = await invoke<number>('player_load_track', { path: currentTrack.value!.path });
            if (mySession !== playSessionId.value) return;
            if (duration > 0.1) currentTrack.value!.duration = duration;
            isBuffering.value = false;
            await executePlayLogic(true);
        } catch (e) {
            if (mySession === playSessionId.value) {
                isPlaying.value = false;
                isBuffering.value = false;
                invoke('player_set_volume', { vol: volume.value / 100.0 });
                if(notifyUI.value) notifyUI.value("PLAY FAILED", "error");
            }
        }
    }, 100);
  };

  // --- 8. 队列控制 (修复随机播放) ---
  const nextTrack = async () => { 
      if(queue.value.length === 0) return; 
      
      if (playMode.value === 'shuffle') {
          // 真正的随机播放逻辑：随机选取一个非当前的索引
          let nextIndex = Math.floor(Math.random() * queue.value.length);
          // 避免重复（除非只有一首歌）
          if (queue.value.length > 1 && nextIndex === currentIndex.value) {
              nextIndex = (nextIndex + 1) % queue.value.length;
          }
          currentIndex.value = nextIndex;
      } else {
          // 顺序循环
          currentIndex.value = (currentIndex.value + 1) % queue.value.length; 
      }
      await loadAndPlay(); 
  };

  const prevTrack = async () => { 
      if(queue.value.length === 0) return; 
      // 上一曲逻辑通常不需要随机，保持线性回退符合直觉，或者也可以随机
      currentIndex.value = currentIndex.value > 0 ? currentIndex.value - 1 : queue.value.length - 1; 
      await loadAndPlay(); 
  };

  const playTrack = (track: Track) => { const idx = queue.value.indexOf(track); if (idx !== -1) { currentIndex.value = idx; loadAndPlay(); } };
  const toggleMode = () => { const modes: PlayMode[] = ['sequence', 'loop', 'shuffle']; playMode.value = modes[(modes.indexOf(playMode.value) + 1) % modes.length]; };

  // --- 9. Seek & Setup ---
  const performWithStateCheck = async (action: () => Promise<void>) => {
      abortCurrentTransition();
      const wasPaused = isPaused.value || !isPlaying.value;
      await new Promise(r => setTimeout(r, 100));
      await action();
      if (wasPaused) {
          await invoke('player_pause');
          internalRealVolume = 0.0; 
          invoke('player_set_volume', { vol: 0.0 });
      } else {
          internalRealVolume = volume.value / 100.0;
          invoke('player_set_volume', { vol: internalRealVolume });
      }
  };

  const setOutputDevice = async (device: string) => {
    await performWithStateCheck(async () => {
        try {
            // 如果用户选了 Default，传给后端的 device 字符串就是 "Default"
            // 请确保 mod.rs 里的 set_audio_device 能处理这个字符串（如果还没处理）
            await invoke('set_output_device', { device });
            activeDevice.value = device;
            if (notifyUI.value) notifyUI.value(`OUTPUT: ${device}`);
            if (currentTrack.value) await invoke('player_seek', { time: currentTime.value });
        } catch (e) { if (notifyUI.value) notifyUI.value('DEVICE ERROR', 'error'); }
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
    abortCurrentTransition();
    stopProgressLoop(); 
    isSeeking.value = true; 
    isBuffering.value = true; 
    
    const targetTime = (percent / 100) * currentTrack.value.duration;
    progress.value = percent; 
    currentTime.value = targetTime;
    
    const mySession = playSessionId.value;

    try {
      await new Promise(r => setTimeout(r, 100));
      await invoke('player_seek', { time: targetTime });
      
      if (mySession === playSessionId.value) {
          isSeeking.value = false; 
          isBuffering.value = false;
          if (wasPlaying) {
              internalRealVolume = volume.value / 100.0;
              invoke('player_set_volume', { vol: internalRealVolume });
              startProgressLoop();
          } else {
              await invoke('player_pause');
          }
      }
    } catch (e) {
      if (mySession === playSessionId.value) { isSeeking.value = false; isBuffering.value = false; }
    }
  };

  // --- 10. Loop & Events ---
  let listenersBound = false;
  const setupEventListeners = async () => {
    if (listenersBound) return;
    listenersBound = true;
    
    await listen<Track>('import-track', (event) => {
      const t = event.payload;
      if (!queue.value.some(track => track.path === t.path)) {
        queue.value.push({ ...t, id: Date.now().toString() + Math.random().toString(36).substr(2, 6), cover: t.cover === 'DEFAULT_COVER' ? DEFAULT_COVER : t.cover, isAvailable: true });
      }
    });
    
    await listen('import-finish', () => { if (notifyUI.value) notifyUI.value('LIBRARY UPDATED'); });
    
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

  watch(volume, (v) => { 
      if(!isProgrammaticVolumeControl) {
          internalRealVolume = v / 100.0;
          invoke('player_set_volume', { vol: internalRealVolume }); 
      }
  });

  return { 
    isPlaying, isPaused, hasStarted, volume, progress, currentTime, playMode, queue, currentIndex, currentTrack, activeEngine, showPlaylist, 
    isDragging, isBuffering, isSeeking,
    likedTracks, likedQueue, availableDevices, activeDevice, 
    togglePlay, nextTrack, prevTrack, seekTo, switchEngine, loadAndPlay, initCheck, setNotifier, importTracks, 
    togglePlaylist, toggleMode, toggleLike, isLiked, fetchDevices, setOutputDevice, playTrack, setChannelMode 
  };
});