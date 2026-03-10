<script setup lang="ts">
import { ref, watch, nextTick, onMounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { usePlayerStore } from '../stores/player';
import { Radio } from 'lucide-vue-next';

const props = defineProps<{ showLyrics: boolean }>();
const player = usePlayerStore();
const DEFAULT_COVER = 'https://images.unsplash.com/photo-1614728853913-6591d801d643?q=80&w=400&auto=format&fit=crop';

// --- 歌词逻辑 ---
const lyricsLines = ref<{ time: number; text: string; duration: number }[]>([]);
const activeLineIndex = ref(-1);
const lineProgress = ref(0);
const scrollOffset = ref(0);
const lyricsWrapperRef = ref<HTMLElement | null>(null);

const parseLrc = (lrc: string) => {
  const lines: { time: number; text: string; duration: number }[] = [];
  const regex = /\[(\d{2}):(\d{2})\.(\d{2,3})\](.*)/;
  const rawLines = lrc.split('\n');
  const tempLines: { time: number; text: string }[] = [];

  rawLines.forEach(line => {
    const match = line.match(regex);
    if (match) {
      const min = parseInt(match[1]);
      const sec = parseInt(match[2]);
      const ms = parseInt(match[3].padEnd(3, '0'));
      const time = min * 60 + sec + ms / 1000;
      const text = match[4].trim();
      if (text) tempLines.push({ time, text });
    }
  });
  tempLines.sort((a, b) => a.time - b.time);
  for (let i = 0; i < tempLines.length; i++) {
    const current = tempLines[i];
    const next = tempLines[i + 1];
    const duration = next ? (next.time - current.time) : 5.0; 
    lines.push({ ...current, duration });
  }
  return lines;
};

const loadLyrics = async () => {
  lyricsLines.value = []; activeLineIndex.value = -1; lineProgress.value = 0; scrollOffset.value = 0;
  if (!player.currentTrack) return;
  try {
    const lrcContent = await invoke<string>('get_lyrics', { path: player.currentTrack.path });
    if (lrcContent) lyricsLines.value = parseLrc(lrcContent);
  } catch (e) { console.error(e); }
};

onMounted(() => {
  if (player.currentTrack) {
    loadLyrics();
  }
});

watch(() => player.currentTrack?.id, loadLyrics);

watch(() => player.currentTime, (time) => {
  if (!props.showLyrics || lyricsLines.value.length === 0) return;
  let currentIdx = lyricsLines.value.findIndex(line => line.time > time);
  if (currentIdx === -1) currentIdx = lyricsLines.value.length - 1;
  else if (currentIdx > 0) currentIdx = currentIdx - 1;

  if (currentIdx !== activeLineIndex.value) {
    activeLineIndex.value = currentIdx;
    updateScrollPosition();
  }
  if (currentIdx !== -1) {
    const currentLine = lyricsLines.value[currentIdx];
    const timeInLine = time - currentLine.time;
    lineProgress.value = Math.max(0, Math.min(100, (timeInLine / currentLine.duration) * 100));
  }
});

const updateScrollPosition = () => {
  requestAnimationFrame(() => {
    if (!lyricsWrapperRef.value || !lyricsWrapperRef.value.children.length) return;
    const activeEl = lyricsWrapperRef.value.children[activeLineIndex.value] as HTMLElement;
    if (activeEl) {
      const elementCenter = activeEl.offsetTop + activeEl.offsetHeight / 2;
      scrollOffset.value = -elementCenter;
    }
  });
};

watch(() => props.showLyrics, (newVal) => {
  if (newVal) {
     nextTick(() => { setTimeout(() => updateScrollPosition(), 50); });
  }
});
</script>

<template>
  <div class="absolute inset-0 z-20 transition-all duration-500 w-full h-full">
    <div v-if="!player.hasStarted || !player.currentTrack" class="flex flex-col items-center justify-center h-full gap-6 animate-fade-in">
        <div class="relative w-48 h-48 flex items-center justify-center">
            <div class="absolute inset-0 rounded-full border-[1px] border-starlight-purple/20 animate-spin-slow-reverse"></div>
            <div class="absolute inset-4 rounded-full border-[1px] border-starlight-cyan/20 border-t-transparent border-l-transparent animate-spin-slow"></div>
            <div class="absolute inset-8 rounded-full border-[1px] border-starlight-purple/30 animate-pulse-slow"></div>
            <Radio :size="24" class="text-starlight-cyan/50 absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2"/>
        </div>
        <div class="text-center space-y-2">
          <h1 class="text-3xl font-bold font-orbitron tracking-wider text-white drop-shadow-lg">No track selected</h1>
          <p class="text-sm text-cosmos-300 font-mono tracking-[0.3em] opacity-70">Idle</p>
        </div>
    </div>

    <div v-else class="w-full h-full relative">
      <Transition name="fade">
          <div v-if="!showLyrics" class="absolute inset-0 flex flex-col items-center justify-center animate-fade-in">
              <div class="relative group">
                  <div class="absolute inset-0 rounded-full border border-starlight-cyan/30 scale-110 opacity-0 group-hover:scale-125 group-hover:opacity-100 transition-all duration-700"></div>
                  <div class="absolute inset-0 rounded-full border border-starlight-purple/30 scale-105 animate-pulse"></div>
                  <div class="w-64 h-64 rounded-full border-4 border-cosmos-800 shadow-[0_0_50px_rgba(0,0,0,0.5)] overflow-hidden animate-spin-slow" :style="{ animationPlayState: player.isPlaying && !player.isBuffering && !player.isPaused ? 'running' : 'paused' }">
                      <img :src="player.currentTrack?.cover || DEFAULT_COVER" class="w-full h-full object-cover opacity-90 select-none" />
                      <div class="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-16 h-16 bg-cosmos-950 rounded-full border border-white/10 flex items-center justify-center">
                          <div class="w-2 h-2 bg-starlight-cyan rounded-full" :class="{ 'animate-ping': player.isPlaying && !player.isBuffering && !player.isPaused }"></div>
                      </div>
                  </div>
              </div>
              <div class="text-center space-y-2 z-10 mt-12 pointer-events-none">
                  <h1 class="text-4xl font-bold font-orbitron tracking-wider text-transparent bg-clip-text bg-gradient-to-r from-white via-starlight-cyan to-white drop-shadow-lg">{{ player.currentTrack?.title || 'Unknown Track' }}</h1>
                  <p class="text-lg text-cosmos-300 font-light tracking-widest uppercase">{{ player.currentTrack?.artist || 'Unknown Artist' }}</p>
              </div>
          </div>
      </Transition>

      <Transition name="slide-up">
          <div v-if="showLyrics" class="absolute inset-0 z-30 bg-black/60 backdrop-blur-xl mask-gradient flex flex-col items-center justify-center">
               <div v-if="lyricsLines.length === 0" class="text-white/20 font-orbitron tracking-widest text-sm animate-pulse">
                  No lyrics found
               </div>
               <div v-else class="w-full h-full relative overflow-hidden"> 
                   <div ref="lyricsWrapperRef" class="absolute left-0 w-full flex flex-col items-center gap-6 transition-transform duration-700 cubic-bezier(0.25, 0.46, 0.45, 0.94)" :style="{ transform: `translateY(${scrollOffset}px)`, top: '50%' }">
                       <div v-for="(line, index) in lyricsLines" :key="index" class="lyric-line px-10 py-2 select-none text-center cursor-pointer transition-all duration-500" :class="index === activeLineIndex ? 'active' : (Math.abs(index - activeLineIndex) <= 1 ? 'near' : 'far')" @click.stop="player.seekTo((line.time / player.currentTrack!.duration) * 100)">
                          <span class="kugou-text relative block font-bold font-sans tracking-wider leading-relaxed" :data-text="line.text" :style="index === activeLineIndex ? { '--prog': lineProgress + '%' } : {}">{{ line.text }}</span>
                       </div>
                   </div>
               </div>
          </div>
      </Transition>
    </div>
  </div>
</template>

<style scoped>
.animate-fade-in { animation: fadeIn 0.5s ease-out; }
@keyframes fadeIn { from { opacity: 0; transform: scale(0.95); } to { opacity: 1; transform: scale(1); } }

.fade-enter-active { transition: opacity 0.3s ease-out; }
.fade-leave-active { transition: opacity 0.2s ease-in; }
.fade-enter-from, .fade-leave-to { opacity: 0; }

.slide-up-enter-active, .slide-up-leave-active { transition: all 0.4s cubic-bezier(0.2, 0.8, 0.2, 1); }
.slide-up-enter-from, .slide-up-leave-to { opacity: 0; transform: translateY(20px); filter: blur(5px); }

.mask-gradient {
  mask-image: linear-gradient(to bottom, transparent 0%, black 15%, black 85%, transparent 100%);
  -webkit-mask-image: linear-gradient(to bottom, transparent 0%, black 15%, black 85%, transparent 100%);
}

.kugou-text { color: rgba(255, 255, 255, 0.55); position: relative; z-index: 1; }
.lyric-line.active .kugou-text { background-image: linear-gradient(to right, #ffffff var(--prog), transparent var(--prog)); -webkit-background-clip: text; background-clip: text; color: transparent; }
.lyric-line.active .kugou-text::after { content: attr(data-text); position: absolute; left: 0; top: 0; z-index: -1; color: rgba(255, 255, 255, 0.55); }
.lyric-line.active { transform: scale(1.15); filter: drop-shadow(0 0 12px rgba(100, 255, 218, 0.4)); opacity: 1; }
.lyric-line.near { transform: scale(0.95); opacity: 0.8; }
.lyric-line.far { transform: scale(0.85); opacity: 0.4; }
</style>