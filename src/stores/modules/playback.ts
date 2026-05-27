import { Ref, computed } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Track, NotificationCallback } from './types';
import { getOriginalCover } from './playlist';

export function usePlayback(deps: {
    isPlaying: Ref<boolean>;
    isPaused: Ref<boolean>;
    hasStarted: Ref<boolean>;
    isSystemBusy: Ref<boolean>;
    isBuffering: Ref<boolean>;
    isSeeking: Ref<boolean>;
    isDragging: Ref<boolean>;
    isTrackSwitching: Ref<boolean>;
    currentTime: Ref<number>;
    progress: Ref<number>;
    playSessionId: Ref<number>;
    volume: Ref<number>;
    notifyUI: Ref<NotificationCallback | null>;

    queue: Ref<Track[]>;
    currentIndex: Ref<number>;
    currentTrack: Ref<Track | null>;
    playMode: Ref<string>;

    activeEngine: Ref<string>;
    isEngineSwitching: Ref<boolean>;
    isDownloadingFFmpeg: Ref<boolean>;
    hasAudioInitialized: Ref<boolean>;
    activeDevice: Ref<string>;
    isSmtcEnabled: Ref<boolean>;
    lastEngineSwitchTime: Ref<number>;
    lastMixerActionTime: Ref<number>;
    engineCoolingRemaining: Ref<number>;
    channelMode: Ref<number>;
    isTrueSurround: Ref<boolean>;

    setBackendVolume: (v: number) => void;
    smoothVolumeTransition: (targetVol: number, duration: number, onComplete?: () => void) => void;
    cancelFade: () => void;

    startProgressLoop: () => void;
    stopProgressLoop: () => void;
    refreshAnchor: () => Promise<void>;
    setAnchorLocal: (time: number) => void;
    freezeAnchor: () => void;
}) {
    let actionTimeoutId: any = null;
    let playActionSession = 0;
    let coolingTimerId: any = null;

    const getPlayActionSession = () => playActionSession;
    const incrementPlayActionSession = () => ++playActionSession;

    const getBackendCover = (): string => {
        if (!deps.currentTrack.value) return '';
        return getOriginalCover(deps.currentTrack.value.cover);
    };

    const syncEngine = async () => {
        try {
            const realEngine = await invoke<string>('get_current_engine');
            deps.activeEngine.value = realEngine;
        } catch (e) { console.error(e); }
    };

    const startEngineCoolingTimer = () => {
        if (coolingTimerId) clearInterval(coolingTimerId);
        deps.lastEngineSwitchTime.value = Date.now();
        deps.engineCoolingRemaining.value = 30;
        coolingTimerId = setInterval(() => {
            const elapsed = (Date.now() - deps.lastEngineSwitchTime.value) / 1000;
            if (elapsed >= 30) {
                deps.engineCoolingRemaining.value = 0;
                clearInterval(coolingTimerId);
                coolingTimerId = null;
            } else {
                deps.engineCoolingRemaining.value = Math.ceil(30 - elapsed);
            }
        }, 1000);
    };

    const executePlayLogic = async (session: number, isNewTrack: boolean) => {
        try {
            if (isNewTrack && deps.currentTrack.value) {
                deps.setBackendVolume(0.0);
            }

            if (session !== playActionSession) return;
            await invoke('player_play').catch(() => {});
            if (session !== playActionSession) return;

            deps.isPlaying.value = true;
            deps.isPaused.value = false;
            if (!deps.hasStarted.value) deps.hasStarted.value = true;

            await deps.refreshAnchor();
            deps.startProgressLoop();

            const targetVol = Math.max(0.001, deps.volume.value / 100.0);
            deps.smoothVolumeTransition(targetVol, 50, () => {
                if (session === playActionSession && deps.isSmtcEnabled.value) {
                    invoke('sync_smtc_status', { isPlaying: true }).catch(() => {});
                }
            });
        } catch (e) { console.error(e); }
    };

    const executePauseLogic = async (session: number, skipFade = false) => {
        try {
            deps.isPlaying.value = false;
            deps.isPaused.value = true;
            deps.stopProgressLoop();
            deps.freezeAnchor();

            if (skipFade) {
                deps.cancelFade();
                deps.setBackendVolume(0.0);
                await invoke('player_pause').catch(() => {});
                if (deps.isSmtcEnabled.value) invoke('sync_smtc_status', { isPlaying: false }).catch(() => {});
            } else {
                deps.smoothVolumeTransition(0.0, 300, async () => {
                    if (session === playActionSession) {
                        await invoke('player_pause').catch(() => {});
                        if (deps.isSmtcEnabled.value) invoke('sync_smtc_status', { isPlaying: false }).catch(() => {});
                    }
                });
            }
        } catch (e) { console.error(e); }
    };

    const togglePlay = () => {
        if (deps.isSystemBusy.value || deps.isEngineSwitching.value) return;
        if (!deps.currentTrack.value) return;
        if (deps.isTrackSwitching.value || deps.isSeeking.value || deps.isBuffering.value) return;

        if (!deps.isPlaying.value && !deps.hasStarted.value) {
            performTrackSwitch(() => {});
            return;
        }

        const intentToPlay = !deps.isPlaying.value;
        deps.isPlaying.value = intentToPlay;
        deps.isPaused.value = !intentToPlay;

        const session = ++playActionSession;
        if (actionTimeoutId) clearTimeout(actionTimeoutId);

        actionTimeoutId = setTimeout(async () => {
            if (session !== playActionSession) return;
            if (intentToPlay) await executePlayLogic(session, false);
            else await executePauseLogic(session);
        }, 50);
    };

    const loadAndPlay = async (): Promise<void> => {
        if (!deps.currentTrack.value) return;

        deps.playSessionId.value++;
        deps.isPlaying.value = true;
        deps.isPaused.value = false;
        deps.currentTime.value = 0;
        deps.progress.value = 0;
        deps.setAnchorLocal(0);
        deps.stopProgressLoop();

        const mySession = deps.playSessionId.value;
        const actionSession = ++playActionSession;

        return new Promise((resolve) => {
            if (actionTimeoutId) clearTimeout(actionTimeoutId);

            actionTimeoutId = setTimeout(async () => {
                if (actionSession !== playActionSession) return resolve();

                let bufferTimeout = setTimeout(() => { deps.isBuffering.value = true; }, 150);

                try {
                    if (!deps.hasAudioInitialized.value && deps.activeDevice.value !== 'Default') {
                        await invoke('set_output_device', { device: deps.activeDevice.value });
                        deps.hasAudioInitialized.value = true;
                    }

                    const duration = await invoke<number>('player_load_track', { path: deps.currentTrack.value!.path });
                    clearTimeout(bufferTimeout);

                    if (mySession !== deps.playSessionId.value || actionSession !== playActionSession) {
                        deps.isBuffering.value = false;
                        resolve();
                        return;
                    }

                    if (duration > 0.1) deps.currentTrack.value!.duration = duration;
                    deps.isBuffering.value = false;
                    await executePlayLogic(actionSession, true);
                } catch (e) {
                    clearTimeout(bufferTimeout);
                    if (mySession === deps.playSessionId.value) {
                        deps.isPlaying.value = false;
                        deps.isPaused.value = true;
                        deps.isBuffering.value = false;
                        deps.notifyUI.value?.("Play failed", "error");
                    }
                } finally {
                    resolve();
                }
            }, 50);
        });
    };

    const performTrackSwitch = async (updateIndexFn: () => void) => {
        if (deps.isSystemBusy.value || deps.isEngineSwitching.value) return;
        if (deps.isTrackSwitching.value) return;
        deps.isTrackSwitching.value = true;
        deps.isSystemBusy.value = true;
        const isFirstPlay = !deps.hasStarted.value;
        const wasPlaying = deps.isPlaying.value;
        const actionSession = ++playActionSession;

        updateIndexFn();

        // SMTC metadata is handled by App.vue watcher on currentTrack.id change.
        // No need to send here — it would cause duplicate writes.

        await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));

        if (wasPlaying && !isFirstPlay) {
            await executePauseLogic(actionSession, true);
        }

        if (!isFirstPlay) {
            await new Promise(r => setTimeout(r, 150));
        }

        await loadAndPlay();
        deps.isTrackSwitching.value = false;
        deps.isSystemBusy.value = false;
    };

    const nextTrack = async () => {
        if (deps.queue.value.length === 0) return;
        await performTrackSwitch(() => {
            if (deps.playMode.value === 'shuffle') {
                const total = deps.queue.value.length;
                if (total > 1) {
                    const seed = (Date.now() ^ (deps.currentIndex.value * 123456789));
                    const chaos = Math.abs(Math.sin(seed) * 100000.0);
                    let targetIndex = Math.floor((chaos - Math.floor(chaos)) * total);
                    if (targetIndex === deps.currentIndex.value) targetIndex = (targetIndex + 1) % total;
                    deps.currentIndex.value = targetIndex;
                }
            } else {
                deps.currentIndex.value = (deps.currentIndex.value + 1) % deps.queue.value.length;
            }
        });
    };

    const prevTrack = async () => {
        if (deps.queue.value.length === 0) return;
        await performTrackSwitch(() => {
            if (deps.currentIndex.value > 0) deps.currentIndex.value = deps.currentIndex.value - 1;
            else deps.currentIndex.value = deps.queue.value.length - 1;
        });
    };

    const playTrack = async (track: Track) => {
        let idx = deps.queue.value.findIndex(t => t.id === track.id);
        if (idx === -1) {
            deps.queue.value.push({ ...track, isAvailable: true });
            idx = deps.queue.value.length - 1;
        }
        await performTrackSwitch(() => { deps.currentIndex.value = idx; });
    };

    const seekTo = async (percent: number) => {
        if (deps.isEngineSwitching.value) return;
        if (!deps.currentTrack.value || deps.currentTrack.value.duration <= 0) return;

        const wasPlaying = deps.isPlaying.value && !deps.isPaused.value;
        deps.isSeeking.value = true;

        const actionSession = ++playActionSession;

        if (wasPlaying) {
            deps.isPlaying.value = false;
            deps.isPaused.value = true;
            deps.stopProgressLoop();
            await invoke('player_pause').catch(() => {});
        }

        if (actionSession !== playActionSession) {
            deps.isSeeking.value = false;
            return;
        }

        const targetTime = (percent / 100) * deps.currentTrack.value.duration;
        deps.setAnchorLocal(targetTime);

        try {
            await invoke('player_seek', { time: targetTime });
        } catch (e) {
        } finally {
            if (actionSession === playActionSession) {
                deps.isSeeking.value = false;
                if (wasPlaying) {
                    await executePlayLogic(actionSession, false);
                }
            }
        }
    };

    const setOutputDevice = async (device: string): Promise<'SUCCESS' | 'THROTTLED' | 'FAILED'> => {
        if (deps.isSystemBusy.value || deps.isEngineSwitching.value) return 'FAILED';
        if (Date.now() - deps.lastMixerActionTime.value < 1000) return 'THROTTLED';

        deps.lastMixerActionTime.value = Date.now();
        deps.isSystemBusy.value = true;
        deps.isBuffering.value = true;
        deps.notifyUI.value?.(`Hot-swapping: ${device}...`, 'info');

        try {
            if (!deps.hasStarted.value) {
                deps.activeDevice.value = device;
                return 'SUCCESS';
            }
            await invoke('set_output_device', { device });
            deps.activeDevice.value = device;
            deps.hasAudioInitialized.value = true;

            if (deps.currentTrack.value) {
                await deps.refreshAnchor();
            }
            deps.notifyUI.value?.('Output Swapped');
            return 'SUCCESS';
        } catch (e) {
            deps.notifyUI.value?.('Migration Failed', 'error');
            return 'FAILED';
        } finally {
            deps.isSystemBusy.value = false;
            deps.isBuffering.value = false;
        }
    };

    const switchEngine = async (engineId: string): Promise<'SUCCESS' | 'DOWNLOADING' | 'FAILED' | 'COOLING'> => {
        if (deps.isSystemBusy.value || deps.isDownloadingFFmpeg.value || deps.isEngineSwitching.value || deps.isSeeking.value || deps.isBuffering.value || deps.isDragging.value) {
            deps.notifyUI.value?.('System busy', 'error');
            return 'FAILED';
        }

        const now = Date.now();
        if (now - deps.lastEngineSwitchTime.value < 30000) {
            const remaining = Math.ceil(30 - (now - deps.lastEngineSwitchTime.value) / 1000);
            deps.notifyUI.value?.(`Cooling: ${remaining}s`, 'cooling');
            return 'COOLING';
        }

        const previousEngine = deps.activeEngine.value;
        if (previousEngine === engineId) return 'SUCCESS';

        if (engineId === 'ffmpeg') {
            try {
                const exists = await invoke<boolean>('check_ffmpeg_exists');
                if (!exists) {
                    deps.isDownloadingFFmpeg.value = true;
                    deps.notifyUI.value?.('Fetching engine...', 'info');
                    invoke('start_ffmpeg_download').catch(() => {});
                    return 'DOWNLOADING';
                }
            } catch (e) {
                console.error("FFmpeg check failed:", e);
            }
        }

        deps.isSystemBusy.value = true;
        deps.isBuffering.value = true;
        deps.isEngineSwitching.value = true;
        deps.notifyUI.value?.(`Initializing ${engineId}...`);

        try {
            const savedTime = deps.currentTime.value;
            const wasPlaying = deps.isPlaying.value;
            const session = ++playActionSession;

            if (wasPlaying) {
                await executePauseLogic(session, true);
                await new Promise(r => setTimeout(r, 500));
            }

            const res = await invoke<string>('init_audio_engine', { engineId });

            if (res === "DOWNLOADING") {
                deps.isDownloadingFFmpeg.value = true;
                deps.activeEngine.value = previousEngine;
                if (wasPlaying) await executePlayLogic(session, false);
                return 'DOWNLOADING';
            }

            if (res.includes("READY") || res === "SUCCESS") {
                deps.hasAudioInitialized.value = true;
                deps.activeEngine.value = engineId;

                if (deps.currentTrack.value) {
                    const realTarget = deps.volume.value / 100.0;
                    deps.setBackendVolume(realTarget);
                    await invoke('player_load_track', { path: deps.currentTrack.value.path });
                    await invoke('player_seek', { time: savedTime });

                    await deps.refreshAnchor();

                    if (wasPlaying) await executePlayLogic(session, false);
                    else await invoke('player_pause');
                }

                deps.isEngineSwitching.value = false;
                startEngineCoolingTimer();
                return 'SUCCESS';
            }
            throw new Error("Invalid response");
        } catch (e: any) {
            deps.notifyUI.value?.(`Switch error`, 'error');
            await syncEngine();
            deps.isEngineSwitching.value = false;
            return 'FAILED';
        } finally {
            deps.isSystemBusy.value = false;
            deps.isBuffering.value = false;
        }
    };

    const setChannelMode = async (mode: number): Promise<'SUCCESS' | 'THROTTLED' | 'FAILED'> => {
        if (deps.isSystemBusy.value || deps.isEngineSwitching.value) return 'FAILED';
        if (mode === 2) deps.isTrueSurround.value = false;
        else if (Date.now() - deps.lastMixerActionTime.value < 1000) return 'THROTTLED';

        deps.lastMixerActionTime.value = Date.now();
        if (deps.channelMode.value === mode) return 'SUCCESS';

        deps.isSystemBusy.value = true;
        deps.isBuffering.value = true;

        try {
            deps.channelMode.value = mode;
            localStorage.setItem('channel_mode', mode.toString());
            localStorage.setItem('true_surround', JSON.stringify(deps.isTrueSurround.value));

            const finalMode = (deps.isTrueSurround.value && mode > 2) ? mode + 100 : mode;
            if (!deps.hasStarted.value) {
                invoke('player_set_channels', { mode: finalMode }).catch(() => {});
                return 'SUCCESS';
            }
            await invoke('player_set_channels', { mode: finalMode });

            if (deps.currentTrack.value && !deps.isTrackSwitching.value && !deps.isSeeking.value) {
                await invoke('player_seek', { time: deps.currentTime.value });
            }
            return 'SUCCESS';
        } catch (e) {
            return 'FAILED';
        } finally {
            deps.isSystemBusy.value = false;
            deps.isBuffering.value = false;
        }
    };

    const toggleTrueSurround = async (): Promise<'SUCCESS' | 'THROTTLED' | 'FAILED'> => {
        if (deps.channelMode.value === 2) return 'FAILED';
        if (deps.isSystemBusy.value || deps.isEngineSwitching.value) return 'FAILED';
        if (Date.now() - deps.lastMixerActionTime.value < 1000) return 'THROTTLED';

        deps.lastMixerActionTime.value = Date.now();
        deps.isSystemBusy.value = true;
        deps.isBuffering.value = true;

        try {
            deps.isTrueSurround.value = !deps.isTrueSurround.value;
            localStorage.setItem('true_surround', JSON.stringify(deps.isTrueSurround.value));

            const finalMode = (deps.isTrueSurround.value && deps.channelMode.value > 2) ? deps.channelMode.value + 100 : deps.channelMode.value;
            if (!deps.hasStarted.value) {
                invoke('player_set_channels', { mode: finalMode }).catch(() => {});
                return 'SUCCESS';
            }
            await invoke('player_set_channels', { mode: finalMode });

            if (deps.currentTrack.value && !deps.isTrackSwitching.value && !deps.isSeeking.value) {
                await invoke('player_seek', { time: deps.currentTime.value });
            }
            return 'SUCCESS';
        } catch (e) {
            return 'FAILED';
        } finally {
            deps.isSystemBusy.value = false;
            deps.isBuffering.value = false;
        }
    };

    const setupPlaybackListeners = async () => {
        await listen<number>('seek-end', (e) => {
            if (deps.isSystemBusy.value || deps.isSeeking.value || deps.isDragging.value || deps.isBuffering.value) return;
            if (Math.abs(deps.currentTime.value - e.payload) > 1.0) {
                deps.setAnchorLocal(e.payload);
            }
        });

        await listen('force-pause', () => {
            deps.isPlaying.value = false;
            deps.isPaused.value = true;
            deps.stopProgressLoop();
            deps.freezeAnchor();
        });
    };

    return {
        togglePlay,
        loadAndPlay,
        nextTrack,
        prevTrack,
        playTrack,
        seekTo,
        setOutputDevice,
        switchEngine,
        setChannelMode,
        toggleTrueSurround,
        executePlayLogic,
        executePauseLogic,
        syncEngine,
        startEngineCoolingTimer,
        setupPlaybackListeners,
        getPlayActionSession,
        incrementPlayActionSession,
    };
}