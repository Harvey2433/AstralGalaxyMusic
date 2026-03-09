<script setup lang="ts">
import { ref, computed, onUnmounted } from 'vue';
import { usePlayerStore } from '../stores/player';
import { 
  Play, Pause, SkipForward, SkipBack, ListMusic, 
  Shuffle, Repeat, Repeat1, Volume1, VolumeX, Volume2, 
  ChevronUp, ChevronDown 
} from 'lucide-vue-next';

defineProps<{ showLyrics: boolean }>();
const emit = defineEmits(['toggle-lyrics']);

const player = usePlayerStore();
const volumeBarRef = ref<HTMLElement | null>(null);
const isDraggingVol = ref(false);
const localProgress = ref(0);

const VolumeIcon = computed(() => { 
  if(player.volume === 0) return VolumeX; 
  if(player.volume < 50) return Volume1; 
  return Volume2; 
});

// --- 🛠️ 核心 Bug 修复：解决“白色圆球”跟不上/偏移的问题 ---

const handleVolumeUpdate = (e: PointerEvent) => {
  if (!volumeBarRef.value) return;
  const rect = volumeBarRef.value.getBoundingClientRect();
  
  // 1. 基础百分比计算
  let percent = ((e.clientX - rect.left) / rect.width) * 100;
  
  // 2. 边界限制：确保圆球不会滑出轨道
  percent = Math.max(0, Math.min(100, percent));
  
  player.volume = percent;
};

const onPointerMove = (e: PointerEvent) => {
  if (isDraggingVol.value) {
    handleVolumeUpdate(e);
  }
};

const onPointerUp = (e: PointerEvent) => {
  if (isDraggingVol.value && volumeBarRef.value) {
    isDraggingVol.value = false;
    // 释放捕获
    try {
      volumeBarRef.value.releasePointerCapture(e.pointerId);
    } catch (err) {}
    
    volumeBarRef.value.removeEventListener('pointermove', onPointerMove);
    volumeBarRef.value.removeEventListener('pointerup', onPointerUp);
  }
};

const startVolumeDrag = (e: PointerEvent) => { 
  if (!volumeBarRef.value) return;
  
  // 阻止默认行为，防止白色滑块变成“文本选择”状态
  e.preventDefault();
  
  isDraggingVol.value = true;
  handleVolumeUpdate(e);
  
  // 🔥 指针捕获：这是让圆球不掉线的关键
  try {
    volumeBarRef.value.setPointerCapture(e.pointerId);
  } catch (err) {}

  volumeBarRef.value.addEventListener('pointermove', onPointerMove, { passive: true });
  volumeBarRef.value.addEventListener('pointerup', onPointerUp, { once: true });
};

onUnmounted(() => {
  if (volumeBarRef.value) {
    volumeBarRef.value.removeEventListener('pointermove', onPointerMove);
    volumeBarRef.value.removeEventListener('pointerup', onPointerUp);
  }
});

// 进度条逻辑（原生 input 已天然处理滑块偏移）
const onProgressInput = (e: Event) => { 
    if (!player.isDragging) {
        player.isDragging = true;
        window.addEventListener('pointerup', onProgressRelease, { once: true });
    }
    localProgress.value = parseFloat((e.target as HTMLInputElement).value); 
};

const onProgressRelease = () => {
    if (player.isDragging) {
        player.seekTo(localProgress.value); 
        setTimeout(() => { player.isDragging = false; }, 150); 
    }
};

const toggleMute = () => { player.volume = player.volume > 0 ? 0 : 50; };
</script>

<template>
  <div class="h-28 px-8 pb-4 bg-gradient-to-t from-cosmos-950 via-cosmos-900/90 to-transparent flex flex-col justify-end relative z-40">
    
    <div class="w-full h-6 mb-4 flex items-center cursor-default group relative no-drag-btn hover:scale-[1.005] transition-transform duration-300 ease-[cubic-bezier(0.2,0.8,0.2,1)]">
       <div class="absolute w-full h-1 bg-white/10 rounded-full overflow-hidden pointer-events-none">
          <div class="absolute h-full top-0 left-0 bg-gradient-to-r from-starlight-purple to-starlight-cyan transition duration-100"
               :class="player.isBuffering ? 'animate-pulse opacity-50' : ''"
               :style="{ width: (player.isDragging ? localProgress : player.progress) + '%' }">
          </div>
       </div>
       <input type="range" min="0" max="100" step="0.1" 
              :value="player.isDragging ? localProgress : player.progress" 
              @input="onProgressInput" 
              class="w-full h-6 opacity-0 cursor-pointer z-10" />
       <div class="absolute h-3 w-3 bg-white rounded-full shadow-[0_0_10px_white] pointer-events-none transition duration-200" 
            :class="player.isDragging ? 'opacity-100 scale-150' : 'opacity-0 group-hover:opacity-100'" 
            :style="{ left: `calc(${(player.isDragging ? localProgress : player.progress)}% - 6px)` }">
       </div>
    </div>

    <div class="flex items-center justify-between">
      <div class="w-1/3 flex items-center" :class="{ 'opacity-0 scale-95': !player.hasStarted && !player.currentTrack }">
          <div class="flex items-center gap-4 group cursor-pointer w-fit max-w-full active:scale-95 transition-all duration-400 ease-[cubic-bezier(0.2,0.8,0.2,1)]" @click="emit('toggle-lyrics')">
              <div class="relative shrink-0 w-12 h-12 rounded border border-white/10 overflow-hidden bg-cosmos-900 shadow-lg group-hover:shadow-[0_0_15px_rgba(100,255,218,0.3)] transition-all duration-500">
                  <img :src="player.currentTrack?.cover" class="w-full h-full object-cover transition-transform duration-500 group-hover:scale-110" />
                  <div class="absolute inset-0 bg-black/60 opacity-0 group-hover:opacity-100 flex items-center justify-center transition-opacity duration-300 backdrop-blur-[2px]">
                     <component :is="showLyrics ? ChevronDown : ChevronUp" :size="20" class="text-starlight-cyan animate-bounce" />
                  </div>
              </div>
              <div class="text-sm overflow-hidden flex-1 min-w-0">
                  <div class="text-white font-bold max-w-[150px] truncate group-hover:text-starlight-cyan transition-colors duration-300">{{ player.currentTrack?.title || 'No Track' }}</div>
                  <div class="text-xs text-white/40 group-hover:text-white/60 truncate transition-colors duration-300">{{ player.currentTrack?.artist || 'Unknown' }}</div>
              </div>
          </div>
      </div>
      
      <div class="flex items-center gap-6">
        <button class="text-white/40 hover:text-white transition-all duration-400 ease-[cubic-bezier(0.2,0.8,0.2,1)] hover:scale-110 active:scale-75 no-drag-btn no-outline" @click="player.toggleMode">
          <Shuffle v-if="player.playMode === 'shuffle'" :size="20" class="text-starlight-cyan"/>
          <Repeat1 v-else-if="player.playMode === 'loop'" :size="20" class="text-starlight-cyan"/>
          <Repeat v-else :size="20"/>
        </button>
        <button class="text-white hover:text-starlight-cyan transition-all duration-400 ease-[cubic-bezier(0.2,0.8,0.2,1)] hover:scale-110 active:scale-75 no-drag-btn no-outline" @click="player.prevTrack"><SkipBack :size="28" fill="currentColor"/></button>
        <button @click="player.togglePlay" class="w-14 h-14 rounded-full bg-white text-cosmos-950 flex items-center justify-center transition-all duration-500 ease-[cubic-bezier(0.2,0.8,0.2,1)] hover:scale-110 hover:shadow-[0_0_20px_rgba(255,255,255,0.4)] active:scale-90 no-drag-btn no-outline">
            <Pause v-if="player.isPlaying && !player.isPaused" fill="currentColor"/><Play v-else fill="currentColor" class="ml-1"/>
        </button>
        <button class="text-white hover:text-starlight-cyan transition-all duration-400 ease-[cubic-bezier(0.2,0.8,0.2,1)] hover:scale-110 active:scale-75 no-drag-btn no-outline" @click="player.nextTrack"><SkipForward :size="28" fill="currentColor"/></button>
        <button class="transition-all duration-400 ease-[cubic-bezier(0.2,0.8,0.2,1)] hover:scale-110 active:scale-75 no-drag-btn no-outline" :class="player.showPlaylist ? 'text-starlight-cyan scale-110' : 'text-white/40 hover:text-white'" @click="player.togglePlaylist"><ListMusic :size="20"/></button>
      </div>

      <div class="flex items-center justify-end gap-3 w-1/3 group select-none">
        <button @click="toggleMute" class="outline-none no-drag-btn no-outline transition-all duration-300 active:scale-75">
          <component :is="VolumeIcon" :size="20" class="text-white/60 hover:text-starlight-cyan transition-colors cursor-pointer"/>
        </button>
        
        <div 
          ref="volumeBarRef" 
          class="relative w-24 h-4 flex items-center cursor-pointer no-drag-btn hover:scale-105 transition-transform duration-400 ease-[cubic-bezier(0.2,0.8,0.2,1)]" 
          style="touch-action: none;" 
          @pointerdown="startVolumeDrag"
        >
          <div class="w-full h-1 bg-white/10 rounded-full overflow-hidden pointer-events-none">
            <div class="h-full bg-starlight-cyan transition-none" :style="{ width: player.volume + '%' }"></div>
          </div>
          
          <div 
            class="absolute h-3 w-3 bg-white rounded-full shadow-[0_0_12px_white] pointer-events-none" 
            :class="[isDraggingVol ? 'scale-150 opacity-100' : 'opacity-0 group-hover:opacity-100 transition-all duration-300']" 
            :style="{ 
              left: `calc(${player.volume}% - 6px)`,
              transition: isDraggingVol ? 'none' : 'all 0.3s cubic-bezier(0.2, 0.8, 0.2, 1)' 
            }"
          ></div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.no-drag-btn { -webkit-app-region: no-drag; }
/* 强制关闭 transition 避免拖拽时的“粘滞感” */
.transition-none { transition: none !important; }
</style>