import { Ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { NotificationCallback } from './types';

export function useFFmpegHandler(deps: {
    isDownloadingFFmpeg: Ref<boolean>;
    ffmpegProgress: Ref<number>;
    isEngineSwitching: Ref<boolean>;
    hasAudioInitialized: Ref<boolean>;
    activeEngine: Ref<string>;
    isSystemBusy: Ref<boolean>;
    isBuffering: Ref<boolean>;
    isPlaying: Ref<boolean>;
    currentTime: Ref<number>;
    volume: Ref<number>;
    notifyUI: Ref<NotificationCallback | null>;
    currentTrackPath: () => string | null;
    setBackendVolume: (v: number) => void;
    executePauseLogic: (session: number, skipFade: boolean) => Promise<void>;
    executePlayLogic: (session: number, isNewTrack: boolean) => Promise<void>;
    getPlayActionSession: () => number;
    incrementPlayActionSession: () => number;
    startEngineCoolingTimer: () => void;
}) {
    const setupFFmpegListeners = async () => {
        await listen('ffmpeg-status', async (e: any) => {
            const status = e.payload;
            if (status === 'downloading') {
                deps.isDownloadingFFmpeg.value = true;
                deps.ffmpegProgress.value = 0;
                deps.notifyUI.value?.('Fetching engine...', 'info');
            } else if (status === 'extracting') {
                deps.isDownloadingFFmpeg.value = true;
                deps.ffmpegProgress.value = 99;
                deps.notifyUI.value?.('Extracting core...', 'info');
            } else if (status === 'ready') {
                deps.ffmpegProgress.value = 100;
                deps.notifyUI.value?.('Core deployed');

                const savedTime = deps.currentTime.value;
                const wasPlaying = deps.isPlaying.value;
                const session = deps.incrementPlayActionSession();

                deps.isSystemBusy.value = true;
                deps.isBuffering.value = true;
                deps.isEngineSwitching.value = true;

                if (wasPlaying) {
                    await deps.executePauseLogic(session, true);
                    await new Promise(r => setTimeout(r, 500));
                }

                try {
                    const res = await invoke<string>('init_audio_engine', { engineId: 'ffmpeg' });
                    if (res.includes("READY")) {
                        deps.hasAudioInitialized.value = true;
                        deps.activeEngine.value = 'ffmpeg';
                        const trackPath = deps.currentTrackPath();
                        if (trackPath) {
                            const realTarget = deps.volume.value / 100.0;
                            deps.setBackendVolume(realTarget);
                            await invoke('player_load_track', { path: trackPath });
                            await invoke('player_seek', { time: savedTime });
                            if (wasPlaying) await deps.executePlayLogic(session, false);
                            else await invoke('player_pause');
                        }
                        deps.startEngineCoolingTimer();
                    }
                } catch (err) {
                    deps.notifyUI.value?.('FFmpeg failed', 'error');
                } finally {
                    deps.isDownloadingFFmpeg.value = false;
                    deps.isEngineSwitching.value = false;
                    deps.isSystemBusy.value = false;
                    deps.isBuffering.value = false;
                }
            } else if (status === 'cooling') {
                deps.isDownloadingFFmpeg.value = false;
                deps.isEngineSwitching.value = false;
                deps.startEngineCoolingTimer();
                deps.notifyUI.value?.('System cooling...', 'cooling');
            } else if (status === 'error') {
                deps.isDownloadingFFmpeg.value = false;
                deps.isEngineSwitching.value = false;
                deps.notifyUI.value?.('Download error', 'error');
            }
        });

        await listen('ffmpeg-progress', (e: any) => {
            deps.ffmpegProgress.value = e.payload as number;
        });
    };

    return {
        setupFFmpegListeners,
    };
}