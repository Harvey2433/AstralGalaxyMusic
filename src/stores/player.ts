import { defineStore } from 'pinia';
import { ref, computed, watch, onMounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

const DEFAULT_COVER = 'https://images.unsplash.com/photo-1614728853913-6591d801d643?q=80&w=400&auto=format&fit=crop';

export interface Track {
  id: string; title: string; artist: string; album: string; cover: string; duration: number; path: string; isAvailable?: boolean; 
}
export type PlayMode = 'sequence' | 'loop' | 'shuffle';

type NotificationCallback = (msg: string, type?: 'info' | 'error' | 'cooling') => void;

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
  
  const isSmtcEnabled = ref(JSON.parse(localStorage.getItem('smtc_enabled') || 'true'));
  const isEngineSwitching = ref(false);
  const hasAudioInitialized = ref(false);
  const lastEngineSwitchTime = ref(0);
  const engineCoolingRemaining = ref(0);

  const isTrackSwitching = ref(false);
  let actionTimeoutId: any = null;
  let coolingTimerId: any = null; // 独立保存冷却计时器的引用，防止冲突

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

  // 🔥 修复关键 1：重新封装的极严谨计时器，真正从调用这一刻开始算冷却！
  const startCoolingTimer = () => {
      if (coolingTimerId) clearInterval(coolingTimerId); // 清除可能存在的旧计时器
      lastEngineSwitchTime.value = Date.now(); // 记录切换成功的那一刻
      engineCoolingRemaining.value = 30;

      coolingTimerId = setInterval(() => {
          const elapsed = (Date.now() - lastEngineSwitchTime.value) / 1000;
          if (elapsed >= 30) {
              engineCoolingRemaining.value = 0;
              clearInterval(coolingTimerId);
              coolingTimerId = null;
          } else {
              engineCoolingRemaining.value = Math.ceil(30 - elapsed);
          }
      }, 1000);
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
              if (wasPlaying) await invoke('player_pause');

              try {
                  isEngineSwitching.value = true;
                  const res = await invoke<string>('init_audio_engine', { engineId: 'ffmpeg' });
                  
                  if (res.includes("READY")) {
                      activeEngine.value = 'ffmpeg';
                      if (currentTrack.value) {
                          await invoke('player_set_volume', { vol: 0.0 });
                          await invoke('player_load_track', { path: currentTrack.value.path });
                          await invoke('player_seek', { time: savedTime });
                          
                          await invoke('player_set_volume', { vol: Math.max(0.01, volume.value / 100.0) });

                          if (wasPlaying) {
                              await executePlayLogic(false); 
                              notifyUI.value?.('FFMPEG ONLINE. RESUMING.');
                          } else {
                              await invoke('player_pause');
                          }
                      }
                      
                      // 🔥 修复关键 2：漫长的下载和解压完全结束后，在此刻才开始 30 秒倒计时！
                      startCoolingTimer();
                  }
              } catch (err) {
                  notifyUI.value?.('FFMPEG FAILED TO LOAD', 'error');
              } finally {
                  isEngineSwitching.value = false;
              }
          } else if (status === 'error') {
              isDownloadingFFmpeg.value = false;
              notifyUI.value?.('DOWNLOAD FAILED. CHECK NETWORK.', 'error');
          }
      });

      await listen('ffmpeg-progress', (e: any) => { ffmpegProgress.value = e.payload as number; });
      await setupEventListeners();
  });

  const switchEngine = async (engineId: string): Promise<'SUCCESS' | 'DOWNLOADING' | 'FAILED' | 'COOLING'> => {
      if (isDownloadingFFmpeg.value || isEngineSwitching.value) return 'FAILED';
      
      const now = Date.now();
      if (now - lastEngineSwitchTime.value < 30000) {
          const remaining = Math.ceil(30 - (now - lastEngineSwitchTime.value) / 1000);
          notifyUI.value?.(`SYSTEM COOLING: ${remaining}S`, 'cooling');
          return 'COOLING';
      }
      
      const previousEngine = activeEngine.value;
      if (previousEngine === engineId) return 'SUCCESS';
      
      isEngineSwitching.value = true;
      notifyUI.value?.(`INITIALIZING ${engineId.toUpperCase()}...`);
      
      try {
          const savedTime = currentTime.value;
          const wasPlaying = isPlaying.value;
          
          if (wasPlaying) {
              await executePauseLogic();
              await new Promise(r => setTimeout(r, 500)); 
          }

          const res = await invoke<string>('init_audio_engine', { engineId });
          
          if (res === "DOWNLOADING") {
              isDownloadingFFmpeg.value = true;
              activeEngine.value = previousEngine;
              if (wasPlaying) await executePlayLogic(false);
              isEngineSwitching.value = false;
              return 'DOWNLOADING';
          }
          
          if (res.includes("READY") || res === "SUCCESS") {
              hasAudioInitialized.value = true;
              activeEngine.value = engineId;
              
              if (currentTrack.value) {
                  await invoke('player_set_volume', { vol: 0.0 });
                  await invoke('player_load_track', { path: currentTrack.value.path });
                  await invoke('player_seek', { time: savedTime });
                  
                  await invoke('player_set_volume', { vol: Math.max(0.01, volume.value / 100.0) });

                  if (wasPlaying) {
                      await executePlayLogic(false); 
                  } else {
                      await invoke('player_pause');
                  }
              }
              
              isEngineSwitching.value = false;
              // 🔥 修复关键 3：全部重载完毕，并且成功发声后，才开始结算冷却时间！
              startCoolingTimer(); 
              return 'SUCCESS';
          }
          throw new Error("Invalid response");
      } catch (e: any) {
          notifyUI.value?.(`SWITCH FAILED: ${e}`, 'error');
          await syncEngine();
          isEngineSwitching.value = false;
          return 'FAILED';
      }
  };

  const executePlayLogic = async (isNewTrack: boolean) => {
      try {
        if (isNewTrack) await invoke('player_set_volume', { vol: Math.max(0.01, volume.value / 100.0) });
        await invoke('player_play');
        isPlaying.value = true;
        isPaused.value = false;
        if (!hasStarted.value) hasStarted.value = true;
        startProgressLoop(); 
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

    if (isTrackSwitching.value || isSeeking.value || isBuffering.value) return;

    if (!isPlaying.value && !hasStarted.value) {
        performTrackSwitch(() => {});
        return;
    }

    const intentToPlay = !isPlaying.value; 
    isPlaying.value = intentToPlay;
    isPaused.value = !intentToPlay; 
    
    if (actionTimeoutId) clearTimeout(actionTimeoutId);
    actionTimeoutId = setTimeout(async () => {
        if (intentToPlay) await executePlayLogic(false);
        else await executePauseLogic();
    }, 50);
  };

  const loadAndPlay = async (): Promise<void> => {
    if (!currentTrack.value) return;
    playSessionId.value++;
    
    isPlaying.value = true;
    isPaused.value = false;
    currentTime.value = 0;
    progress.value = 0;
    stopProgressLoop();

    const mySession = playSessionId.value;

    return new Promise((resolve) => {
        if (actionTimeoutId) clearTimeout(actionTimeoutId);
        actionTimeoutId = setTimeout(async () => {
            let bufferTimeout = setTimeout(() => { isBuffering.value = true; }, 150);

            try {
                if (!hasAudioInitialized.value && activeDevice.value !== 'Default') {
                    await invoke('set_output_device', { device: activeDevice.value });
                    hasAudioInitialized.value = true;
                }

                const duration = await invoke<number>('player_load_track', { path: currentTrack.value!.path });
                
                clearTimeout(bufferTimeout); 

                if (mySession !== playSessionId.value) {
                    isBuffering.value = false;
                    resolve();
                    return;
                }
                
                if (duration > 0.1) currentTrack.value!.duration = duration;
                isBuffering.value = false;
                await executePlayLogic(true);
            } catch (e) {
                clearTimeout(bufferTimeout);
                if (mySession === playSessionId.value) {
                    isPlaying.value = false;
                    isBuffering.value = false;
                    notifyUI.value?.("PLAY FAILED", "error");
                }
            }
            resolve();
        }, 50);
    });
  };

  const performTrackSwitch = async (updateIndexFn: () => void) => {
      if (isTrackSwitching.value) return;
      isTrackSwitching.value = true; 
      
      const isFirstPlay = !hasStarted.value;
      const delay = isFirstPlay ? 0 : 450;
      const wasPlaying = isPlaying.value;
      
      if (wasPlaying && !isFirstPlay) {
          await executePauseLogic(); 
      }
      
      if (delay > 0) {
          setTimeout(async () => {
              updateIndexFn();
              await loadAndPlay();
              isTrackSwitching.value = false; 
          }, delay);
      } else {
          updateIndexFn();
          await loadAndPlay();
          isTrackSwitching.value = false; 
      }
  };

  const nextTrack = async () => { 
      if(queue.value.length === 0) return; 
      await performTrackSwitch(() => {
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
      });
  };

  const prevTrack = async () => { 
      if(queue.value.length === 0) return; 
      await performTrackSwitch(() => {
          currentIndex.value = currentIndex.value > 0 ? currentIndex.value - 1 : queue.value.length - 1; 
      });
  };

  const playTrack = async (track: Track) => { 
      const idx = queue.value.indexOf(track); 
      if (idx !== -1) { await performTrackSwitch(() => { currentIndex.value = idx; }); } 
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
  const toggleMode = () => { const modes: PlayMode[] = ['sequence', 'loop', 'shuffle']; playMode.value = modes[(modes.indexOf(playMode.value) + 1) % modes.length]; };

  const performWithStateCheck = async (action: () => Promise<void>) => {
      const wasPaused = isPaused.value || !isPlaying.value;
      await action();
      if (wasPaused) { await invoke('player_pause'); } 
      else { await invoke('player_set_volume', { vol: Math.max(0.01, volume.value / 100.0) }); }
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
    if (isTrackSwitching.value || isSeeking.value) return; 

    const wasPlaying = isPlaying.value && !isPaused.value;
    
    isSeeking.value = true; 
    stopProgressLoop(); 
    
    isPlaying.value = false;
    isPaused.value = true;
    
    const targetTime = (percent / 100) * currentTrack.value.duration;
    progress.value = percent; 
    currentTime.value = targetTime;
    
    try {
      await invoke('player_seek', { time: targetTime });
    } catch (e) {}

    isSeeking.value = false; 

    if (wasPlaying) {
        isPlaying.value = true;
        isPaused.value = false;
        startProgressLoop();
    }
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
  
  const initCheck = async () => { 
      await setupEventListeners(); 
      queue.value.forEach(track => {
          invoke('check_file_exists', { path: track.path })
            .then((exists) => { track.isAvailable = exists as boolean; })
            .catch(() => { track.isAvailable = false; });
      });
  };

  let rafId: number | null = null;
  let lastFrameTime = 0;
  let listenersBound = false;

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
             if (playMode.value === 'loop') { 
                 currentTime.value = 0; 
                 invoke('player_seek', { time: 0.0 }); 
             } else { 
                 nextTrack(); 
                 return; 
             }
          }
          if (currentTrack.value.duration > 0) progress.value = (currentTime.value / currentTrack.value.duration) * 100;
      }
      rafId = requestAnimationFrame(loop);
    };
    rafId = requestAnimationFrame(loop);
  };
  const stopProgressLoop = () => { if (rafId !== null) { cancelAnimationFrame(rafId); rafId = null; } };

  watch(volume, (v) => { invoke('player_set_volume', { vol: v / 100.0 }); });

  return { 
    isPlaying, isPaused, hasStarted, volume, progress, currentTime, playMode, queue, currentIndex, currentTrack, activeEngine, showPlaylist, 
    isDragging, isBuffering, isSeeking, 
    isDownloadingFFmpeg, ffmpegProgress, 
    hasAudioInitialized, isSmtcEnabled, engineCoolingRemaining,
    likedTracks, likedQueue, availableDevices, activeDevice, 
    togglePlay, nextTrack, prevTrack, seekTo, switchEngine, loadAndPlay, initCheck, setNotifier, importTracks, 
    togglePlaylist, toggleMode, toggleLike, isLiked, fetchDevices, setOutputDevice, playTrack, setChannelMode 
  };
});