<script setup lang="ts">
import { computed } from 'vue';
import { usePlayerStore } from '../stores/player';
import { Loader2, ScanEye, AlertTriangle } from 'lucide-vue-next';

const props = defineProps<{
  notificationText: string,
  isNotificationVisible: boolean,
  isError: boolean
}>();

const player = usePlayerStore();

const currentIslandMode = computed(() => {
  if (props.isNotificationVisible) return props.isError ? 'error' : 'notification';
  if (player.isBuffering || player.isSeeking) return 'loading'; 
  if (player.isPlaying && player.hasStarted && player.currentTrack) return 'media';
  return 'idle';
});
</script>

<template>
  <div 
    class="fixed top-[16.5px] left-1/2 -translate-x-1/2 z-[100] min-h-[40px] bg-black/10 backdrop-blur-md rounded-2xl border border-white/5 shadow-[0_4px_30px_rgba(0,0,0,0.1)] overflow-hidden transition-all duration-500 cubic-bezier(0.175, 0.885, 0.32, 1.275) pointer-events-none grid grid-cols-1 grid-rows-1 items-center justify-items-center"
    :class="[
      currentIslandMode === 'idle' ? 'opacity-0 -translate-y-4 w-auto' : 'opacity-100 translate-y-0',
      currentIslandMode === 'media' ? 'w-auto min-w-[200px] max-w-[600px] px-4' : 'w-auto min-w-[200px] px-6',
      currentIslandMode === 'error' ? 'border-red-500/30' : ''
    ]"
  >
    <div class="absolute inset-0 bg-gradient-to-b from-white/[0.05] to-transparent z-0 w-full h-full"></div>
    
    <div class="col-start-1 row-start-1 transition-opacity duration-300 flex items-center gap-3 w-full justify-center" :class="currentIslandMode === 'loading' ? 'opacity-100 z-10' : 'opacity-0 z-0'">
      <Loader2 :size="16" class="text-starlight-cyan animate-spin" />
      <span class="text-[10px] font-mono font-bold tracking-[0.1em] text-white">PROCESSING</span>
    </div>

    <div class="col-start-1 row-start-1 transition-opacity duration-300 flex items-center gap-3 w-full justify-center" :class="currentIslandMode === 'notification' ? 'opacity-100 z-10' : 'opacity-0 z-0'">
      <ScanEye :size="16" class="text-starlight-cyan animate-pulse" />
      <span class="text-[10px] font-mono font-bold tracking-[0.1em] text-white">{{ notificationText }}</span>
    </div>

    <div class="col-start-1 row-start-1 transition-opacity duration-300 flex items-center gap-4 w-full justify-between" :class="currentIslandMode === 'media' ? 'opacity-100 z-10' : 'opacity-0 z-0'">
      <div class="w-8 h-8 rounded-full overflow-hidden border border-white/20 shrink-0">
          <img :src="player.currentTrack?.cover" class="w-full h-full object-cover animate-spin-slow" />
      </div>
      <div class="flex flex-col justify-center flex-1 min-w-0 py-1">
          <span class="text-xs font-bold text-white leading-tight truncate">{{ player.currentTrack?.title }}</span>
      </div>
      <div class="flex items-end gap-[2px] h-4 shrink-0">
          <div class="w-[2px] bg-starlight-cyan rounded-full animate-wave-1" :style="{ animationPlayState: player.isPaused ? 'paused' : 'running' }"></div>
          <div class="w-[2px] bg-starlight-purple rounded-full animate-wave-2" :style="{ animationPlayState: player.isPaused ? 'paused' : 'running' }"></div>
          <div class="w-[2px] bg-starlight-cyan rounded-full animate-wave-3" :style="{ animationPlayState: player.isPaused ? 'paused' : 'running' }"></div>
      </div>
    </div>
  </div>
</template>