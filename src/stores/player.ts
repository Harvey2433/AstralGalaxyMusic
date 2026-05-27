import { defineStore } from 'pinia';
import { ref, watch, onMounted, computed } from 'vue';
import { invoke } from '@tauri-apps/api/core';

import { Track, NotificationCallback } from './modules/types';
import { usePlaylist, preconvertCover, getOriginalCover } from './modules/playlist';
import { useEngine } from './modules/engine';
import { useVolume } from './modules/volume';
import { useProgress } from './modules/progress';
import { useImporter } from './modules/importer';
import { usePersistence } from './modules/persistence';
import { useFFmpegHandler } from './modules/ffmpeg-handler';
import { useCredits } from './modules/credits';
import { usePlayback } from './modules/playback';

export const usePlayerStore = defineStore('player', () => {
    const playlist = usePlaylist();
    const engine = useEngine();
    const vol = useVolume();

    const isPlaying = ref(false);
    const isPaused = ref(false);
    const hasStarted = ref(false);
    const progress = ref(0);
    const currentTime = ref(0);

    const isDragging = ref(false);
    const isBuffering = ref(false);
    const isSeeking = ref(false);
    const playSessionId = ref(0);
    const isTrackSwitching = ref(false);

    const isSystemBusy = ref(false);
    const needsInitialization = ref(false);

    const isImporting = ref(false);
    const importCount = ref(0);
    const importTotal = ref(0);
    const importProgress = ref(0);

    const likedQueue = ref<Track[]>([]);
    const likedIdSet = new Set<string>();

    const notifyUI = ref<NotificationCallback | null>(null);
    const setNotifier = (fn: NotificationCallback) => { notifyUI.value = fn; };

    const toggleLike = (track: Track) => {
        const idx = likedQueue.value.findIndex(t => t.id === track.id);
        if (idx === -1) {
            likedQueue.value.push(track);
            likedIdSet.add(track.id);
        } else {
            likedQueue.value.splice(idx, 1);
            likedIdSet.delete(track.id);
        }
    };
    const isLiked = (track: Track) => likedIdSet.has(track.id);

    const volumeGuard = () =>
        isSystemBusy.value || engine.isEngineSwitching.value || isBuffering.value || isSeeking.value;

    const setVolume = (v: number) => vol.setVolume(v, volumeGuard);
    const toggleMute = () => vol.toggleMute(volumeGuard);

    const progressCtrl = useProgress({
        isPlaying, isPaused, isDragging, isBuffering, isSeeking, isSystemBusy,
        currentTime, progress,
        currentTrack: computed(() => playlist.currentTrack.value as { duration: number } | null),
        playMode: computed(() => playlist.playMode.value),
        onTrackEnd: () => { playback.nextTrack(); },
    });

    const playback = usePlayback({
        isPlaying, isPaused, hasStarted, isSystemBusy, isBuffering, isSeeking,
        isDragging, isTrackSwitching, currentTime, progress, playSessionId,
        volume: vol.volume, notifyUI,
        queue: playlist.queue, currentIndex: playlist.currentIndex,
        currentTrack: computed(() => playlist.currentTrack.value),
        playMode: computed(() => playlist.playMode.value),
        activeEngine: engine.activeEngine,
        isEngineSwitching: engine.isEngineSwitching,
        isDownloadingFFmpeg: engine.isDownloadingFFmpeg,
        hasAudioInitialized: engine.hasAudioInitialized,
        activeDevice: engine.activeDevice,
        isSmtcEnabled: engine.isSmtcEnabled,
        lastEngineSwitchTime: engine.lastEngineSwitchTime,
        lastMixerActionTime: engine.lastMixerActionTime,
        engineCoolingRemaining: engine.engineCoolingRemaining,
        channelMode: engine.channelMode,
        isTrueSurround: engine.isTrueSurround,
        setBackendVolume: vol.setBackendVolume,
        smoothVolumeTransition: vol.smoothVolumeTransition,
        cancelFade: vol.cancelFade,
        startProgressLoop: progressCtrl.startProgressLoop,
        stopProgressLoop: progressCtrl.stopProgressLoop,
        refreshAnchor: progressCtrl.refreshAnchor,
        setAnchorLocal: progressCtrl.setAnchorLocal,
        freezeAnchor: progressCtrl.freezeAnchor,
    });

    const importer = useImporter({
        queue: playlist.queue,
        isImporting, importCount, importTotal, importProgress, notifyUI,
    });

    const persistence = usePersistence({
        volume: vol.volume, lastActiveVolume: vol.lastActiveVolume,
        activeEngine: engine.activeEngine, channelMode: engine.channelMode,
        isTrueSurround: engine.isTrueSurround, activeDevice: engine.activeDevice,
        likedQueue, likedIdSet, needsInitialization, notifyUI,
    });

    const ffmpegHandler = useFFmpegHandler({
        isDownloadingFFmpeg: engine.isDownloadingFFmpeg,
        ffmpegProgress: engine.ffmpegProgress,
        isEngineSwitching: engine.isEngineSwitching,
        hasAudioInitialized: engine.hasAudioInitialized,
        activeEngine: engine.activeEngine,
        isSystemBusy, isBuffering, isPlaying,
        currentTime, volume: vol.volume, notifyUI,
        currentTrackPath: () => playlist.currentTrack.value?.path || null,
        setBackendVolume: vol.setBackendVolume,
        executePauseLogic: playback.executePauseLogic,
        executePlayLogic: playback.executePlayLogic,
        getPlayActionSession: playback.getPlayActionSession,
        incrementPlayActionSession: playback.incrementPlayActionSession,
        startEngineCoolingTimer: playback.startEngineCoolingTimer,
    });

    const credits = useCredits({
        isPlaying, isPaused,
        executePauseLogic: playback.executePauseLogic,
        executePlayLogic: playback.executePlayLogic,
        incrementPlayActionSession: playback.incrementPlayActionSession,
    });

    watch(vol.volume, (v) => {
        if (volumeGuard()) return;
        const target = v / 100.0;
        if (isPlaying.value && !isPaused.value) {
            if (vol.isFading()) {
                vol.smoothVolumeTransition(target, 150);
            } else {
                vol.setBackendVolume(target);
            }
        }
    });

    onMounted(async () => {
        await playback.syncEngine();

        const settings = await persistence.restoreData();
        if (settings) {
            const initialVol = Math.pow(vol.volume.value / 100.0, 2);
            invoke('player_set_volume', { vol: initialVol }).catch(() => {});

            const finalMode = (engine.isTrueSurround.value && engine.channelMode.value > 2)
                ? engine.channelMode.value + 100
                : engine.channelMode.value;
            invoke('player_set_channels', { mode: finalMode }).catch(() => {});

            if (engine.activeEngine.value !== 'galaxy') {
                invoke('init_audio_engine', { engineId: engine.activeEngine.value }).catch(() => {});
            }
        }

        await ffmpegHandler.setupFFmpegListeners();
        await importer.setupImportListeners();
        await playback.setupPlaybackListeners();

        persistence.startPersistenceWatch();
        progressCtrl.startGlobalSyncTimer();
    });

    return {
        ...playlist,
        ...engine,
        isPlaying, isPaused, hasStarted,
        volume: vol.volume, lastActiveVolume: vol.lastActiveVolume,
        progress, currentTime,
        isDragging, isBuffering, isSeeking, isSystemBusy,
        playSessionId, isTrackSwitching,
        isImporting, importCount, importTotal, importProgress,
        needsInitialization,
        showCredits: credits.showCredits,
        likedQueue, toggleLike, isLiked,
        setNotifier, setVolume, toggleMute,
        togglePlay: playback.togglePlay,
        nextTrack: playback.nextTrack,
        prevTrack: playback.prevTrack,
        playTrack: playback.playTrack,
        seekTo: playback.seekTo,
        loadAndPlay: playback.loadAndPlay,
        switchEngine: playback.switchEngine,
        setOutputDevice: playback.setOutputDevice,
        setChannelMode: playback.setChannelMode,
        toggleTrueSurround: playback.toggleTrueSurround,
        initCheck: importer.initCheck,
        importTracks: importer.importTracks,
        startCredits: credits.startCredits,
        endCredits: credits.endCredits,
    };
});