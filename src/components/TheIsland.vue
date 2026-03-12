<script setup lang="ts">
import { computed, ref } from 'vue';
import { usePlayerStore } from '../stores/player';
import { Loader2, ScanEye, AlertTriangle, DownloadCloud, Snowflake } from 'lucide-vue-next';

const player = usePlayerStore();
const notificationText = ref('');
const notificationType = ref<'info' | 'error' | 'cooling'>('info');
const isNotificationVisible = ref(false);
let notificationTimer: any = null;

const notify = (text: string, type: 'info' | 'error' | 'cooling' = 'info') => {
  if (notificationTimer) clearTimeout(notificationTimer);
  notificationText.value = text; 
  notificationType.value = type; 
  isNotificationVisible.value = true;
  // 错误提示停留时间长一点，方便梦梦姐看清
  notificationTimer = setTimeout(() => { isNotificationVisible.value = false; }, type === 'error' ? 4000 : 2000);
};

defineExpose({ notify });

const currentIslandMode = computed(() => {
  if (isNotificationVisible.value) return 'notification';
  if (player.isImporting) return 'importing'; 
  if (player.isBuffering || player.isSeeking) return 'loading'; 
  if (player.isPlaying && player.hasStarted && player.currentTrack) return 'media';
  if (player.isDownloadingFFmpeg) return 'downloading';
  return 'idle';
});

// 核心魔法：动态计算灵动岛宽度
const islandWidth = computed(() => {
  if (currentIslandMode.value === 'idle') return '80px';
  if (currentIslandMode.value === 'media') return '260px';
  if (currentIslandMode.value === 'loading') return '110px';
  if (currentIslandMode.value === 'downloading') return '160px';
  // 给 iOS 的 Leading/Trailing 布局流出足够的呼吸空间
  if (currentIslandMode.value === 'importing') return '160px'; 
  if (currentIslandMode.value === 'notification') {
    const textWidth = notificationText.value.length * 8;
    return Math.max(220, textWidth + 80) + 'px';
  }
  return '220px';
});

// 核心魔法：动态计算灵动岛高度
const islandHeight = computed(() => {
  if (currentIslandMode.value === 'idle') return '20px';
  if (currentIslandMode.value === 'media') return '40px';
  if (currentIslandMode.value === 'loading') return '32px';
  return '36px'; // notification, downloading, importing 状态共用
});
</script>

<template>
  <div 
    class="fixed top-[32px] left-1/2 z-[100] flex items-center justify-center overflow-hidden bg-cosmos-950/70 backdrop-blur-xl border border-white/10 shadow-[0_10px_30px_rgba(0,0,0,0.5)] origin-center will-change-transform -translate-x-1/2 -translate-y-1/2"
    :class="currentIslandMode === 'idle' ? 'opacity-0 -mt-6 scale-90' : 'opacity-100 mt-0 scale-100'"
    :style="{
      width: islandWidth,
      height: islandHeight,
      transition: 'all 0.6s cubic-bezier(0.32, 0.72, 0, 1.25)',
      borderRadius: '999px'
    }"
  >
     <div 
       class="absolute inset-0 flex items-center gap-2.5 px-2.5 transition-all duration-500 ease-out"
       :class="currentIslandMode === 'media' ? 'opacity-100 scale-100 delay-100' : 'opacity-0 scale-95 pointer-events-none'"
     >
       <div class="w-7 h-7 rounded-full overflow-hidden shrink-0 shadow-md border border-white/10">
         <img :src="player.currentTrack?.cover" class="w-full h-full object-cover" :class="{ 'animate-spin-slow': player.isPlaying && !player.isPaused }" />
       </div>
       <div class="flex-1 flex flex-col justify-center min-w-0">
          <span class="text-[11px] font-semibold text-white truncate leading-tight">{{ player.currentTrack?.title }}</span>
          <span class="text-[9px] text-white/50 truncate leading-none mt-0.5">{{ player.currentTrack?.artist }}</span>
       </div>
       <div class="w-8 h-8 flex items-center justify-center gap-[3px] shrink-0">
         <template v-if="player.isDownloadingFFmpeg">
           <DownloadCloud :size="14" class="text-yellow-400 animate-pulse" />
         </template>
         <template v-else>
           <div class="w-[3px] bg-starlight-cyan rounded-full animate-wave-1" :style="{ animationPlayState: player.isPaused ? 'paused' : 'running' }"></div>
           <div class="w-[3px] bg-starlight-purple rounded-full animate-wave-2" :style="{ animationPlayState: player.isPaused ? 'paused' : 'running' }"></div>
           <div class="w-[3px] bg-starlight-cyan rounded-full animate-wave-3" :style="{ animationPlayState: player.isPaused ? 'paused' : 'running' }"></div>
         </template>
       </div>
     </div>

     <div 
       class="absolute inset-0 flex items-center justify-center px-4 gap-3 transition-all duration-500 ease-out"
       :class="currentIslandMode === 'notification' ? 'opacity-100 scale-100 delay-100' : 'opacity-0 scale-95 pointer-events-none'"
     >
       <component 
         :is="notificationType === 'error' ? AlertTriangle : (notificationType === 'cooling' ? Snowflake : ScanEye)" 
         :size="16" 
         :class="notificationType === 'error' ? 'text-red-500' : (notificationType === 'cooling' ? 'text-blue-400 animate-pulse' : 'text-starlight-cyan')" 
         class="shrink-0"
       />
       <span class="text-[10px] font-mono font-bold tracking-widest whitespace-nowrap" :class="notificationType === 'cooling' ? 'text-blue-300' : 'text-white'">
         {{ notificationText }}
       </span>
     </div>

     <div 
       class="absolute inset-0 flex items-center justify-center gap-2 transition-all duration-500 ease-out"
       :class="currentIslandMode === 'loading' ? 'opacity-100 scale-100 delay-100' : 'opacity-0 scale-95 pointer-events-none'"
     >
       <Loader2 :size="14" class="text-starlight-cyan animate-spin shrink-0" />
       <span class="text-[9px] font-mono font-bold tracking-widest text-white">Processing</span>
     </div>

     <div 
       class="absolute inset-0 flex items-center justify-center gap-3 px-4 transition-all duration-500 ease-out"
       :class="currentIslandMode === 'downloading' ? 'opacity-100 scale-100 delay-100' : 'opacity-0 scale-95 pointer-events-none'"
     >
       <DownloadCloud :size="16" class="text-yellow-400 animate-pulse shrink-0" />
       <span class="text-[10px] font-mono font-bold tracking-widest text-yellow-400 truncate min-w-0">Downloading...</span>
     </div>

     <div 
       class="absolute inset-0 flex items-center justify-between px-3.5 transition-all duration-500 ease-out"
       :class="currentIslandMode === 'importing' ? 'opacity-100 scale-100 delay-100' : 'opacity-0 scale-95 pointer-events-none'"
     >
       <div class="flex items-center gap-2 min-w-0">
           <div class="relative flex items-center justify-center shrink-0 w-[16px] h-[16px]">
              <svg class="w-full h-full -rotate-90" viewBox="0 0 24 24">
                <circle class="text-white/15" stroke-width="4" stroke="currentColor" fill="transparent" r="10" cx="12" cy="12" />
                <circle class="text-starlight-cyan transition-all duration-300 ease-out" 
                        style="filter: drop-shadow(0 0 2px rgba(100,255,218,0.5));" 
                        stroke-width="4" 
                        stroke-dasharray="62.83" 
                        :stroke-dashoffset="62.83 - (player.importProgress / 100) * 62.83" 
                        stroke-linecap="round" 
                        stroke="currentColor" 
                        fill="transparent" 
                        r="10" cx="12" cy="12" />
              </svg>
           </div>
           <span class="text-[11px] font-semibold text-white tracking-wide truncate mt-[1px]">Importing</span>
       </div>
       
       <div class="flex shrink-0 pl-2">
           <span class="text-[10px] font-mono font-medium text-starlight-cyan/80 tracking-tighter mt-[1px]">{{ player.importCount }} / {{ player.importTotal }}</span>
       </div>
     </div>

     <div 
       class="absolute bottom-0 left-0 h-[2px] bg-yellow-400 transition-all duration-300 shadow-[0_0_8px_rgba(250,204,21,0.8)] z-50 rounded-b-full pointer-events-none"
       :style="{ width: player.isDownloadingFFmpeg ? player.ffmpegProgress + '%' : '0%', opacity: player.isDownloadingFFmpeg ? 1 : 0 }"
     ></div>
  </div>
</template>

<style scoped>
@keyframes wave { 0%, 100% { height: 4px; } 50% { height: 14px; } }
.animate-wave-1 { animation: wave 0.8s infinite ease-in-out; }
.animate-wave-2 { animation: wave 1.1s infinite ease-in-out; }
.animate-wave-3 { animation: wave 0.9s infinite ease-in-out; }
.animate-spin-slow { animation: spin 8s linear infinite; }
</style>