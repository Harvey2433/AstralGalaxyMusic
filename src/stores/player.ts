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

// 辅助：异步延迟
const delay = (ms: number) => new Promise(resolve => setTimeout(resolve, ms));

export const usePlayerStore = defineStore('player', () => {
  // --- 1. 核心状态 ---
  const isPlaying = ref(false);
  const isPaused = ref(false);
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
  const isFading = ref(false); // 淡入淡出锁
  const playSessionId = ref(0); 

  // --- 3. 辅助状态 ---
  const likedTracks = ref<Set<string>>(new Set(JSON.parse(localStorage.getItem('liked_tracks') || '[]')));
  const availableDevices = ref<string[]>([]);
  const activeDevice = ref('Default');
  const notifyUI = ref<NotificationCallback | null>(null);
  const setNotifier = (fn: NotificationCallback) => { notifyUI.value = fn; };

  // --- 4. 队列管理 ---
  const queue = ref<Track[]>([]);
  const currentIndex = ref(0);
  const currentTrack = computed(() => queue.value[currentIndex.value] || null);
  const likedQueue = computed(() => queue.value.filter(t => likedTracks.value.has(t.id)));

  // --- 5. 基础功能 ---
  const toggleLike = (track: Track) => {
    if (likedTracks.value.has(track.id)) { likedTracks.value.delete(track.id); } 
    else { likedTracks.value.add(track.id); }
    localStorage.setItem('liked_tracks', JSON.stringify(Array.from(likedTracks.value)));
  };
  const isLiked = (track: Track) => likedTracks.value.has(track.id);
  const togglePlaylist = () => { showPlaylist.value = !showPlaylist.value; };
  const fetchDevices = async () => { try { availableDevices.value = await invoke('get_output_devices'); } catch (e) {} };

  // --- 6. 音量淡入淡出 (核心修复：不依赖后端 getter) ---
  const applyFade = (startVol: number, endVol: number, durationSeconds: number) => {
    // 强制更新锁，防止冲突
    isFading.value = true;
    const startTime = performance.now();
    
    // 将 0-100 映射到 0.0-1.0
    const start = startVol / 100.0;
    const end = endVol / 100.0;

    return new Promise<void>((resolve) => {
      const tick = () => {
        const now = performance.now();
        const progress = Math.min((now - startTime) / (durationSeconds * 1000), 1.0);
        
        // 使用正弦缓动，听感更平滑
        const ease = Math.sin(progress * Math.PI / 2);
        const current = start + (end - start) * ease;
        
        invoke('player_set_volume', { vol: current });

        if (progress < 1.0) {
          requestAnimationFrame(tick);
        } else {
          isFading.value = false;
          resolve();
        }
      };
      requestAnimationFrame(tick);
    });
  };

  // --- 7. 带有状态保护的操作 (修复暂停状态下切换设置导致自动播放) ---
  const performWithStateCheck = async (action: () => Promise<void>) => {
      // 记录操作前的状态
      const wasPaused = isPaused.value || !isPlaying.value;
      
      await action();

      // 如果之前是暂停的，操作后强制恢复暂停状态
      if (wasPaused) {
          await invoke('player_pause'); // 确保后端暂停
          isPlaying.value = false;
          isPaused.value = true;
          stopProgressSimulation();
      }
  };

  const setOutputDevice = async (device: string) => {
    await performWithStateCheck(async () => {
        try {
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

  // --- 8. 事件监听 ---
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
        currentTime.value = e.payload;
        if (currentTrack.value && currentTrack.value.duration > 0) {
            progress.value = (currentTime.value / currentTrack.value.duration) * 100;
        }
    });
  };

  const switchEngine = async (engineId: string) => {
    try { await invoke('init_audio_engine', { engineId }); activeEngine.value = engineId; return true; } 
    catch (e: any) { return false; }
  };
  const importTracks = async () => { await setupEventListeners(); try { await invoke('import_music'); } catch(e){} };
  const initCheck = async () => { await setupEventListeners(); for (const track of queue.value) { try { await invoke('check_file_exists', { path: track.path }); track.isAvailable = true; } catch(e){ track.isAvailable = false; } } };

  // --- 9. 核心播放逻辑 (修复：无声、崩溃、延迟) ---
  
  const loadAndPlay = async () => {
    if (!currentTrack.value) return;
    
    // 强制延迟 0.15s (用户要求)
    await delay(150);

    playSessionId.value++;
    stopProgressSimulation();
    
    // 初始化状态
    isBuffering.value = true;
    isSeeking.value = false;
    isDragging.value = false;
    isFading.value = true; // 锁定音量监听
    currentTime.value = 0;
    progress.value = 0;
    isPaused.value = false;
    isPlaying.value = false;

    const mySession = playSessionId.value;

    try {
      // 1. 先静音 (防止爆音)
      await invoke('player_set_volume', { vol: 0.0 });

      // 2. 加载
      const duration = await invoke<number>('player_load_track', { path: currentTrack.value.path });
      
      // 3. 检查切歌
      if (mySession !== playSessionId.value) return;

      if (duration > 0.1) currentTrack.value.duration = duration;
      
      // 4. 状态就绪
      isPlaying.value = true; 
      isBuffering.value = false;
      startProgressSimulation(); 

      // 5. 执行 0.45s 淡入 (从 0 到 设定音量)
      await applyFade(0, volume.value, 0.45);

    } catch (e) {
      if (mySession === playSessionId.value) {
          console.error(e);
          isPlaying.value = false;
          isBuffering.value = false;
          // 🔥 关键修复：如果出错，必须恢复音量，否则下次播放会无声
          invoke('player_set_volume', { vol: volume.value / 100.0 });
          isFading.value = false;
          if(notifyUI.value) notifyUI.value("PLAY FAILED", "error");
      }
    }
  };

  const togglePlay = async () => {
    if (!currentTrack.value) return;
    
    if (isPlaying.value) { 
        // --- 暂停 ---
        // 1. 淡出 0.45s
        await applyFade(volume.value, 0, 0.45);
        // 2. 暂停后端
        await invoke('player_pause');
        isPlaying.value = false; 
        isPaused.value = true; 
        stopProgressSimulation(); 
    } else { 
        // --- 播放 ---
        if (isPaused.value) { 
            await delay(150); // 延迟
            await invoke('player_play');
            isPlaying.value = true; 
            isPaused.value = false; 
            startProgressSimulation(); 
            // 淡入 0.45s (从 0 恢复到设定音量)
            await applyFade(0, volume.value, 0.45);
        } else { 
            await loadAndPlay(); 
        } 
    }
  };

  const nextTrack = async () => { if(queue.value.length===0)return; currentIndex.value = (currentIndex.value + 1) % queue.value.length; await loadAndPlay(); };
  const prevTrack = async () => { if(queue.value.length===0)return; currentIndex.value = currentIndex.value > 0 ? currentIndex.value - 1 : queue.value.length - 1; await loadAndPlay(); };
  
  const playTrack = (track: Track) => {
      const idx = queue.value.indexOf(track);
      if (idx !== -1) { currentIndex.value = idx; loadAndPlay(); }
  };

  const toggleMode = () => { const modes: PlayMode[] = ['sequence', 'loop', 'shuffle']; playMode.value = modes[(modes.indexOf(playMode.value) + 1) % modes.length]; };

  // --- 10. Seek 逻辑 (修复：暂停时不自动播放) ---
  const seekTo = async (percent: number) => {
    if (!currentTrack.value || currentTrack.value.duration <= 0) return;

    // 记录原始状态
    const wasPaused = isPaused.value || !isPlaying.value;

    stopProgressSimulation(); 
    isSeeking.value = true; 
    isBuffering.value = true; 
    
    const targetTime = (percent / 100) * currentTrack.value.duration;
    progress.value = percent; 
    currentTime.value = targetTime;
    
    const mySession = playSessionId.value;

    try {
      await invoke('player_seek', { time: targetTime });
      
      if (mySession === playSessionId.value) {
          isSeeking.value = false; 
          isBuffering.value = false;
          
          if (wasPaused) {
              // 🔥 修复：如果之前是暂停的，Seek 后强制暂停，不自动播放
              await invoke('player_pause');
              isPlaying.value = false;
              isPaused.value = true;
          } else {
              isPlaying.value = true;
              startProgressSimulation();
          }
      }
    } catch (e) {
      if (mySession === playSessionId.value) {
          isSeeking.value = false;
          isBuffering.value = false;
      }
    }
  };

  // --- 11. 模拟器 ---
  let timer: any = null;
  const startProgressSimulation = () => {
    stopProgressSimulation();
    timer = setInterval(() => {
      if (!isPlaying.value || !currentTrack.value || currentTrack.value.duration <= 0) return;
      if (isDragging.value || isBuffering.value || isSeeking.value) return; 
      
      if (currentTime.value >= currentTrack.value.duration) { 
          if(playMode.value === 'loop') { 
              invoke('player_seek', { time: 0.0 });
              currentTime.value = 0; progress.value = 0;
          } else { 
              nextTrack(); 
          } 
          return; 
      }
      currentTime.value += 0.5;
      progress.value = (currentTime.value / currentTrack.value.duration) * 100;
    }, 500);
  };
  const stopProgressSimulation = () => { if (timer) clearInterval(timer); };
  
  // 普通音量调节 (如果不在淡入淡出中，实时响应)
  watch(volume, (v) => { 
      if(!isFading.value) invoke('player_set_volume', { vol: v / 100.0 }); 
  });

  return { 
    isPlaying, isPaused, volume, progress, currentTime, playMode, queue, currentIndex, currentTrack, activeEngine, showPlaylist, 
    isDragging, isBuffering, isSeeking,
    likedTracks, likedQueue, availableDevices, activeDevice, 
    togglePlay, nextTrack, prevTrack, seekTo, switchEngine, loadAndPlay, initCheck, setNotifier, importTracks, 
    togglePlaylist, 
    toggleMode, toggleLike, isLiked, fetchDevices, setOutputDevice, playTrack, setChannelMode 
  };
});