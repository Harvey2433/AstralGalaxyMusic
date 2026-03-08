<script setup lang="ts">
import { usePlayerStore } from '../stores/player';
import { Heart, PlusCircle } from 'lucide-vue-next';

const player = usePlayerStore();
</script>

<template>
  <Transition name="drawer-spring">
    <div v-if="player.showPlaylist" class="absolute top-0 right-0 bottom-0 w-80 bg-cosmos-950/95 backdrop-blur-xl border-l border-white/10 z-40 flex flex-col shadow-[-10px_0_30px_rgba(0,0,0,0.5)]">
      
      <div class="p-5 border-b border-white/5 flex justify-between items-center bg-black/20">
          <h3 class="font-orbitron text-white text-sm tracking-widest flex items-center gap-2">
            PLAYLIST
          </h3>
          <button @click="player.togglePlaylist" class="text-white/50 hover:text-white hover:rotate-90 active:scale-75 transition-all duration-400 ease-[cubic-bezier(0.2,0.8,0.2,1)] no-outline">✕</button>
      </div>

      <div class="flex-1 overflow-y-auto scrollbar-hide p-3 space-y-1">
        <div 
          v-for="(track, index) in player.queue" 
          :key="track.id" 
          @dblclick="player.playTrack(track)" 
          class="flex items-center gap-3 p-3 rounded-lg cursor-pointer group transition-all duration-400 ease-[cubic-bezier(0.2,0.8,0.2,1)]"
          :class="player.currentIndex === index && player.hasStarted
            ? 'bg-starlight-cyan/10' 
            : 'hover:bg-white/5'"
        >
          <img :src="track.cover" class="w-8 h-8 rounded object-cover transition-opacity duration-300"
               :class="player.currentIndex === index && player.hasStarted ? 'opacity-100' : 'opacity-80 group-hover:opacity-100'" />
          
          <div class="flex-1 min-w-0">
            <div class="text-xs font-bold truncate transition-colors duration-300" 
                 :class="player.currentIndex === index && player.hasStarted ? 'text-starlight-cyan' : 'text-white'">
                 {{ track.title }}
            </div>
            <div class="text-[10px] truncate transition-colors duration-300 mt-0.5" 
                 :class="player.currentIndex === index && player.hasStarted ? 'text-starlight-cyan/70' : 'text-white/40 group-hover:text-white/60'">
                 {{ track.artist }}
            </div>
          </div>
          
          <button @click.stop="player.toggleLike(track)" class="transition-all duration-400 ease-[cubic-bezier(0.2,0.8,0.2,1)] text-white/50 hover:text-red-500 hover:scale-125 active:scale-75 mr-2"
                  :class="player.currentIndex === index && player.hasStarted ? 'opacity-100' : 'opacity-0 group-hover:opacity-100'">
            <Heart :size="14" :class="{ 'fill-red-500 text-red-500 opacity-100': player.isLiked(track) }" />
          </button>
          
          <div v-if="player.currentIndex === index && player.hasStarted" class="flex items-end gap-[2px] h-3 ml-1">
            <div class="w-[2px] bg-starlight-cyan rounded-full animate-wave-1" :style="{ animationPlayState: player.isPlaying ? 'running' : 'paused' }"></div>
            <div class="w-[2px] bg-starlight-cyan rounded-full animate-wave-2" :style="{ animationPlayState: player.isPlaying ? 'running' : 'paused' }"></div>
            <div class="w-[2px] bg-starlight-cyan rounded-full animate-wave-3" :style="{ animationPlayState: player.isPlaying ? 'running' : 'paused' }"></div>
          </div>
        </div>
      </div>

      <div class="p-4 border-t border-white/10">
        <button @click="player.importTracks" class="w-full py-3 bg-white/5 hover:bg-starlight-cyan/20 border border-white/10 hover:border-starlight-cyan/50 text-white rounded-lg flex items-center justify-center gap-2 transition-all duration-400 ease-[cubic-bezier(0.2,0.8,0.2,1)] active:scale-95 group no-drag-btn no-outline">
          <PlusCircle :size="16" class="group-hover:text-starlight-cyan transition-colors duration-300"/>
          <span class="text-xs font-bold tracking-widest group-hover:text-starlight-cyan transition-colors duration-300">ADD LOCAL FILES</span>
        </button>
      </div>

    </div>
  </Transition>
</template>

<style scoped>
/* 鸿蒙阻尼侧滑动画 */
.drawer-spring-enter-active, .drawer-spring-leave-active { 
  transition: transform 0.6s cubic-bezier(0.2, 0.8, 0.2, 1), opacity 0.4s ease; 
}
.drawer-spring-enter-from, .drawer-spring-leave-to { 
  transform: translateX(100%); 
  opacity: 0;
}

/* 隐藏滚动条 */
.scrollbar-hide::-webkit-scrollbar { display: none; }
.scrollbar-hide { -ms-overflow-style: none; scrollbar-width: none; }

/* 优雅版局部 EQ 动画 */
@keyframes local-wave { 
  0%, 100% { height: 3px; opacity: 0.6; } 
  50% { height: 10px; opacity: 1; } 
}
.animate-wave-1 { animation: local-wave 0.8s infinite ease-in-out; }
.animate-wave-2 { animation: local-wave 1.1s infinite ease-in-out; }
.animate-wave-3 { animation: local-wave 0.9s infinite ease-in-out; }
</style>