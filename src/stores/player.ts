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
  const isPlaying = ref(false);
  const isPaused = ref(false);
  const volume = ref(80);
  const progress = ref(0);
  const currentTime = ref(0);
  const playMode = ref<PlayMode>('sequence');
  const activeEngine = ref('galaxy');
  const showPlaylist = ref(false);
  
  const isDragging = ref(false);   // UI 拖拽状态
  const isBuffering = ref(false);  // 后端缓冲状态
  const isSeeking = ref(false);    // Seek 交互锁 (新增)

  // 我喜欢与设备状态
  const likedTracks = ref<Set<string>>(new Set(JSON.parse(localStorage.getItem('liked_tracks') || '[]')));
  const availableDevices = ref<string[]>([]);
  const activeDevice = ref('Default');

  const notifyUI = ref<NotificationCallback | null>(null);
  const setNotifier = (fn: NotificationCallback) => { notifyUI.value = fn; };

  const queue = ref<Track[]>([]);
  const currentIndex = ref(0);
  const currentTrack = computed(() => queue.value[currentIndex.value] || null);
  const likedQueue = computed(() => queue.value.filter(t => likedTracks.value.has(t.id)));

  const toggleLike = (track: Track) => {
    if (likedTracks.value.has(track.id)) {
      likedTracks.value.delete(track.id);
      if (notifyUI.value) notifyUI.value('REMOVED FROM LIKES');
    } else {
      likedTracks.value.add(track.id);
      if (notifyUI.value) notifyUI.value('ADDED TO LIKES');
    }
    localStorage.setItem('liked_tracks', JSON.stringify(Array.from(likedTracks.value)));
  };

  const isLiked = (track: Track) => likedTracks.value.has(track.id);

  // 双击播放逻辑
  const playTrack = (track: Track) => {
      const idx = queue.value.indexOf(track);
      if (idx !== -1) {
          currentIndex.value = idx;
          loadAndPlay();
      }
  };

  const fetchDevices = async () => {
    try {
      const devices = await invoke<string[]>('get_output_devices');
      availableDevices.value = devices;
    } catch (e) {}
  };

  // 热切换：设备与声道
  const setOutputDevice = async (device: string) => {
    try {
      await invoke('set_output_device', { device });
      activeDevice.value = device;
      if (notifyUI.value) notifyUI.value(`OUTPUT: ${device}`);
      // 切换设备后，重新 Seek 到当前位置以应用到新设备
      if (currentTrack.value) { invoke('player_seek', { time: currentTime.value }); }
    } catch (e) {
      if (notifyUI.value) notifyUI.value('DEVICE ERROR', 'error');
    }
  };

  const setChannelMode = async (mode: number) => {
      await invoke('player_set_channels', { mode });
      if (currentTrack.value) { invoke('player_seek', { time: currentTime.value }); }
  };

  let listenersBound = false;
  const setupEventListeners = async () => {
    if (listenersBound) return;
    listenersBound = true;
    await listen<Track>('import-track', (event) => {
      const t = event.payload;
      const alreadyExists = queue.value.some(track => track.path === t.path);
      if (!alreadyExists) {
        queue.value.push({
          ...t,
          id: Date.now().toString() + Math.random().toString(36).substr(2, 6),
          cover: t.cover === 'DEFAULT_COVER' ? DEFAULT_COVER : t.cover,
          isAvailable: true
        });
      }
    });
    await listen('import-finish', () => { if (notifyUI.value) notifyUI.value('LIBRARY UPDATED'); });
    await listen('seek-start', () => { isBuffering.value = true; });
    
    // 监听 Seek 结束
    await listen<number>('seek-end', (e) => {
        // 如果当前正处于用户主动 Seek 的锁定状态，忽略后端发来的位置更新，防止进度条跳动
        if (isSeeking.value) return;
        
        isBuffering.value = false;
        // 修复：直接更新，不再判断 progress === 0，解决回拖到0卡死问题
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

  const loadAndPlay = async () => {
    if (!currentTrack.value) return;
    try {
      stopProgressSimulation();
      // 修复：彻底重置所有状态，防止上一首歌的 Seek 状态遗留导致下一首歌播放异常
      isBuffering.value = true; 
      isDragging.value = false; 
      isSeeking.value = false; // 必须重置
      currentTime.value = 0; 
      progress.value = 0; 
      isPaused.value = false;
      
      const duration = await invoke<number>('player_load_track', { path: currentTrack.value.path });
      if (duration > 0.1) currentTrack.value.duration = duration;
      
      await invoke('player_set_volume', { vol: volume.value / 100.0 });
      isPlaying.value = true; isBuffering.value = false;
      startProgressSimulation(); 
    } catch (e) { isPlaying.value = false; isBuffering.value = false; if(notifyUI.value) notifyUI.value("PLAY FAILED", "error"); }
  };

  const togglePlay = async () => {
    if (!currentTrack.value) return;
    if (isPlaying.value) { isPlaying.value = false; isPaused.value = true; await invoke('player_pause'); stopProgressSimulation(); }
    else { if (isPaused.value) { isPlaying.value = true; isPaused.value = false; await invoke('player_play'); startProgressSimulation(); } else { await loadAndPlay(); } }
  };

  const nextTrack = async () => { isPaused.value = false; if(queue.value.length===0)return; currentIndex.value = (currentIndex.value + 1) % queue.value.length; await loadAndPlay(); };
  const prevTrack = async () => { isPaused.value = false; if(queue.value.length===0)return; currentIndex.value = currentIndex.value > 0 ? currentIndex.value - 1 : queue.value.length - 1; await loadAndPlay(); };
  const toggleMode = () => { const modes: PlayMode[] = ['sequence', 'loop', 'shuffle']; playMode.value = modes[(modes.indexOf(playMode.value) + 1) % modes.length]; };
  const togglePlaylist = () => { showPlaylist.value = !showPlaylist.value; };
  
  // 修复：带锁的 Seek 方法，防止进度条回弹
  const seekTo = (percent: number) => {
    if (currentTrack.value && currentTrack.value.duration > 0) {
      stopProgressSimulation(); // 先暂停模拟器
      isSeeking.value = true;   // 加锁
      
      const targetTime = (percent / 100) * currentTrack.value.duration;
      
      // 乐观更新 UI
      progress.value = percent; 
      currentTime.value = targetTime;
      
      // 调用后端
      invoke('player_seek', { time: targetTime })
        .then(() => {
            isSeeking.value = false; // 成功解锁
            if (isPlaying.value) startProgressSimulation(); // 恢复模拟
        })
        .catch(() => {
            isSeeking.value = false; // 失败也要解锁
        });
    }
  };

  let timer: any = null;
  const startProgressSimulation = () => {
    stopProgressSimulation();
    timer = setInterval(() => {
      // 各种阻断条件检查
      if (!isPlaying.value || !currentTrack.value || currentTrack.value.duration <= 0) return;
      if (isDragging.value) return; 
      if (isBuffering.value) return; 
      if (isSeeking.value) return; // 修复：如果正在 Seek，不要更新
      
      if (currentTime.value >= currentTrack.value.duration) { 
          if(playMode.value === 'loop') { 
              currentTime.value = 0; 
              progress.value = 0;
              invoke('player_seek', { time: 0.0 }); 
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
  watch(volume, (v) => { invoke('player_set_volume', { vol: v / 100.0 }); });

  return { 
    isPlaying, isPaused, volume, progress, currentTime, playMode, queue, currentIndex, currentTrack, activeEngine, showPlaylist, isDragging, isBuffering, isSeeking,
    likedTracks, likedQueue, availableDevices, activeDevice, 
    togglePlay, nextTrack, prevTrack, seekTo, switchEngine, loadAndPlay, initCheck, setNotifier, importTracks, togglePlaylist, toggleMode,
    toggleLike, isLiked, fetchDevices, setOutputDevice, playTrack, setChannelMode 
  };
});