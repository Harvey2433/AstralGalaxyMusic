<script setup lang="ts">
import { ref, watch, onMounted, nextTick } from 'vue';
import { usePlayerStore } from '../stores/player';
import { invoke } from '@tauri-apps/api/core';

const player = usePlayerStore();
const lyricsLines = ref<{ time: number; text: string }[]>([]);
const activeLineIndex = ref(-1);
const scrollContainer = ref<HTMLElement | null>(null);

// 解析 LRC 格式: [mm:ss.xx]歌词
const parseLrc = (lrc: string) => {
  const lines: { time: number; text: string }[] = [];
  const regex = /\[(\d{2}):(\d{2})\.(\d{2,3})\](.*)/;
  lrc.split('\n').forEach(line => {
    const match = line.match(regex);
    if (match) {
      const min = parseInt(match[1]);
      const sec = parseInt(match[2]);
      const msStr = match[3].padEnd(3, '0');
      const ms = parseInt(msStr);
      const time = min * 60 + sec + ms / 1000;
      const text = match[4].trim();
      if (text) lines.push({ time, text });
    }
  });
  return lines.sort((a, b) => a.time - b.time);
};

const loadLyrics = async () => {
  if (!player.currentTrack) return;
  try {
    const content = await invoke<string>('get_lyrics', { path: player.currentTrack.path });
    lyricsLines.value = content ? parseLrc(content) : [];
  } catch (e) {
    lyricsLines.value = [];
  }
};

// 监听歌曲切歌
watch(() => player.currentTrack?.path, loadLyrics, { immediate: true });

// 精准匹配当前行
watch(() => player.currentTime, (now) => {
  if (!lyricsLines.value.length) return;
  const index = lyricsLines.value.findIndex((line, i) => {
    const next = lyricsLines.value[i + 1];
    return now >= line.time && (!next || now < next.time);
  });
  if (index !== -1 && index !== activeLineIndex.value) {
    activeLineIndex.value = index;
    nextTick(scrollToActive);
  }
});

const scrollToActive = () => {
  const container = scrollContainer.value;
  const activeEl = container?.querySelector('.lyric-line.active');
  if (container && activeEl) {
    const targetY = (activeEl as HTMLElement).offsetTop - container.clientHeight / 2 + (activeEl as HTMLElement).clientHeight / 2;
    container.scrollTo({ top: targetY, behavior: 'smooth' });
  }
};
</script>

<template>
  <div class="lyrics-wrapper w-full h-full relative overflow-hidden flex items-center justify-center">
    <div v-if="lyricsLines.length === 0" class="text-white/20 font-orbitron tracking-widest animate-pulse">
      NO LYRICS FOUND
    </div>
    <div 
      v-else 
      ref="scrollContainer" 
      class="w-full h-full overflow-y-auto scrollbar-hide py-[50%] px-10 text-center"
    >
      <div 
        v-for="(line, index) in lyricsLines" 
        :key="index"
        class="lyric-line transition-all duration-500 py-3 cursor-pointer"
        :class="index === activeLineIndex ? 'active' : ''"
        @click="player.seekTo((line.time / player.currentTrack!.duration) * 100)"
      >
        <span class="text-xl md:text-2xl tracking-wide transition-all duration-500 block"
              :class="index === activeLineIndex 
                ? 'text-starlight-cyan scale-110 font-bold drop-shadow-[0_0_15px_rgba(100,255,218,0.6)]' 
                : 'text-white/30 scale-100 hover:text-white/60'">
          {{ line.text }}
        </span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.lyrics-wrapper {
  mask-image: linear-gradient(to bottom, transparent 0%, black 15%, black 85%, transparent 100%);
  -webkit-mask-image: linear-gradient(to bottom, transparent 0%, black 15%, black 85%, transparent 100%);
}
.scrollbar-hide::-webkit-scrollbar { display: none; }
</style>