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

// 强制音频操作延迟 (不阻塞UI)
const audioDelay = () => new Promise(resolve => setTimeout(resolve, 100));

export const usePlayerStore = defineStore('player', () => {
  // --- 1. 核心状态 ---
  const isPlaying = ref(false); // UI 状态：是否应当显示为播放中
  const isPaused = ref(false);  // 逻辑状态：是否处于暂停中断
  const volume = ref(80);
  const progress = ref(0);
  const currentTime = ref(0);
  const playMode = ref<PlayMode>('sequence');
  const activeEngine = ref('galaxy');
  const showPlaylist = ref(false);
  
  // --- 2. 交互与同步状态锁 ---
  const isDragging = ref(false);   
  const isBuffering = ref(false);  
  const isSeeking = ref(false);    
  const isFading = ref(false); 
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

  // --- 6. 音量淡入淡出 (Volume Envelope) ---
  const applyFade = (startVol: number, endVol: number, durationSeconds: number) => {
    isFading.value = true;
    const startTime = performance.now();
    const start = startVol / 100.0;
    const end = endVol / 100.0;

    return new Promise<void>((resolve) => {
      const tick = () => {
        const now = performance.now();
        const p = Math.min((now - startTime) / (durationSeconds * 1000), 1.0);
        const ease = Math.sin(p * Math.PI / 2); // Sine Ease-Out
        const current = start + (end - start) * ease;
        
        invoke('player_set_volume', { vol: current });

        if (p < 1.0) {
          requestAnimationFrame(tick);
        } else {
          isFading.value = false;
          resolve();
        }
      };
      requestAnimationFrame(tick);
    });
  };

  // --- 7. 事件监听 & 心跳同步 ---
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
    
    // 后端 Seek 完成后的精确时间校准
    await listen<number>('seek-end', (e) => {
        if (isSeeking.value || isDragging.value || isBuffering.value) return; 
        
        // 只有当偏差超过 0.5s 时才强制校准，避免视觉跳动
        if (Math.abs(currentTime.value - e.payload) > 0.5) {
            currentTime.value = e.payload;
        }
    });
  };

  const switchEngine = async (engineId: string) => {
    try { await invoke('init_audio_engine', { engineId }); activeEngine.value = engineId; return true; } 
    catch (e: any) { return false; }
  };
  const importTracks = async () => { await setupEventListeners(); try { await invoke('import_music'); } catch(e){} };
  const initCheck = async () => { await setupEventListeners(); for (const track of queue.value) { try { await invoke('check_file_exists', { path: track.path }); track.isAvailable = true; } catch(e){ track.isAvailable = false; } } };

  // --- 8. 核心播放逻辑 (UI/Audio 分离) ---
  
  const loadAndPlay = async () => {
    if (!currentTrack.value) return;
    
    // 1. UI 立即响应：显示为播放状态，但处于缓冲中
    playSessionId.value++;
    stopProgressLoop();
    
    isPlaying.value = true;      // UI: 播放图标亮起
    isPaused.value = false;
    isBuffering.value = true;    // UI: 灵动岛显示 Loading
    isSeeking.value = false;
    currentTime.value = 0;
    progress.value = 0;
    
    const mySession = playSessionId.value;

    try {
      // 音频操作强制延迟
      await audioDelay();
      if (mySession !== playSessionId.value) return;

      // 静音防爆音
      await invoke('player_set_volume', { vol: 0.0 });
      
      const duration = await invoke<number>('player_load_track', { path: currentTrack.value.path });
      if (mySession !== playSessionId.value) return;

      if (duration > 0.1) currentTrack.value.duration = duration;
      
      // 加载完成，开始跑进度条
      isBuffering.value = false;
      startProgressLoop(); 

      // 淡入
      await applyFade(0, volume.value, 0.45);

    } catch (e) {
      if (mySession === playSessionId.value) {
          isPlaying.value = false; // 回滚 UI 状态
          isBuffering.value = false;
          invoke('player_set_volume', { vol: volume.value / 100.0 }); // 恢复音量
          if(notifyUI.value) notifyUI.value("PLAY FAILED", "error");
      }
    }
  };

  const togglePlay = async () => {
    if (!currentTrack.value) return;
    
    if (isPlaying.value) { 
        // >>> 暂停逻辑 <<<
        // 1. UI 立即响应
        isPlaying.value = false; 
        isPaused.value = true;
        stopProgressLoop(); // 停止进度条

        // 2. 音频后台处理
        await applyFade(volume.value, 0, 0.45); // 淡出
        await audioDelay();
        await invoke('player_pause');
    } else { 
        // >>> 播放逻辑 <<<
        if (isPaused.value) { 
            // 1. UI 立即响应
            isPlaying.value = true; 
            isPaused.value = false; 
            startProgressLoop(); // 跑进度条

            // 2. 音频后台处理
            await audioDelay();
            await invoke('player_play');
            await applyFade(0, volume.value, 0.45); // 淡入
        } else { 
            // 全新播放
            await loadAndPlay(); 
        } 
    }
  };

  const nextTrack = async () => { if(queue.value.length===0)return; currentIndex.value = (currentIndex.value + 1) % queue.value.length; await loadAndPlay(); };
  const prevTrack = async () => { if(queue.value.length===0)return; currentIndex.value = currentIndex.value > 0 ? currentIndex.value - 1 : queue.value.length - 1; await loadAndPlay(); };
  const playTrack = (track: Track) => { const idx = queue.value.indexOf(track); if (idx !== -1) { currentIndex.value = idx; loadAndPlay(); } };
  const toggleMode = () => { const modes: PlayMode[] = ['sequence', 'loop', 'shuffle']; playMode.value = modes[(modes.indexOf(playMode.value) + 1) % modes.length]; };

  // --- 9. Seek 逻辑 ---
  const seekTo = async (percent: number) => {
    if (!currentTrack.value || currentTrack.value.duration <= 0) return;

    // 记录原始状态
    const wasPlaying = isPlaying.value && !isPaused.value;

    stopProgressLoop(); 
    isSeeking.value = true; 
    isBuffering.value = true; 
    
    const targetTime = (percent / 100) * currentTrack.value.duration;
    progress.value = percent; 
    currentTime.value = targetTime;
    
    const mySession = playSessionId.value;

    try {
      await audioDelay(); // 强制延迟
      await invoke('player_seek', { time: targetTime });
      
      if (mySession === playSessionId.value) {
          isSeeking.value = false; 
          isBuffering.value = false;
          
          if (wasPlaying) {
              startProgressLoop(); // 继续跑
          } else {
              // 如果原来是暂停的，seek 后应该保持暂停状态
              // 虽然 seek 会让后端处于 active，但我们需要发指令修正
              await invoke('player_pause');
          }
      }
    } catch (e) {
      if (mySession === playSessionId.value) {
          isSeeking.value = false;
          isBuffering.value = false;
      }
    }
  };

  // --- 10. 高性能进度条循环 (RAF) ---
  let rafId: number | null = null;
  let lastFrameTime = 0;

  const startProgressLoop = () => {
    stopProgressLoop();
    lastFrameTime = performance.now();
    
    const loop = (timestamp: number) => {
      if (!isPlaying.value || isPaused.value || !currentTrack.value) return; // 停止条件
      
      const deltaTime = (timestamp - lastFrameTime) / 1000; // 秒
      lastFrameTime = timestamp;

      // 只有在非交互状态下才更新
      if (!isDragging.value && !isBuffering.value && !isSeeking.value) {
          currentTime.value += deltaTime;
          
          // 播放结束判定
          if (currentTime.value >= currentTrack.value.duration) {
             if (playMode.value === 'loop') {
                 currentTime.value = 0;
                 invoke('player_seek', { time: 0.0 });
             } else {
                 nextTrack();
                 return; // 退出循环
             }
          }
          
          if (currentTrack.value.duration > 0) {
              progress.value = (currentTime.value / currentTrack.value.duration) * 100;
          }
      }
      
      rafId = requestAnimationFrame(loop);
    };
    
    rafId = requestAnimationFrame(loop);
  };

  const stopProgressLoop = () => {
    if (rafId !== null) {
      cancelAnimationFrame(rafId);
      rafId = null;
    }
  };

  // 带有状态保护的操作 (用于切换设备/声道)
  const performWithStateCheck = async (action: () => Promise<void>) => {
      const wasPaused = isPaused.value || !isPlaying.value;
      await audioDelay();
      await action();
      if (wasPaused) {
          await invoke('player_pause'); 
          isPlaying.value = false;
          isPaused.value = true;
          stopProgressLoop();
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
  
  watch(volume, (v) => { if(!isFading.value) invoke('player_set_volume', { vol: v / 100.0 }); });

  return { 
    isPlaying, isPaused, volume, progress, currentTime, playMode, queue, currentIndex, currentTrack, activeEngine, showPlaylist, 
    isDragging, isBuffering, isSeeking,
    likedTracks, likedQueue, availableDevices, activeDevice, 
    togglePlay, nextTrack, prevTrack, seekTo, switchEngine, loadAndPlay, initCheck, setNotifier, importTracks, 
    togglePlaylist, toggleMode, toggleLike, isLiked, fetchDevices, setOutputDevice, playTrack, setChannelMode 
  };
});