import { ref, computed } from 'vue';
import { Track, PlayMode } from './types';

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