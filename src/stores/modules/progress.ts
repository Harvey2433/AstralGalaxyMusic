import { Ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';

export function useProgress(deps: {
    isPlaying: Ref<boolean>;
    isPaused: Ref<boolean>;
    isDragging: Ref<boolean>;
    isBuffering: Ref<boolean>;
    isSeeking: Ref<boolean>;
    isSystemBusy: Ref<boolean>;
    currentTime: Ref<number>;
    progress: Ref<number>;
    currentTrack: Ref<{ duration: number } | null>;
    playMode: Ref<string>;
    onTrackEnd: () => void;
}) {
    let rafId: number | null = null;
    let syncTimerId: any = null;

    // Anchor-based time model: backend atomic clock is authoritative.
    // Frontend extrapolates from (anchorBackendTime, anchorLocalTs) each frame.
    let anchorBackendTime = 0;
    let anchorLocalTs = 0;
    let isAnchored = false;

    const refreshAnchor = async () => {
        try {
            const backendTime = await invoke<number>('get_current_time');
            anchorBackendTime = backendTime;
            anchorLocalTs = performance.now();
            isAnchored = true;
            deps.currentTime.value = backendTime;
            if (deps.currentTrack.value && deps.currentTrack.value.duration > 0) {
                deps.progress.value = (backendTime / deps.currentTrack.value.duration) * 100;
            }
        } catch (_) {}
    };

    const setAnchorLocal = (time: number) => {
        anchorBackendTime = time;
        anchorLocalTs = performance.now();
        isAnchored = true;
        deps.currentTime.value = time;
        if (deps.currentTrack.value && deps.currentTrack.value.duration > 0) {
            deps.progress.value = (time / deps.currentTrack.value.duration) * 100;
        }
    };

    const freezeAnchor = () => {
        if (isAnchored) {
            const elapsed = (performance.now() - anchorLocalTs) / 1000;
            anchorBackendTime = anchorBackendTime + elapsed;
            anchorLocalTs = performance.now();
        }
    };

    const startProgressLoop = () => {
        stopProgressLoop();

        const loop = () => {
            if (!deps.isPlaying.value || deps.isPaused.value) {
                rafId = null;
                return;
            }

            if (!deps.isDragging.value && !deps.isBuffering.value && !deps.isSeeking.value && !deps.isSystemBusy.value && deps.currentTrack.value && isAnchored) {
                const elapsed = (performance.now() - anchorLocalTs) / 1000;
                const extrapolated = anchorBackendTime + elapsed;

                if (extrapolated >= deps.currentTrack.value.duration) {
                    if (deps.playMode.value === 'loop') {
                        setAnchorLocal(0);
                        invoke('player_seek', { time: 0.0 }).catch(() => {});
                    } else {
                        deps.onTrackEnd();
                        rafId = null;
                        return;
                    }
                } else {
                    deps.currentTime.value = extrapolated;
                    if (deps.currentTrack.value.duration > 0) {
                        deps.progress.value = (extrapolated / deps.currentTrack.value.duration) * 100;
                    }
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

    const startGlobalSyncTimer = () => {
        if (syncTimerId) clearInterval(syncTimerId);
        syncTimerId = setInterval(async () => {
            if (deps.isPlaying.value && !deps.isPaused.value && !deps.isSystemBusy.value && !deps.isDragging.value && !deps.isSeeking.value && !deps.isBuffering.value) {
                try {
                    const backendTime = await invoke<number>('get_current_time');
                    const localNow = performance.now();
                    const extrapolated = isAnchored
                        ? anchorBackendTime + (localNow - anchorLocalTs) / 1000
                        : deps.currentTime.value;
                    const drift = Math.abs(extrapolated - backendTime);

                    if (drift > 0.05) {
                        anchorBackendTime = backendTime;
                        anchorLocalTs = localNow;
                        isAnchored = true;
                        deps.currentTime.value = backendTime;
                        if (deps.currentTrack.value && deps.currentTrack.value.duration > 0) {
                            deps.progress.value = (backendTime / deps.currentTrack.value.duration) * 100;
                        }
                    }
                } catch (_) {}
            }
        }, 1000);
    };

    return {
        startProgressLoop,
        stopProgressLoop,
        startGlobalSyncTimer,
        refreshAnchor,
        setAnchorLocal,
        freezeAnchor,
    };
}