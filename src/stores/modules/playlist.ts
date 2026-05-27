import { ref, computed } from 'vue';
import { Track, PlayMode } from './types';

// ---------------------------------------------------------------
// Cover blob URL cache (non-reactive, global singleton)
// Converts base64 data URI to blob URL at import time so that
// scrolling the playlist never triggers synchronous base64 decode.
// The original base64 is kept in a parallel map for SMTC/persistence.
// ---------------------------------------------------------------
const blobCache = new Map<string, string>();
const originalCoverMap = new Map<string, string>();

// Convert base64 to blob URL for display. Store original for backend use.
export function preconvertCover(rawCover: string): string {
    if (!rawCover || rawCover === 'DEFAULT_COVER') return rawCover;
    if (!rawCover.startsWith('data:')) return rawCover;

    const cacheKey = rawCover.length + ':' + rawCover.substring(5, 53);

    const cached = blobCache.get(cacheKey);
    if (cached) return cached;

    try {
        const commaIdx = rawCover.indexOf(',');
        if (commaIdx === -1) return rawCover;

        const mimeMatch = rawCover.substring(0, commaIdx).match(/data:([^;]+)/);
        const mime = mimeMatch ? mimeMatch[1] : 'image/jpeg';
        const b64 = rawCover.substring(commaIdx + 1);

        const binary = atob(b64);
        const bytes = new Uint8Array(binary.length);
        for (let i = 0; i < binary.length; i++) {
            bytes[i] = binary.charCodeAt(i);
        }

        const url = URL.createObjectURL(new Blob([bytes], { type: mime }));
        blobCache.set(cacheKey, url);
        // Keep the original base64 so SMTC and persistence can use it
        originalCoverMap.set(url, rawCover);
        return url;
    } catch {
        return rawCover;
    }
}

// Retrieve original base64/URL for backend use (SMTC, persistence)
export function getOriginalCover(blobUrl: string): string {
    if (!blobUrl) return '';
    return originalCoverMap.get(blobUrl) || blobUrl;
}

export function usePlaylist() {
    const queue = ref<Track[]>([]);
    const currentIndex = ref(0);
    const playMode = ref<PlayMode>('sequence');
    const showPlaylist = ref(false);

    const currentTrack = computed(() => {
        if (queue.value.length === 0 || currentIndex.value < 0 || currentIndex.value >= queue.value.length) return null;
        return queue.value[currentIndex.value];
    });

    const togglePlaylist = () => { showPlaylist.value = !showPlaylist.value; };

    const toggleMode = () => {
        const modes: PlayMode[] = ['sequence', 'loop', 'shuffle'];
        playMode.value = modes[(modes.indexOf(playMode.value) + 1) % modes.length];
    };

    return {
        queue,
        currentIndex,
        playMode,
        showPlaylist,
        currentTrack,
        togglePlaylist,
        toggleMode
    };
}