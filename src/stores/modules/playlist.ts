
import { ref, computed } from 'vue';
import { Track, PlayMode } from './types';

export function usePlaylist() {
    const queue = ref<Track[]>([]);
    const currentIndex = ref(0);
    const playMode = ref<PlayMode>('sequence');
    const showPlaylist = ref(false);
    
    const likedTracks = ref<Set<string>>(new Set(JSON.parse(localStorage.getItem('liked_tracks') || '[]')));

    const currentTrack = computed(() => {
        if (queue.value.length === 0 || currentIndex.value < 0 || currentIndex.value >= queue.value.length) return null;
        return queue.value[currentIndex.value];
    });

    const likedQueue = computed(() => queue.value.filter(t => likedTracks.value.has(t.id)));

    const toggleLike = (track: Track) => {
        if (likedTracks.value.has(track.id)) likedTracks.value.delete(track.id);
        else likedTracks.value.add(track.id);
        localStorage.setItem('liked_tracks', JSON.stringify(Array.from(likedTracks.value)));
    };

    const isLiked = (track: Track) => likedTracks.value.has(track.id);
    
    const togglePlaylist = () => { showPlaylist.value = !showPlaylist.value; };
    
    const toggleMode = () => {
        const modes: PlayMode[] = ['sequence', 'loop', 'shuffle'];
        playMode.value = modes[(modes.indexOf(playMode.value) + 1) % modes.length];
    };

    return { queue, currentIndex, playMode, showPlaylist, likedTracks, currentTrack, likedQueue, toggleLike, isLiked, togglePlaylist, toggleMode };
}