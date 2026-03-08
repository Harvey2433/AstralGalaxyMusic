<script setup lang="ts">
import { computed, ref } from 'vue';
import { usePlayerStore } from '../stores/player';
import { Loader2, ScanEye, AlertTriangle } from 'lucide-vue-next';

const player = usePlayerStore();
const notificationText = ref('');
const isNotificationVisible = ref(false);
const isError = ref(false);
let notificationTimer: any = null;

const notify = (text: string, type: 'info' | 'error' = 'info') => {
  if (notificationTimer) clearTimeout(notificationTimer);
  notificationText.value = text; isError.value = type === 'error'; isNotificationVisible.value = true;
  // 错误提示留存 3 秒，普通提示 2 秒
  setTimeout(() => { isNotificationVisible.value = false; isError.value = false; }, type === 'error' ? 3000 : 2000);
};

defineExpose({ notify });

// 🔥 重构的多任务状态机：严格定义优先级
const currentIslandMode = computed(() => {
  // 1. 最高优先级：突发的文本通知 (错误/信息)
  if (isNotificationVisible.value) return isError.value ? 'error' : 'notification';
  // 2. 媒体流加载中
  if (player.isBuffering || player.isSeeking) return 'loading'; 
  // 3. 正在播放音乐 (边下边播时，优先展示音乐)
  if (player.isPlaying && player.hasStarted && player.currentTrack) return 'media';
  // 4. 空闲时的后台下载维持态
  if (player.isDownloadingFFmpeg) return 'downloading';
  // 5. 完全空闲，隐藏岛屿
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
      <div class="absolute inset-0 bg-gradient-to-b from-white/[0.05] to-transparent pointer-events-none z-0 col-start-1 row-start-1 w-full h-full"></div>
      
      <div class="col-start-1 row-start-1 transition-opacity duration-300 flex items-center gap-3 w-full justify-center" :class="currentIslandMode === 'loading' ? 'opacity-100 z-10' : 'opacity-0 z-0 pointer-events-none'"><Loader2 :size="16" class="text-starlight-cyan animate-spin shrink-0" /><span class="text-[10px] font-mono font-bold tracking-[0.1em] text-white whitespace-nowrap">PROCESSING</span></div>
      
      <div class="col-start-1 row-start-1 transition-opacity duration-300 flex items-center gap-3 w-full justify-center" :class="currentIslandMode === 'notification' ? 'opacity-100 z-10' : 'opacity-0 z-0 pointer-events-none'"><ScanEye :size="16" class="text-starlight-cyan animate-pulse shrink-0" /><span class="text-[10px] font-mono font-bold tracking-[0.1em] text-white whitespace-nowrap overflow-hidden text-ellipsis min-w-0">{{ notificationText }}</span></div>
      
      <div class="col-start-1 row-start-1 transition-opacity duration-300 flex items-center gap-3 w-full justify-center" :class="currentIslandMode === 'error' ? 'opacity-100 z-10' : 'opacity-0 z-0 pointer-events-none'"><AlertTriangle :size="16" class="text-red-500 animate-pulse shrink-0" /><span class="text-[10px] font-mono font-bold tracking-[0.1em] text-red-100 whitespace-nowrap">{{ notificationText }}</span></div>
      
      <div class="col-start-1 row-start-1 transition-opacity duration-300 flex items-center gap-3 w-full justify-center" :class="currentIslandMode === 'downloading' ? 'opacity-100 z-10' : 'opacity-0 z-0 pointer-events-none'">
          <Loader2 :size="16" class="text-yellow-400 animate-spin shrink-0" />
          <span class="text-[10px] font-mono font-bold tracking-[0.1em] text-yellow-400 whitespace-nowrap">FETCHING CORE...</span>
      </div>

      <div class="col-start-1 row-start-1 transition-opacity duration-300 flex items-center gap-4 w-full justify-between min-w-0" :class="currentIslandMode === 'media' ? 'opacity-100 z-10' : 'opacity-0 z-0 pointer-events-none'"><div class="w-8 h-8 rounded-full overflow-hidden border border-white/20 relative shrink-0"><img :src="player.currentTrack?.cover" class="w-full h-full object-cover animate-spin-slow" /></div><div class="flex flex-col justify-center flex-1 min-w-0 py-1 overflow-hidden"><span class="text-xs font-bold text-white leading-tight truncate text-left">{{ player.currentTrack?.title }}</span></div><div class="flex items-end gap-[2px] h-4 shrink-0 ml-auto"><div class="w-[2px] bg-starlight-cyan rounded-full animate-wave-1" :style="{ animationPlayState: player.isPaused ? 'paused' : 'running' }"></div><div class="w-[2px] bg-starlight-purple rounded-full animate-wave-2" :style="{ animationPlayState: player.isPaused ? 'paused' : 'running' }"></div><div class="w-[2px] bg-starlight-cyan rounded-full animate-wave-3" :style="{ animationPlayState: player.isPaused ? 'paused' : 'running' }"></div></div></div>
      
      <div 
        class="absolute bottom-0 left-0 h-[2px] bg-yellow-400 transition-all duration-300 shadow-[0_0_8px_rgba(250,204,21,0.8)] z-20 rounded-b-xl"
        :style="{ width: player.isDownloadingFFmpeg ? player.ffmpegProgress + '%' : '0%', opacity: player.isDownloadingFFmpeg ? 1 : 0 }"
      ></div>
    </div>
</template>

<style scoped>
@keyframes wave { 0%, 100% { height: 4px; } 50% { height: 12px; } }
.animate-wave-1 { animation: wave 0.8s infinite ease-in-out; }
.animate-wave-2 { animation: wave 1.1s infinite ease-in-out; }
.animate-wave-3 { animation: wave 0.9s infinite ease-in-out; }
</style>