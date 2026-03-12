import { defineStore } from 'pinia';
import { ref, watch, onMounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import { Track, NotificationCallback } from './modules/types';
import { usePlaylist } from './modules/playlist';
import { useEngine } from './modules/engine';

const DEFAULT_COVER = 'https://picui.ogmua.cn/s1/2026/03/09/69aeb0db3989e.webp';

export const usePlayerStore = defineStore('player', () => {
  const playlist = usePlaylist();
  const engine = useEngine();

  const isPlaying = ref(false);
  const isPaused = ref(false);
  const hasStarted = ref(false);
  const volume = ref(80);
  const progress = ref(0);
  const currentTime = ref(0);
  
  const isDragging = ref(false);   
  const isBuffering = ref(false);  
  const isSeeking = ref(false);    
  const playSessionId = ref(0);    
  const isTrackSwitching = ref(false);

  const isImporting = ref(false);
  const importCount = ref(0);
  const importTotal = ref(0);
  const importProgress = ref(0);

  let actionTimeoutId: any = null;
  let coolingTimerId: any = null;

  const lastActiveVolume = ref(Number(localStorage.getItem('last_active_vol') || '80'));
  const notifyUI = ref<NotificationCallback | null>(null);
  const setNotifier = (fn: NotificationCallback) => { notifyUI.value = fn; };

  // ==========================================
  // 🔥 终极重构：防爆状态机与无缝淡入淡出引擎
  // ==========================================
  let fadeRafId: number | null = null;
  let actualVolume = 0.0;     // 记录底层真实的当前音量，作为一切无缝衔接的绝对基准
  let lastIpcTime = 0;        // IPC 防风暴节流阀，保护后端不被饿死
  let playActionSession = 0;  // 异步并发锁，解决高频 SMTC 和狂点鼠标导致的逻辑竞态

  // 统一的底层音量发送口
  const setBackendVolume = (v: number) => {
      actualVolume = Math.max(0, Math.min(1, v));
      // 🔥 大道至简：最高给到 0.5 (-6dB) 的黄金动态余量！
      // 配合 2 次幂对数曲线，留足空间给 Windows 音效插件折腾，彻底消灭削波撕裂
      const logVol = Math.pow(actualVolume, 2) * 0.5; 
      invoke('player_set_volume', { vol: logVol }).catch(()=>{});
      lastIpcTime = performance.now();
  };

  const smoothVolumeTransition = (targetVol: number, duration: number, onComplete?: () => void) => {
      // 1. 如果有正在执行的动画，立即砍掉，准备无缝接管
      if (fadeRafId !== null) {
          cancelAnimationFrame(fadeRafId);
          fadeRafId = null;
      }

      // 2. 从当前真实音量起步，保证不管是在淡出中途被打断还是播放中调整，都绝对平滑
      const startVol = actualVolume;
      const diff = targetVol - startVol;

      // 如果差距极小或不需要过渡，直接瞬发
      if (Math.abs(diff) < 0.001 || duration <= 0) {
          setBackendVolume(targetVol);
          if (onComplete) onComplete();
          return;
      }

      let startTime: number | null = null;

      const step = (timestamp: number) => {
          if (startTime === null) startTime = timestamp;
          const elapsed = timestamp - startTime;
          let fadeProgress = Math.min(elapsed / duration, 1.0);

          // 计算当前帧理论音量
          actualVolume = startVol + diff * fadeProgress;

          // 🔥 IPC 节流：强制降频到 ~30fps (33ms)，不让前端高刷卡死后端
          if (timestamp - lastIpcTime > 33 || fadeProgress >= 1) {
              const logVol = Math.pow(actualVolume, 2) * 0.5;
              invoke('player_set_volume', { vol: logVol }).catch(()=>{});
              lastIpcTime = timestamp;
          }

          if (fadeProgress >= 1) {
              fadeRafId = null;
              if (onComplete) onComplete();
          } else {
              fadeRafId = requestAnimationFrame(step);
          }
      };

      fadeRafId = requestAnimationFrame(step);
  };

  const syncEngine = async () => {
      try {
          const realEngine = await invoke<string>('get_current_engine');
          engine.activeEngine.value = realEngine;
      } catch (e) { console.error(e); }
  };

  const startEngineCoolingTimer = () => {
      if (coolingTimerId) clearInterval(coolingTimerId); 
      engine.lastEngineSwitchTime.value = Date.now(); 
      engine.engineCoolingRemaining.value = 30;

      coolingTimerId = setInterval(() => {
          const elapsed = (Date.now() - engine.lastEngineSwitchTime.value) / 1000;
          if (elapsed >= 30) {
              engine.engineCoolingRemaining.value = 0;
              clearInterval(coolingTimerId);
              coolingTimerId = null;
          } else {
              engine.engineCoolingRemaining.value = Math.ceil(30 - elapsed);
          }
      }, 1000);
  };

  onMounted(async () => {
      await syncEngine();
      
      await listen('ffmpeg-status', async (e: any) => {
          const status = e.payload;
          if (status === 'downloading') {
              engine.isDownloadingFFmpeg.value = true;
              engine.ffmpegProgress.value = 0;
              notifyUI.value?.('Fetching engine...', 'info');
          } else if (status === 'extracting') { 
              engine.isDownloadingFFmpeg.value = true;
              engine.ffmpegProgress.value = 99;
              notifyUI.value?.('Extracting core...', 'info');
          } else if (status === 'ready') { 
              engine.isDownloadingFFmpeg.value = false;
              engine.ffmpegProgress.value = 100;
              notifyUI.value?.('Core deployed');
              
              const savedTime = currentTime.value;
              const wasPlaying = isPlaying.value;
              if (wasPlaying) await invoke('player_pause');

              try {
                  engine.isEngineSwitching.value = true;
                  const res = await invoke<string>('init_audio_engine', { engineId: 'ffmpeg' });
                  
                  if (res.includes("READY")) {
                      engine.activeEngine.value = 'ffmpeg';
                      if (playlist.currentTrack.value) {
                          setBackendVolume(0.0);
                          await invoke('player_load_track', { path: playlist.currentTrack.value.path });
                          await invoke('player_seek', { time: savedTime });
                          if (wasPlaying) await executePlayLogic(playActionSession, false); 
                          else await invoke('player_pause');
                      }
                      startEngineCoolingTimer();
                  }
              } catch (err) {
                  notifyUI.value?.('FFmpeg failed', 'error');
              } finally {
                  engine.isEngineSwitching.value = false;
              }
          } else if (status === 'cooling') {
              engine.isDownloadingFFmpeg.value = false;
              engine.isEngineSwitching.value = false;
              startEngineCoolingTimer();
              notifyUI.value?.('System cooling...', 'cooling');
          } else if (status === 'error') {
              engine.isDownloadingFFmpeg.value = false;
              engine.isEngineSwitching.value = false; 
              notifyUI.value?.('Download error', 'error');
          }
      });

      await listen('ffmpeg-progress', (e: any) => { engine.ffmpegProgress.value = e.payload as number; });
      await setupEventListeners();
  });

  const switchEngine = async (engineId: string): Promise<'SUCCESS' | 'DOWNLOADING' | 'FAILED' | 'COOLING'> => {
      if (engine.isDownloadingFFmpeg.value || engine.isEngineSwitching.value || isSeeking.value || isBuffering.value || isDragging.value) {
          notifyUI.value?.('System busy', 'error'); return 'FAILED';
      }
      
      const now = Date.now();
      if (now - engine.lastEngineSwitchTime.value < 30000) {
          const remaining = Math.ceil(30 - (now - engine.lastEngineSwitchTime.value) / 1000);
          notifyUI.value?.(`Cooling: ${remaining}s`, 'cooling'); return 'COOLING';
      }
      
      const previousEngine = engine.activeEngine.value;
      if (previousEngine === engineId) return 'SUCCESS';
      
      engine.isEngineSwitching.value = true;
      notifyUI.value?.(`Initializing ${engineId}...`);
      
      try {
          const savedTime = currentTime.value;
          const wasPlaying = isPlaying.value;
          const session = ++playActionSession; 
          
          if (wasPlaying) {
              await executePauseLogic(session, true); 
              await new Promise(r => setTimeout(r, 500)); 
          }

          const res = await invoke<string>('init_audio_engine', { engineId });
          
          if (res === "DOWNLOADING") {
              engine.isDownloadingFFmpeg.value = true;
              engine.activeEngine.value = previousEngine;
              if (wasPlaying) await executePlayLogic(session, false);
              return 'DOWNLOADING';
          }
          
          if (res.includes("READY") || res === "SUCCESS") {
              engine.hasAudioInitialized.value = true;
              engine.activeEngine.value = engineId;
              
              if (playlist.currentTrack.value) {
                  setBackendVolume(0.0);
                  await invoke('player_load_track', { path: playlist.currentTrack.value.path });
                  await invoke('player_seek', { time: savedTime });
                  
                  if (wasPlaying) await executePlayLogic(session, false); 
                  else await invoke('player_pause');
              }
              
              engine.isEngineSwitching.value = false;
              startEngineCoolingTimer(); 
              return 'SUCCESS';
          }
          throw new Error("Invalid response");
      } catch (e: any) {
          notifyUI.value?.(`Switch error`, 'error');
          await syncEngine();
          engine.isEngineSwitching.value = false; 
          return 'FAILED';
      }
  };

  // ==========================================
  // 核心播放控制：完美抗压防死锁
  // ==========================================
  const executePlayLogic = async (session: number, isNewTrack: boolean) => {
      try {
        if (isNewTrack && playlist.currentTrack.value) {
            setBackendVolume(0.0);
            if (engine.isSmtcEnabled.value) {
                invoke('sync_smtc_metadata', { 
                    title: playlist.currentTrack.value.title, 
                    artist: playlist.currentTrack.value.artist, 
                    cover: playlist.currentTrack.value.cover 
                }).catch(()=>{});
            }
        }

        // 校验：在调用底层前，是否已经被最新点击的 Pause 覆盖了？
        if (session !== playActionSession) return;
        await invoke('player_play').catch(()=>{});
        // 再次校验以防后端 IO 阻塞导致的状态过期
        if (session !== playActionSession) return;

        isPlaying.value = true;
        isPaused.value = false;
        if (!hasStarted.value) hasStarted.value = true;
        startProgressLoop(); 
        
        const targetVol = Math.max(0.001, volume.value / 100.0);
        
        // 触发平滑过渡引擎：如果是从半路暂停回来，这里会完美基于 actualVolume 无缝攀升！
        smoothVolumeTransition(targetVol, 400, () => {
            if (session === playActionSession && engine.isSmtcEnabled.value) {
                invoke('sync_smtc_status', { isPlaying: true }).catch(()=>{});
            }
        });
      } catch (e) { console.error(e); }
  };

  const executePauseLogic = async (session: number, skipFade = false) => {
      try {
          isPlaying.value = false;
          isPaused.value = true;
          stopProgressLoop();
          
          if (skipFade) {
              if (fadeRafId !== null) {
                  cancelAnimationFrame(fadeRafId);
                  fadeRafId = null;
              }
              setBackendVolume(0.0);
              await invoke('player_pause').catch(()=>{});
              if (engine.isSmtcEnabled.value) invoke('sync_smtc_status', { isPlaying: false }).catch(()=>{});
          } else {
              // 淡出到 0：这里降音量，底层仍在播放直到降到 0
              smoothVolumeTransition(0.0, 300, async () => {
                  // 确保淡出完之后，用户没有中途又点了播放
                  if (session === playActionSession) {
                      await invoke('player_pause').catch(()=>{});
                      if (engine.isSmtcEnabled.value) invoke('sync_smtc_status', { isPlaying: false }).catch(()=>{});
                  }
              });
          }
      } catch (e) { console.error(e); }
  };

  const togglePlay = () => {
    if (engine.isEngineSwitching.value || engine.isDownloadingFFmpeg.value) return; 
    if (!playlist.currentTrack.value) return;
    if (isTrackSwitching.value || isSeeking.value || isBuffering.value) return;

    if (!isPlaying.value && !hasStarted.value) {
        performTrackSwitch(() => {});
        return;
    }

    const intentToPlay = !isPlaying.value; 
    isPlaying.value = intentToPlay;
    isPaused.value = !intentToPlay; 
    
    // 🔥 并发霸权锁：抢占令牌，使队列里还在等待的相反操作直接报废
    const session = ++playActionSession; 

    if (actionTimeoutId) clearTimeout(actionTimeoutId);
    
    // 防抖时间 50ms：轻松化解 SMTC 的高频抽风连击
    actionTimeoutId = setTimeout(async () => {
        if (session !== playActionSession) return; 
        if (intentToPlay) await executePlayLogic(session, false);
        else await executePauseLogic(session);
    }, 50);
  };

  const loadAndPlay = async (): Promise<void> => {
    if (!playlist.currentTrack.value) return;
    
    playSessionId.value++;
    isPlaying.value = true;
    isPaused.value = false;
    currentTime.value = 0;
    progress.value = 0;
    stopProgressLoop();

    const mySession = playSessionId.value;
    const actionSession = ++playActionSession;

    return new Promise((resolve) => {
        if (actionTimeoutId) clearTimeout(actionTimeoutId);
        
        actionTimeoutId = setTimeout(async () => {
            if (actionSession !== playActionSession) return resolve();

            let bufferTimeout = setTimeout(() => { isBuffering.value = true; }, 150);

            try {
                if (!engine.hasAudioInitialized.value && engine.activeDevice.value !== 'Default') {
                    await invoke('set_output_device', { device: engine.activeDevice.value });
                    engine.hasAudioInitialized.value = true;
                }

                const duration = await invoke<number>('player_load_track', { path: playlist.currentTrack.value!.path });
                
                clearTimeout(bufferTimeout); 

                if (mySession !== playSessionId.value || actionSession !== playActionSession) {
                    isBuffering.value = false; resolve(); return;
                }
                
                if (duration > 0.1) playlist.currentTrack.value!.duration = duration;
                
                isBuffering.value = false;
                await executePlayLogic(actionSession, true);
            } catch (e) {
                clearTimeout(bufferTimeout);
                if (mySession === playSessionId.value) {
                    isPlaying.value = false;
                    isPaused.value = true;
                    isBuffering.value = false;
                    notifyUI.value?.("Play failed", "error");
                }
            }
            resolve();
        }, 50);
    });
  };

  const performTrackSwitch = async (updateIndexFn: () => void) => {
      if (engine.isEngineSwitching.value || engine.isDownloadingFFmpeg.value) return; 
      if (isTrackSwitching.value) return;
      isTrackSwitching.value = true; 
      
      const isFirstPlay = !hasStarted.value;
      const delay = isFirstPlay ? 0 : 450;
      const wasPlaying = isPlaying.value;
      const actionSession = ++playActionSession;
      
      if (wasPlaying && !isFirstPlay) await executePauseLogic(actionSession); 
      
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
      if(playlist.queue.value.length === 0) return; 
      await performTrackSwitch(() => {
          if (playlist.playMode.value === 'shuffle') {
              const total = playlist.queue.value.length;
              if (total > 1) {
                  const seed = (Date.now() ^ (playlist.currentIndex.value * 123456789));
                  const chaos = Math.abs(Math.sin(seed) * 100000.0);
                  let targetIndex = Math.floor((chaos - Math.floor(chaos)) * total);
                  if (targetIndex === playlist.currentIndex.value) targetIndex = (targetIndex + 1) % total;
                  playlist.currentIndex.value = targetIndex;
              }
          } else {
              playlist.currentIndex.value = (playlist.currentIndex.value + 1) % playlist.queue.value.length; 
          }
      });
  };

  const prevTrack = async () => { 
      if(playlist.queue.value.length === 0) return; 
      await performTrackSwitch(() => {
          if (playlist.currentIndex.value > 0) playlist.currentIndex.value = playlist.currentIndex.value - 1;
          else playlist.currentIndex.value = playlist.queue.value.length - 1;
      });
  };

  const playTrack = async (track: Track) => { 
      const idx = playlist.queue.value.indexOf(track); 
      if (idx !== -1) await performTrackSwitch(() => { playlist.currentIndex.value = idx; }); 
  };

  const setOutputDevice = async (device: string): Promise<'SUCCESS' | 'THROTTLED' | 'FAILED'> => {
      if (engine.isEngineSwitching.value || engine.isDownloadingFFmpeg.value) return 'FAILED';
      if (Date.now() - engine.lastMixerActionTime.value < 1000) return 'THROTTLED';
      engine.lastMixerActionTime.value = Date.now();

      try {
          await invoke('set_output_device', { device });
          engine.activeDevice.value = device;
          engine.hasAudioInitialized.value = true;
          
          if (playlist.currentTrack.value && !isTrackSwitching.value && !isBuffering.value && !isSeeking.value) {
              await invoke('player_seek', { time: currentTime.value });
          }
      } catch (e) { 
          notifyUI.value?.('Device error', 'error'); return 'FAILED'; 
      }
      return 'SUCCESS';
  };

  const setChannelMode = async (mode: number): Promise<'SUCCESS' | 'THROTTLED' | 'FAILED'> => {
      if (engine.isEngineSwitching.value || engine.isDownloadingFFmpeg.value) return 'FAILED';
      if (mode === 2) engine.isTrueSurround.value = false;
      else if (Date.now() - engine.lastMixerActionTime.value < 1000) return 'THROTTLED';
      engine.lastMixerActionTime.value = Date.now();

      if (engine.channelMode.value === mode) return 'SUCCESS';
      engine.channelMode.value = mode;
      localStorage.setItem('channel_mode', mode.toString());
      localStorage.setItem('true_surround', JSON.stringify(engine.isTrueSurround.value));

      const finalMode = (engine.isTrueSurround.value && mode > 2) ? mode + 100 : mode;
      await invoke('player_set_channels', { mode: finalMode });
      
      if (playlist.currentTrack.value && !isTrackSwitching.value && !isBuffering.value && !isSeeking.value) {
          await invoke('player_seek', { time: currentTime.value });
      }
      return 'SUCCESS';
  };

  const toggleTrueSurround = async (): Promise<'SUCCESS' | 'THROTTLED' | 'FAILED'> => {
      if (engine.channelMode.value === 2) return 'FAILED';
      if (engine.isEngineSwitching.value || engine.isDownloadingFFmpeg.value) return 'FAILED';
      if (Date.now() - engine.lastMixerActionTime.value < 1000) return 'THROTTLED';
      engine.lastMixerActionTime.value = Date.now();

      engine.isTrueSurround.value = !engine.isTrueSurround.value;
      localStorage.setItem('true_surround', JSON.stringify(engine.isTrueSurround.value));

      const finalMode = (engine.isTrueSurround.value && engine.channelMode.value > 2) ? engine.channelMode.value + 100 : engine.channelMode.value;
      await invoke('player_set_channels', { mode: finalMode });
      
      if (playlist.currentTrack.value && !isTrackSwitching.value && !isBuffering.value && !isSeeking.value) {
          await invoke('player_seek', { time: currentTime.value });
      }
      return 'SUCCESS';
  };

  const setVolume = (v: number) => {
      if (engine.isEngineSwitching.value || engine.isDownloadingFFmpeg.value || isBuffering.value || isSeeking.value) return;
      volume.value = v;
      if (v > 0) {
          lastActiveVolume.value = v;
          localStorage.setItem('last_active_vol', v.toString());
      }
  };

  const toggleMute = () => {
      if (engine.isEngineSwitching.value || engine.isDownloadingFFmpeg.value || isBuffering.value || isSeeking.value) return;
      if (volume.value > 0) {
          lastActiveVolume.value = volume.value;
          localStorage.setItem('last_active_vol', volume.value.toString());
          volume.value = 0;
      } else {
          volume.value = lastActiveVolume.value;
      }
  };

  const seekTo = async (percent: number) => {
    if (engine.isEngineSwitching.value || engine.isDownloadingFFmpeg.value) return; 
    if (!playlist.currentTrack.value || playlist.currentTrack.value.duration <= 0) return;
    if (isTrackSwitching.value || isSeeking.value) return; 

    const wasPlaying = isPlaying.value && !isPaused.value;
    isSeeking.value = true; 
    const actionSession = ++playActionSession; 
    
    if (wasPlaying) {
        isPlaying.value = false;
        isPaused.value = true;
        stopProgressLoop();
        
        await new Promise<void>(resolve => {
            smoothVolumeTransition(0.0, 150, async () => {
                if (actionSession === playActionSession) {
                    await invoke('player_pause').catch(()=>{});
                }
                resolve();
            });
        });
    } else {
        stopProgressLoop();
        isPlaying.value = false;
        isPaused.value = true;
    }
    
    if (actionSession !== playActionSession) {
        isSeeking.value = false; return;
    }

    const targetTime = (percent / 100) * playlist.currentTrack.value.duration;
    progress.value = percent; 
    currentTime.value = targetTime;
    
    try { await invoke('player_seek', { time: targetTime }); } catch (e) {}

    isSeeking.value = false; 
    if (wasPlaying && actionSession === playActionSession) {
        isPlaying.value = true;
        isPaused.value = false;
        await executePlayLogic(actionSession, false);
    }
  };

  let listenersBound = false;

  const setupEventListeners = async () => {
    if (listenersBound) return;
    listenersBound = true;
    
    await listen<number>('import-start', (e) => {
        importTotal.value = e.payload; importCount.value = 0; importProgress.value = 0;
    });
    
    await listen<Track>('import-track', (e) => {
        const t = e.payload;
        if (!playlist.queue.value.some(track => track.path === t.path)) {
            playlist.queue.value.push({ 
                ...t, 
                id: Date.now().toString() + Math.random().toString(36).substring(2, 8), 
                cover: t.cover === 'DEFAULT_COVER' ? DEFAULT_COVER : t.cover, 
                isAvailable: true 
            });
        }
        importCount.value++;
        if (importTotal.value > 0) importProgress.value = (importCount.value / importTotal.value) * 100;
    });
    
    await listen('import-finish', () => { 
        isImporting.value = false; 
        setTimeout(() => notifyUI.value?.('Library updated'), 400); 
    });
    
    await listen('import-cancel', () => { isImporting.value = false; });
    
    await listen<number>('seek-end', (e) => {
        if (isSeeking.value || isDragging.value || isBuffering.value) return; 
        if (Math.abs(currentTime.value - e.payload) > 0.5) currentTime.value = e.payload;
    });
  };

  const importTracks = async () => { 
      if (isImporting.value) return;
      await setupEventListeners(); 
      isImporting.value = true; importProgress.value = 0; importCount.value = 0; importTotal.value = 0;
      try { await invoke('import_music'); } catch(e) { isImporting.value = false; } 
  };
  
  const initCheck = async () => { 
      await setupEventListeners(); 
      playlist.queue.value.forEach(track => {
          invoke('check_file_exists', { path: track.path })
            .then((exists) => { track.isAvailable = exists as boolean; })
            .catch(() => { track.isAvailable = false; });
      });
  };

  let rafId: number | null = null;
  let lastFrameTime = 0;

  const startProgressLoop = () => {
    stopProgressLoop();
    lastFrameTime = performance.now();
    
    const loop = (timestamp: number) => {
      if (!isPlaying.value || isPaused.value || !playlist.currentTrack.value) return; 
      
      const deltaTime = (timestamp - lastFrameTime) / 1000; 
      lastFrameTime = timestamp;
      
      if (!isDragging.value && !isBuffering.value && !isSeeking.value) {
          currentTime.value += deltaTime;
          if (currentTime.value >= playlist.currentTrack.value.duration) {
             if (playlist.playMode.value === 'loop') { 
                 currentTime.value = 0; invoke('player_seek', { time: 0.0 }); 
             } else { nextTrack(); return; }
          }
          if (playlist.currentTrack.value.duration > 0) progress.value = (currentTime.value / playlist.currentTrack.value.duration) * 100;
      }
      rafId = requestAnimationFrame(loop);
    };
    rafId = requestAnimationFrame(loop);
  };

  const stopProgressLoop = () => { 
      if (rafId !== null) { cancelAnimationFrame(rafId); rafId = null; } 
  };

  // 🔥 监听UI音量条：完美的无缝接入
  watch(volume, (v) => { 
      if (engine.isEngineSwitching.value || engine.isDownloadingFFmpeg.value || isBuffering.value || isSeeking.value) return;

      const target = v / 100.0;
      
      // 只有真正在发声的时候，才去触发底层的音量修改
      if (isPlaying.value && !isPaused.value) {
          if (fadeRafId !== null) {
              // 【无缝衔接】：正在淡入淡出中被打断修改音量
              // 直接以当前 actualVolume 为起点，平滑过渡到新值！
              smoothVolumeTransition(target, 150);
          } else {
              // 【非淡入淡出状态】：直接修改，瞬发响应
              setBackendVolume(target);
          }
      } else {
          // 如果处于完全暂停状态，不发IPC，等待下一次 Play 动作自动平滑推向新音量
      }
  });

  const showCredits = ref(false);
  let wasPlayingBeforeCredits = false;

  const startCredits = async () => {
      wasPlayingBeforeCredits = isPlaying.value && !isPaused.value;
      if (wasPlayingBeforeCredits) {
          const session = ++playActionSession;
          isPlaying.value = false;
          isPaused.value = true;
          await executePauseLogic(session, false); 
      }
      showCredits.value = true;
  };

  const endCredits = async () => {
      showCredits.value = false;
      if (wasPlayingBeforeCredits) {
          const session = ++playActionSession;
          isPlaying.value = true;
          isPaused.value = false;
          await executePlayLogic(session, false); 
      }
  };

  return { 
    ...playlist,
    ...engine,
    isPlaying, isPaused, hasStarted, volume, progress, currentTime, 
    isDragging, isBuffering, isSeeking, playSessionId, isTrackSwitching, 
    isImporting, importCount, importTotal, importProgress, lastActiveVolume, showCredits, 

    setNotifier, setVolume, toggleMute, togglePlay, nextTrack, prevTrack, 
    seekTo, switchEngine, loadAndPlay, initCheck, importTracks, 
    setOutputDevice, playTrack, setChannelMode, toggleTrueSurround, 
    startCredits, endCredits
  };
});