import { ref } from 'vue';

export function useCredits(deps: {
    isPlaying: { value: boolean };
    isPaused: { value: boolean };
    executePauseLogic: (session: number, skipFade: boolean) => Promise<void>;
    executePlayLogic: (session: number, isNewTrack: boolean) => Promise<void>;
    incrementPlayActionSession: () => number;
}) {
    const showCredits = ref(false);
    let wasPlayingBeforeCredits = false;

    const startCredits = async () => {
        wasPlayingBeforeCredits = deps.isPlaying.value && !deps.isPaused.value;
        if (wasPlayingBeforeCredits) {
            const session = deps.incrementPlayActionSession();
            deps.isPlaying.value = false;
            deps.isPaused.value = true;
            await deps.executePauseLogic(session, false);
        }
        showCredits.value = true;
    };

    const endCredits = async () => {
        showCredits.value = false;
        if (wasPlayingBeforeCredits) {
            const session = deps.incrementPlayActionSession();
            deps.isPlaying.value = true;
            deps.isPaused.value = false;
            await deps.executePlayLogic(session, false);
        }
    };

    return {
        showCredits,
        startCredits,
        endCredits,
    };
}