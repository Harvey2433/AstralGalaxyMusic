import { ref, Ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';

export function useVolume() {
    const volume = ref(80);
    const lastActiveVolume = ref(Number(localStorage.getItem('last_active_vol') || '80'));

    let fadeRafId: number | null = null;
    let backendVolRafId: number | null = null;
    let actualVolume = 0.0;
    let lastIpcTime = 0;

    const setBackendVolume = (v: number) => {
        actualVolume = Math.max(0, Math.min(1, v));
        if (backendVolRafId === null) {
            backendVolRafId = requestAnimationFrame(() => {
                const logVol = Math.pow(actualVolume, 2);
                invoke('player_set_volume', { vol: logVol }).catch(() => {});
                lastIpcTime = performance.now();
                backendVolRafId = null;
            });
        }
    };

    const smoothVolumeTransition = (targetVol: number, duration: number, onComplete?: () => void) => {
        if (fadeRafId !== null) {
            cancelAnimationFrame(fadeRafId);
            fadeRafId = null;
        }
        const startVol = actualVolume;
        const diff = targetVol - startVol;
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
            actualVolume = startVol + diff * fadeProgress;
            if (timestamp - lastIpcTime > 33 || fadeProgress >= 1) {
                const logVol = Math.pow(actualVolume, 2);
                invoke('player_set_volume', { vol: logVol }).catch(() => {});
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

    const cancelFade = () => {
        if (fadeRafId !== null) {
            cancelAnimationFrame(fadeRafId);
            fadeRafId = null;
        }
    };

    const isFading = () => fadeRafId !== null;

    let volSaveTimeout: any = null;
    const saveActiveVolume = (v: number) => {
        if (volSaveTimeout) clearTimeout(volSaveTimeout);
        volSaveTimeout = setTimeout(() => {
            localStorage.setItem('last_active_vol', v.toString());
        }, 500);
    };

    // setVolume and toggleMute need guard flags from outside, so they accept a guard callback
    const setVolume = (v: number, isGuarded: () => boolean) => {
        if (isGuarded()) return;
        volume.value = v;
        if (v > 0) {
            lastActiveVolume.value = v;
            saveActiveVolume(v);
        }
    };

    const toggleMute = (isGuarded: () => boolean) => {
        if (isGuarded()) return;
        if (volume.value > 0) {
            lastActiveVolume.value = volume.value;
            saveActiveVolume(volume.value);
            volume.value = 0;
        } else {
            volume.value = lastActiveVolume.value;
        }
    };

    return {
        volume,
        lastActiveVolume,
        setBackendVolume,
        smoothVolumeTransition,
        cancelFade,
        isFading,
        setVolume,
        toggleMute,
    };
}