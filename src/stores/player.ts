import { defineStore } from 'pinia';
import { ref, computed, watch, onMounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

const DEFAULT_COVER = 'https://picui.ogmua.cn/s1/2026/03/09/69aeb0db3989e.webp';

export interface Track {
  id: string; 
  title: string; 
  artist: string; 
  album: string; 
  cover: string; 
  duration: number; 
  path: string; 
  isAvailable?: boolean; 
}

export type PlayMode = 'sequence' | 'loop' | 'shuffle';

type NotificationCallback = (msg: string, type?: 'info' | 'error' | 'cooling') => void;

export const usePlayerStore = defineStore('player', () => {
  // ==========================================
  // 基础播放状态
  // ==========================================
  const isPlaying = ref(false);
  const isPaused = ref(false);
  const hasStarted = ref(false);
  const volume = ref(80);
  const progress = ref(0);
  const currentTime = ref(0);
  const playMode = ref<PlayMode>('sequence');
  const showPlaylist = ref(false);
  
  // ==========================================
  // 拖拽与缓冲状态
  // ==========================================
  const isDragging = ref(false);   
  const isBuffering = ref(false);  
  const isSeeking = ref(false);    
  const playSessionId = ref(0);    
  
  // ==========================================
  // 引擎与系统状态
  // ==========================================
  const activeEngine = ref('galaxy'); 
  const isDownloadingFFmpeg = ref(false);
  const ffmpegProgress = ref(0);
  
  const isSmtcEnabled = ref(JSON.parse(localStorage.getItem('smtc_enabled') || 'true'));
  const isEngineSwitching = ref(false);
  const hasAudioInitialized = ref(false);
  
  const lastEngineSwitchTime = ref(0);
  const engineCoolingRemaining = ref(0);
  
  // 极简无锁机制：混音器 1 秒硬拦截
  const lastMixerActionTime = ref(0);

  const channelMode = ref(Number(localStorage.getItem('channel_mode') || '2'));
  const isTrueSurround = ref(JSON.parse(localStorage.getItem('true_surround') || 'false'));

  // ==========================================
  // 导入状态监控
  // ==========================================
  const isImporting = ref(false);
  const importCount = ref(0);
  const importTotal = ref(0);
  const importProgress = ref(0);

  const isTrackSwitching = ref(false);
  let actionTimeoutId: any = null;
  let coolingTimerId: any = null;

  // ==========================================
  // 队列与设备状态
  // ==========================================
  const likedTracks = ref<Set<string>>(new Set(JSON.parse(localStorage.getItem('liked_tracks') || '[]')));
  const availableDevices = ref<string[]>([]);
  const activeDevice = ref('Default');
  const notifyUI = ref<NotificationCallback | null>(null);
  
  const setNotifier = (fn: NotificationCallback) => { 
      notifyUI.value = fn; 
  };

  const queue = ref<Track[]>([]);
  const currentIndex = ref(0);
  
  const currentTrack = computed(() => {
      if (queue.value.length === 0 || currentIndex.value < 0 || currentIndex.value >= queue.value.length) {
          return null;
      }
      return queue.value[currentIndex.value];
  });
  
  const likedQueue = computed(() => {
      return queue.value.filter(t => likedTracks.value.has(t.id));
  });

  // ==========================================
  // 🔥 核心魔法：470ms 线性渐变与 SMTC 同步拦截器
  // ==========================================
  let fadeRafId: number | null = null;
  let currentFadeVolume = 0; 
  let targetPlayState = false; 

  const applyVolumeFade = (targetVol: number, duration: number, onComplete?: () => void) => {
      if (fadeRafId !== null) {
          cancelAnimationFrame(fadeRafId);
          fadeRafId = null;
      }
      
      const startVol = currentFadeVolume;
      const diff = targetVol - startVol;
      
      if (Math.abs(diff) < 0.001 || duration <= 0) {
          currentFadeVolume = targetVol;
          invoke('player_set_volume', { vol: currentFadeVolume });
          if (onComplete) onComplete();
          return;
      }
      
      let startTime: number | null = null;
      
      const step = (timestamp: number) => {
          if (startTime === null) startTime = timestamp;
          const elapsed = timestamp - startTime;
          let fadeProgress = Math.min(elapsed / duration, 1.0);
          
          currentFadeVolume = startVol + diff * fadeProgress;
          invoke('player_set_volume', { vol: currentFadeVolume });
          
          if (fadeProgress >= 1) {
              fadeRafId = null;
              if (onComplete) onComplete(); 
          } else {
              fadeRafId = requestAnimationFrame(step);
          }
      };
      
      fadeRafId = requestAnimationFrame(step); 
  };

  // ==========================================
  // 引擎初始化与冷却系统
  // ==========================================
  const syncEngine = async () => {
      try {
          const realEngine = await invoke<string>('get_current_engine');
          activeEngine.value = realEngine;
      } catch (e) { 
          console.error("Sync Engine Failed:", e); 
      }
  };

  const startEngineCoolingTimer = () => {
      if (coolingTimerId) {
          clearInterval(coolingTimerId); 
      }
      lastEngineSwitchTime.value = Date.now(); 
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
              notifyUI.value?.('Fetching engine...', 'info');
              
          } else if (status === 'extracting') { 
              isDownloadingFFmpeg.value = true;
              ffmpegProgress.value = 99;
              notifyUI.value?.('Extracting core...', 'info');
              
          } else if (status === 'ready') { 
              isDownloadingFFmpeg.value = false;
              ffmpegProgress.value = 100;
              notifyUI.value?.('Core deployed');
              
              const savedTime = currentTime.value;
              const wasPlaying = isPlaying.value;
              
              if (wasPlaying) {
                  await invoke('player_pause');
              }

              try {
                  isEngineSwitching.value = true;
                  const res = await invoke<string>('init_audio_engine', { engineId: 'ffmpeg' });
                  
                  if (res.includes("READY")) {
                      activeEngine.value = 'ffmpeg';
                      if (currentTrack.value) {
                          await invoke('player_set_volume', { vol: 0.0 });
                          await invoke('player_load_track', { path: currentTrack.value.path });
                          await invoke('player_seek', { time: savedTime });
                          
                          if (wasPlaying) {
                              await executePlayLogic(false); 
                              notifyUI.value?.('FFmpeg online');
                          } else {
                              await invoke('player_pause');
                          }
                      }
                      startEngineCoolingTimer();
                  }
              } catch (err) {
                  notifyUI.value?.('FFmpeg failed', 'error');
              } finally {
                  isEngineSwitching.value = false;
              }
              
          } else if (status === 'cooling') {
              isDownloadingFFmpeg.value = false;
              isEngineSwitching.value = false;
              startEngineCoolingTimer();
              notifyUI.value?.('System cooling...', 'cooling');
              
          } else if (status === 'error') {
              isDownloadingFFmpeg.value = false;
              isEngineSwitching.value = false; 
              notifyUI.value?.('Download error', 'error');
          }
      });

      await listen('ffmpeg-progress', (e: any) => { 
          ffmpegProgress.value = e.payload as number; 
      });
      
      await setupEventListeners();
  });

  const switchEngine = async (engineId: string): Promise<'SUCCESS' | 'DOWNLOADING' | 'FAILED' | 'COOLING'> => {
      if (isDownloadingFFmpeg.value || isEngineSwitching.value || isSeeking.value || isBuffering.value || isDragging.value) {
          notifyUI.value?.('System busy', 'error');
          return 'FAILED';
      }
      
      const now = Date.now();
      if (now - lastEngineSwitchTime.value < 30000) {
          const remaining = Math.ceil(30 - (now - lastEngineSwitchTime.value) / 1000);
          notifyUI.value?.(`Cooling: ${remaining}s`, 'cooling');
          return 'COOLING';
      }
      
      const previousEngine = activeEngine.value;
      if (previousEngine === engineId) {
          return 'SUCCESS';
      }
      
      isEngineSwitching.value = true;
      notifyUI.value?.(`Initializing ${engineId}...`);
      
      try {
          const savedTime = currentTime.value;
          const wasPlaying = isPlaying.value;
          
          if (wasPlaying) {
              await executePauseLogic(true); 
              await new Promise(r => setTimeout(r, 500)); 
          }

          const res = await invoke<string>('init_audio_engine', { engineId });
          
          if (res === "DOWNLOADING") {
              isDownloadingFFmpeg.value = true;
              activeEngine.value = previousEngine;
              if (wasPlaying) {
                  await executePlayLogic(false);
              }
              return 'DOWNLOADING';
          }
          
          if (res.includes("READY") || res === "SUCCESS") {
              hasAudioInitialized.value = true;
              activeEngine.value = engineId;
              
              if (currentTrack.value) {
                  await invoke('player_set_volume', { vol: 0.0 });
                  await invoke('player_load_track', { path: currentTrack.value.path });
                  await invoke('player_seek', { time: savedTime });
                  
                  if (wasPlaying) {
                      await executePlayLogic(false); 
                  } else {
                      await invoke('player_pause');
                  }
              }
              
              isEngineSwitching.value = false;
              startEngineCoolingTimer(); 
              return 'SUCCESS';
          }
          throw new Error("Invalid response");
      } catch (e: any) {
          notifyUI.value?.(`Switch error`, 'error');
          await syncEngine();
          isEngineSwitching.value = false; 
          return 'FAILED';
      }
  };

  // ==========================================
  // 核心播放控制系统
  // ==========================================
  const executePlayLogic = async (isNewTrack: boolean) => {
      try {
        targetPlayState = true;
        
        if (isNewTrack && currentTrack.value) {
            currentFadeVolume = 0.0;
            await invoke('player_set_volume', { vol: 0.0 });
            if (isSmtcEnabled.value) {
                invoke('sync_smtc_metadata', { 
                    title: currentTrack.value.title, 
                    artist: currentTrack.value.artist, 
                    cover: currentTrack.value.cover 
                });
            }
        }

        await invoke('player_play');
        isPlaying.value = true;
        isPaused.value = false;
        
        if (!hasStarted.value) {
            hasStarted.value = true;
        }
        
        startProgressLoop(); 
        
        const targetVol = Math.max(0.001, volume.value / 100.0);
        
        applyVolumeFade(targetVol, 470, () => {
            if (targetPlayState === true && isSmtcEnabled.value) {
                invoke('sync_smtc_status', { isPlaying: true });
            }
        });
      } catch (e) { 
          console.error(e); 
      }
  };

  const executePauseLogic = async (skipFade = false) => {
      try {
          targetPlayState = false;
          isPlaying.value = false;
          isPaused.value = true;
          stopProgressLoop();
          
          if (skipFade) {
              await invoke('player_pause');
              if (isSmtcEnabled.value) {
                  invoke('sync_smtc_status', { isPlaying: false });
              }
          } else {
              applyVolumeFade(0.0, 470, async () => {
                  if (targetPlayState === false) {
                      await invoke('player_pause');
                      if (isSmtcEnabled.value) {
                          invoke('sync_smtc_status', { isPlaying: false });
                      }
                  }
              });
          }
      } catch (e) { 
          console.error(e); 
      }
  };

  const togglePlay = () => {
    if (isEngineSwitching.value || isDownloadingFFmpeg.value) return; 

    if (!currentTrack.value) return;
    if (isTrackSwitching.value || isSeeking.value || isBuffering.value) return;

    if (!isPlaying.value && !hasStarted.value) {
        performTrackSwitch(() => {});
        return;
    }

    const intentToPlay = !isPlaying.value; 
    isPlaying.value = intentToPlay;
    isPaused.value = !intentToPlay; 
    
    if (actionTimeoutId) {
        clearTimeout(actionTimeoutId);
    }
    
    actionTimeoutId = setTimeout(async () => {
        if (intentToPlay) {
            await executePlayLogic(false);
        } else {
            await executePauseLogic();
        }
    }, 10);
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
        if (actionTimeoutId) {
            clearTimeout(actionTimeoutId);
        }
        
        actionTimeoutId = setTimeout(async () => {
            let bufferTimeout = setTimeout(() => { 
                isBuffering.value = true; 
            }, 150);

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
                
                if (duration > 0.1) {
                    currentTrack.value!.duration = duration;
                }
                
                isBuffering.value = false;
                await executePlayLogic(true);
            } catch (e) {
                clearTimeout(bufferTimeout);
                if (mySession === playSessionId.value) {
                    isPlaying.value = false;
                    isBuffering.value = false;
                    notifyUI.value?.("Play failed", "error");
                }
            }
            resolve();
        }, 50);
    });
  };

  const performTrackSwitch = async (updateIndexFn: () => void) => {
      if (isEngineSwitching.value || isDownloadingFFmpeg.value) return; 

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
                  if (targetIndex === currentIndex.value) {
                      targetIndex = (targetIndex + 1) % total;
                  }
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
          if (currentIndex.value > 0) {
              currentIndex.value = currentIndex.value - 1;
          } else {
              currentIndex.value = queue.value.length - 1;
          }
      });
  };

  const playTrack = async (track: Track) => { 
      const idx = queue.value.indexOf(track); 
      if (idx !== -1) { 
          await performTrackSwitch(() => { 
              currentIndex.value = idx; 
          }); 
      } 
  };

  const toggleLike = (track: Track) => {
    if (likedTracks.value.has(track.id)) { 
        likedTracks.value.delete(track.id); 
    } else { 
        likedTracks.value.add(track.id); 
    }
    localStorage.setItem('liked_tracks', JSON.stringify(Array.from(likedTracks.value)));
  };

  const isLiked = (track: Track) => {
      return likedTracks.value.has(track.id);
  };

  const togglePlaylist = () => { 
      showPlaylist.value = !showPlaylist.value; 
  };

  const fetchDevices = async () => { 
    try { 
      const realDevices = await invoke<string[]>('get_output_devices');
      availableDevices.value = ['Default', ...realDevices];
    } catch (e) { 
      availableDevices.value = ['Default']; 
    } 
  };

  const toggleMode = () => { 
      const modes: PlayMode[] = ['sequence', 'loop', 'shuffle']; 
      const currentIdx = modes.indexOf(playMode.value);
      playMode.value = modes[(currentIdx + 1) % modes.length]; 
  };

  // ==========================================
  // 混音器控制 (无锁版)
  // ==========================================
  const setOutputDevice = async (device: string): Promise<'SUCCESS' | 'THROTTLED' | 'FAILED'> => {
      if (isEngineSwitching.value || isDownloadingFFmpeg.value) return 'FAILED';
      
      if (Date.now() - lastMixerActionTime.value < 1000) return 'THROTTLED';
      lastMixerActionTime.value = Date.now();

      try {
          await invoke('set_output_device', { device });
          activeDevice.value = device;
          hasAudioInitialized.value = true;
          
          if (currentTrack.value && !isTrackSwitching.value && !isBuffering.value && !isSeeking.value) {
              await invoke('player_seek', { time: currentTime.value });
          }
      } catch (e) { 
          notifyUI.value?.('Device error', 'error'); 
          return 'FAILED'; 
      }

      return 'SUCCESS';
  };

  const setChannelMode = async (mode: number): Promise<'SUCCESS' | 'THROTTLED' | 'FAILED'> => {
      if (isEngineSwitching.value || isDownloadingFFmpeg.value) return 'FAILED';
      
      if (mode === 2) {
          isTrueSurround.value = false;
      } else {
          if (Date.now() - lastMixerActionTime.value < 1000) return 'THROTTLED';
      }
      
      lastMixerActionTime.value = Date.now();

      if (channelMode.value === mode) return 'SUCCESS';

      channelMode.value = mode;
      localStorage.setItem('channel_mode', mode.toString());
      localStorage.setItem('true_surround', JSON.stringify(isTrueSurround.value));

      const finalMode = (isTrueSurround.value && mode > 2) ? mode + 100 : mode;
      await invoke('player_set_channels', { mode: finalMode });
      
      if (currentTrack.value && !isTrackSwitching.value && !isBuffering.value && !isSeeking.value) {
          await invoke('player_seek', { time: currentTime.value });
      }

      return 'SUCCESS';
  };

  const toggleTrueSurround = async (): Promise<'SUCCESS' | 'THROTTLED' | 'FAILED'> => {
      if (channelMode.value === 2) return 'FAILED';
      if (isEngineSwitching.value || isDownloadingFFmpeg.value) return 'FAILED';
      
      if (Date.now() - lastMixerActionTime.value < 1000) return 'THROTTLED';
      lastMixerActionTime.value = Date.now();

      isTrueSurround.value = !isTrueSurround.value;
      localStorage.setItem('true_surround', JSON.stringify(isTrueSurround.value));

      const finalMode = (isTrueSurround.value && channelMode.value > 2) ? channelMode.value + 100 : channelMode.value;
      await invoke('player_set_channels', { mode: finalMode });
      
      if (currentTrack.value && !isTrackSwitching.value && !isBuffering.value && !isSeeking.value) {
          await invoke('player_seek', { time: currentTime.value });
      }

      return 'SUCCESS';
  };

  // ==========================================
  // 🔥 终极防爆音秒切逻辑
  // ==========================================
  const seekTo = async (percent: number) => {
    if (isEngineSwitching.value || isDownloadingFFmpeg.value) return; 

    if (!currentTrack.value || currentTrack.value.duration <= 0) return;
    if (isTrackSwitching.value || isSeeking.value) return; 

    const wasPlaying = isPlaying.value && !isPaused.value;
    
    isSeeking.value = true; 
    
    if (wasPlaying) {
        targetPlayState = false;
        isPlaying.value = false;
        isPaused.value = true;
        stopProgressLoop();
        
        await new Promise<void>(resolve => {
            applyVolumeFade(0.0, 150, async () => {
                await invoke('player_pause');
                resolve();
            });
        });
    } else {
        stopProgressLoop();
        isPlaying.value = false;
        isPaused.value = true;
    }
    
    const targetTime = (percent / 100) * currentTrack.value.duration;
    progress.value = percent; 
    currentTime.value = targetTime;
    
    try {
      await invoke('player_seek', { time: targetTime });
    } catch (e) {
      console.error("Seek failed:", e);
    }

    isSeeking.value = false; 

    if (wasPlaying) {
        await executePlayLogic(false);
    }
  };

  // ==========================================
  // 事件监听与导入系统
  // ==========================================
  let listenersBound = false;

  const setupEventListeners = async () => {
    if (listenersBound) return;
    listenersBound = true;
    
    await listen<number>('import-start', (e) => {
        importTotal.value = e.payload;
        importCount.value = 0;
        importProgress.value = 0;
    });
    
    await listen<Track>('import-track', (e) => {
        const t = e.payload;
        if (!queue.value.some(track => track.path === t.path)) {
            queue.value.push({ 
                ...t, 
                id: Date.now().toString() + Math.random().toString(36).substring(2, 8), 
                cover: t.cover === 'DEFAULT_COVER' ? DEFAULT_COVER : t.cover, 
                isAvailable: true 
            });
        }
        importCount.value++;
        if (importTotal.value > 0) {
            importProgress.value = (importCount.value / importTotal.value) * 100;
        }
    });
    
    await listen('import-finish', () => { 
        isImporting.value = false; 
        setTimeout(() => {
            notifyUI.value?.('Library updated'); 
        }, 400); 
    });
    
    await listen('import-cancel', () => { 
        isImporting.value = false; 
    });
    
    await listen<number>('seek-end', (e) => {
        if (isSeeking.value || isDragging.value || isBuffering.value) return; 
        if (Math.abs(currentTime.value - e.payload) > 0.5) {
            currentTime.value = e.payload;
        }
    });
  };

  const importTracks = async () => { 
      if (isImporting.value) return;
      
      await setupEventListeners(); 
      isImporting.value = true;
      importProgress.value = 0;
      importCount.value = 0;
      importTotal.value = 0;
      
      try { 
          await invoke('import_music'); 
      } catch(e) {
          isImporting.value = false;
      } 
  };
  
  const initCheck = async () => { 
      await setupEventListeners(); 
      queue.value.forEach(track => {
          invoke('check_file_exists', { path: track.path })
            .then((exists) => { 
                track.isAvailable = exists as boolean; 
            })
            .catch(() => { 
                track.isAvailable = false; 
            });
      });
  };

  // ==========================================
  // 渲染循环与进度同步
  // ==========================================
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
             if (playMode.value === 'loop') { 
                 currentTime.value = 0; 
                 invoke('player_seek', { time: 0.0 }); 
             } else { 
                 nextTrack(); 
                 return; 
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

  watch(volume, (v) => { 
      if (!isEngineSwitching.value && !isDownloadingFFmpeg.value) {
          invoke('player_set_volume', { vol: v / 100.0 }); 
      }
  });

  // ==========================================
  // 🔥 贡献者页面专属序列引擎
  // ==========================================
  const showCredits = ref(false);
  let wasPlayingBeforeCredits = false;

  const startCredits = async () => {
      wasPlayingBeforeCredits = isPlaying.value && !isPaused.value;
      if (wasPlayingBeforeCredits) {
          await executePauseLogic(); 
      }
      showCredits.value = true;
  };

  const endCredits = async () => {
      showCredits.value = false;
      if (wasPlayingBeforeCredits) {
          await executePlayLogic(false); 
      }
  };

  return { 
    isPlaying, 
    isPaused, 
    hasStarted, 
    volume, 
    progress, 
    currentTime, 
    playMode, 
    queue, 
    currentIndex, 
    currentTrack, 
    activeEngine, 
    showPlaylist, 
    isDragging, 
    isBuffering, 
    isSeeking, 
    isDownloadingFFmpeg, 
    ffmpegProgress, 
    hasAudioInitialized, 
    isSmtcEnabled, 
    engineCoolingRemaining, 
    isEngineSwitching, 
    channelMode, 
    isTrueSurround, 
    isImporting, 
    importCount, 
    importTotal, 
    importProgress,
    likedTracks, 
    likedQueue, 
    availableDevices, 
    activeDevice, 
    showCredits, 
    
    startCredits, 
    endCredits,
    togglePlay, 
    nextTrack, 
    prevTrack, 
    seekTo, 
    switchEngine, 
    loadAndPlay, 
    initCheck, 
    setNotifier, 
    importTracks, 
    togglePlaylist, 
    toggleMode, 
    toggleLike, 
    isLiked, 
    fetchDevices, 
    setOutputDevice, 
    playTrack, 
    setChannelMode, 
    toggleTrueSurround 
  };
});