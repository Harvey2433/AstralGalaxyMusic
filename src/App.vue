<script setup lang="ts">
import { ref, onMounted, computed, watch, nextTick } from 'vue';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { invoke } from '@tauri-apps/api/core';
import { usePlayerStore } from './stores/player'; 
import { 
  Play, Pause, SkipForward, SkipBack, ListMusic, Disc3, Settings, 
  Heart, Shuffle, Repeat, Volume1, VolumeX, Volume2,
  Cpu, Zap, HardDrive, Film, CheckCircle2, Terminal, Loader2, AlertCircle,
  Monitor, Sliders, LogOut, LayoutDashboard, ScanEye, Repeat1, AlertTriangle, PlusCircle, AudioLines, Speaker,
  Activity, Radio, ChevronUp, ChevronDown 
} from 'lucide-vue-next';

const player = usePlayerStore();

// --- 常量定义 (修复 DEFAULT_COVER 缺失报错) ---
const DEFAULT_COVER = 'https://images.unsplash.com/photo-1614728853913-6591d801d643?q=80&w=400&auto=format&fit=crop';

// --- 歌词逻辑 (酷狗级逐字渲染引擎) ---
const showLyrics = ref(false); 
// 增加 duration 字段，用于计算单句进度
const lyricsLines = ref<{ time: number; text: string; duration: number }[]>([]);
const activeLineIndex = ref(-1);
const lineProgress = ref(0); 
const scrollOffset = ref(0);
const lyricsWrapperRef = ref<HTMLElement | null>(null);

// 解析 LRC 并预计算每行持续时间
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

  // 智能计算 duration：当前行时间 到 下一行时间 的差值
  for (let i = 0; i < tempLines.length; i++) {
    const current = tempLines[i];
    const next = tempLines[i + 1];
    // 如果是最后一行，默认给 5秒，防止进度条卡死
    const duration = next ? (next.time - current.time) : 5.0; 
    lines.push({ ...current, duration });
  }

  return lines;
};

const loadLyrics = async () => {
  lyricsLines.value = [];
  activeLineIndex.value = -1;
  lineProgress.value = 0;
  scrollOffset.value = 0; 
  if (!player.currentTrack) return;
  
  try {
    const lrcContent = await invoke<string>('get_lyrics', { path: player.currentTrack.path });
    if (lrcContent) {
      lyricsLines.value = parseLrc(lrcContent);
    }
  } catch (e) { console.error(e); }
};

watch(() => player.currentTrack?.id, loadLyrics);

// 核心：高频渲染循环 (驱动卡拉OK动画)
watch(() => player.currentTime, (time) => {
  if (!showLyrics.value || lyricsLines.value.length === 0) return;

  // 1. 查找当前行
  let currentIdx = lyricsLines.value.findIndex(line => line.time > time);
  if (currentIdx === -1) currentIdx = lyricsLines.value.length - 1;
  else if (currentIdx > 0) currentIdx = currentIdx - 1;

  // 2. 更新滚动位置 (防抖)
  if (currentIdx !== activeLineIndex.value) {
    activeLineIndex.value = currentIdx;
    updateScrollPosition();
  }

  // 3. 计算酷狗级填充进度
  if (currentIdx !== -1) {
    const currentLine = lyricsLines.value[currentIdx];
    const timeInLine = time - currentLine.time;
    // 限制在 0% - 100%
    lineProgress.value = Math.max(0, Math.min(100, (timeInLine / currentLine.duration) * 100));
  }
});

// 计算物理滚动偏移量 (居中算法)
const updateScrollPosition = () => {
  if (!lyricsWrapperRef.value || !lyricsWrapperRef.value.children.length) return;
  const activeEl = lyricsWrapperRef.value.children[activeLineIndex.value] as HTMLElement;
  if (activeEl) {
    // 歌词容器视口高度的一半
    const containerHeight = lyricsWrapperRef.value.parentElement?.clientHeight || 600;
    // 计算偏移：让当前行中心 对齐 视口中心
    scrollOffset.value = -(activeEl.offsetTop - containerHeight / 2 + activeEl.clientHeight / 2);
  }
};

const toggleLyricsView = (state?: boolean) => {
  showLyrics.value = state !== undefined ? state : !showLyrics.value;
  if (showLyrics.value) {
     nextTick(() => {
        // 进入时强制重算一次位置
        updateScrollPosition();
     });
  }
};

// --- 灵动岛逻辑 ---
type IslandMode = 'idle' | 'notification' | 'media' | 'error' | 'loading'; 
const notificationText = ref('');
const isNotificationVisible = ref(false);
const isError = ref(false); 
let notificationTimer: any = null;

const notify = (text: string, type: 'info' | 'error' = 'info') => {
  if (notificationTimer) clearTimeout(notificationTimer);
  notificationText.value = text;
  isError.value = type === 'error';
  isNotificationVisible.value = true;
  const duration = type === 'error' ? 3000 : 2000;
  notificationTimer = setTimeout(() => { isNotificationVisible.value = false; isError.value = false; }, duration);
};

const currentIslandMode = computed<IslandMode>(() => {
  if (isNotificationVisible.value) return isError.value ? 'error' : 'notification';
  if (player.isBuffering || player.isSeeking) return 'loading'; 
  if (player.isPlaying && player.hasStarted && player.currentTrack) return 'media';
  return 'idle';
});

// --- 声道与设备 ---
const currentChannel = ref(2);
const setChannel = (ch: number) => {
  currentChannel.value = ch;
  player.setChannelMode(ch);
  notify(`AUDIO OUTPUT: ${ch === 2 ? 'STEREO' : ch.toFixed(1) + ' SURROUND'}`);
};

const selectOutputDevice = (e: Event) => {
  const target = e.target as HTMLSelectElement;
  player.setOutputDevice(target.value);
};

// --- 窗口 ---
const appWindow = getCurrentWindow();
const minimize = () => appWindow.minimize();
const toggleMaximize = async () => { const isMax = await appWindow.isMaximized(); isMax ? appWindow.unmaximize() : appWindow.maximize(); };
const closeWindow = () => appWindow.close();

const activeTab = ref('dashboard'); 
const showSettings = ref(false); 
watch(activeTab, (n) => { if (n !== 'settings') notify(`${n.toUpperCase()} MODULE`); });
watch(showSettings, (v) => { if (v) notify('SYSTEM CONFIGURATION'); });
const switchTab = (t: string) => { activeTab.value = t; showSettings.value = t === 'settings'; };
const switchToMain = () => { showSettings.value = false; activeTab.value = 'dashboard'; };

// --- 引擎设置 ---
const activeSettingTab = ref('core');
const engineState = ref<'idle' | 'switching' | 'success' | 'failed'>('idle');
const targetEngineId = ref(''); 

const engines = [
  { id: 'galaxy', name: 'GalaxyCore', sub: 'HYPERION', icon: Cpu, color: 'text-starlight-cyan', border: 'border-starlight-cyan', glow: 'shadow-[0_0_15px_rgba(100,255,218,0.3)]', desc: 'Native Rust (Zero-Copy)' },
  { id: 'bass', name: 'BASS Audio', sub: 'AUDIOPHILE', icon: Zap, color: 'text-yellow-400', border: 'border-yellow-400', glow: 'shadow-[0_0_15px_rgba(250,204,21,0.3)]', desc: 'Audiophile Grade' },
  { id: 'mci', name: 'Windows MCI', sub: 'LEGACY', icon: HardDrive, color: 'text-blue-400', border: 'border-blue-400', glow: 'shadow-[0_0_15px_rgba(96,165,250,0.3)]', desc: 'Legacy System' },
  { id: 'ffmpeg', name: 'FFmpeg', sub: 'UNIVERSAL', icon: Film, color: 'text-purple-400', border: 'border-purple-400', glow: 'shadow-[0_0_15px_rgba(192,132,252,0.3)]', desc: 'Universal Format' }
];

const selectEngine = async (id: string) => {
  if (engineState.value === 'switching' || player.activeEngine === id) return;
  targetEngineId.value = id; 
  engineState.value = 'switching';
  notify(`INITIALIZING ${id.toUpperCase()}...`);
  const result = await player.switchEngine(id);
  if (result === true) {
    engineState.value = 'success';
    notify(`${id.toUpperCase()} ENGINE READY`);
    setTimeout(() => { engineState.value = 'idle'; targetEngineId.value = ''; }, 1500);
  } else {
    engineState.value = 'failed';
    notify(`FAILED TO LOAD ${id.toUpperCase()}`, 'error');
    setTimeout(() => { engineState.value = 'idle'; targetEngineId.value = ''; }, 2000);
  }
};

// --- 控制与交互 ---
const volumeBarRef = ref<HTMLElement | null>(null);
const isDraggingVol = ref(false);
const VolumeIcon = computed(() => { if(player.volume===0)return VolumeX; if(player.volume<50)return Volume1; return Volume2; });

const updateVolume = (e: MouseEvent) => { if(!volumeBarRef.value)return; const rect = volumeBarRef.value.getBoundingClientRect(); player.volume = Math.max(0, Math.min(100, ((e.clientX - rect.left) / rect.width) * 100)); };
const startVolumeDrag = (e: MouseEvent) => { isDraggingVol.value = true; updateVolume(e); window.addEventListener('mousemove', onVolumeDrag); window.addEventListener('mouseup', stopVolumeDrag); };
const onVolumeDrag = (e: MouseEvent) => { if(isDraggingVol.value) updateVolume(e); };
const stopVolumeDrag = () => { isDraggingVol.value = false; window.removeEventListener('mousemove', onVolumeDrag); window.removeEventListener('mouseup', stopVolumeDrag); };

const localProgress = ref(0);
const onProgressInput = (e: Event) => {
    const target = e.target as HTMLInputElement;
    player.isDragging = true; 
    localProgress.value = parseFloat(target.value);
};
const onProgressChange = (e: Event) => {
    const target = e.target as HTMLInputElement;
    const val = parseFloat(target.value);
    player.seekTo(val);
    setTimeout(() => { player.isDragging = false; }, 100);
};

const toggleMute = () => { player.volume = player.volume > 0 ? 0 : 50; };

onMounted(() => { 
  notify('ASTRAL_SYSTEM ONLINE'); player.setNotifier(notify); player.initCheck(); player.fetchDevices();
  document.oncontextmenu = (e) => { e.preventDefault(); return false; };
  document.onkeydown = (e) => { if(e.key === 'F12' || (e.ctrlKey && e.key === 'r')) { e.preventDefault(); } };
});
</script>

<template>
  <main class="relative flex w-screen h-screen overflow-hidden text-cosmos-100 bg-[#05080a] font-sans rounded-xl border border-white/10">
    
    <div 
      class="fixed top-[16.5px] left-1/2 -translate-x-1/2 z-[100] min-h-[40px] bg-black/10 backdrop-blur-md rounded-2xl border border-white/5 shadow-[0_4px_30px_rgba(0,0,0,0.1)] overflow-hidden transition-all duration-500 cubic-bezier(0.175, 0.885, 0.32, 1.275) pointer-events-none grid grid-cols-1 grid-rows-1 items-center justify-items-center"
      :class="[
        currentIslandMode === 'idle' ? 'opacity-0 -translate-y-4 w-auto' : 'opacity-100 translate-y-0',
        currentIslandMode === 'media' ? 'w-auto min-w-[200px] max-w-[600px] px-4' : 'w-auto min-w-[200px] px-6',
        currentIslandMode === 'error' ? 'border-red-500/30' : ''
      ]"
    >
      <div class="absolute inset-0 bg-gradient-to-b from-white/[0.05] to-transparent pointer-events-none z-0 col-start-1 row-start-1 w-full h-full"></div>
      
      <div class="col-start-1 row-start-1 transition-opacity duration-300 flex items-center gap-3 w-full justify-center" :class="currentIslandMode === 'loading' ? 'opacity-100 z-10' : 'opacity-0 z-0 pointer-events-none'">
        <Loader2 :size="16" class="text-starlight-cyan animate-spin shrink-0" />
        <span class="text-[10px] font-mono font-bold tracking-[0.1em] text-white whitespace-nowrap">PROCESSING</span>
      </div>

      <div class="col-start-1 row-start-1 transition-opacity duration-300 flex items-center gap-3 w-full justify-center" :class="currentIslandMode === 'notification' ? 'opacity-100 z-10' : 'opacity-0 z-0 pointer-events-none'">
        <ScanEye :size="16" class="text-starlight-cyan animate-pulse shrink-0" />
        <span class="text-[10px] font-mono font-bold tracking-[0.1em] text-white whitespace-nowrap overflow-hidden text-ellipsis min-w-0">{{ notificationText }}</span>
      </div>

      <div class="col-start-1 row-start-1 transition-opacity duration-300 flex items-center gap-3 w-full justify-center" :class="currentIslandMode === 'error' ? 'opacity-100 z-10' : 'opacity-0 z-0 pointer-events-none'">
        <AlertTriangle :size="16" class="text-red-500 animate-pulse shrink-0" />
        <span class="text-[10px] font-mono font-bold tracking-[0.1em] text-red-100 whitespace-nowrap">{{ notificationText }}</span>
      </div>

      <div class="col-start-1 row-start-1 transition-opacity duration-300 flex items-center gap-4 w-full justify-between min-w-0" :class="currentIslandMode === 'media' ? 'opacity-100 z-10' : 'opacity-0 z-0 pointer-events-none'">
        <div class="w-8 h-8 rounded-full overflow-hidden border border-white/20 relative shrink-0">
            <img :src="player.currentTrack?.cover" class="w-full h-full object-cover animate-spin-slow" />
        </div>
        <div class="flex flex-col justify-center flex-1 min-w-0 py-1 overflow-hidden">
            <span class="text-xs font-bold text-white leading-tight truncate text-left">{{ player.currentTrack?.title }}</span>
        </div>
        <div class="flex items-end gap-[2px] h-4 shrink-0 ml-auto">
            <div class="w-[2px] bg-starlight-cyan rounded-full animate-wave-1" :style="{ animationPlayState: player.isPaused ? 'paused' : 'running' }"></div>
            <div class="w-[2px] bg-starlight-purple rounded-full animate-wave-2" :style="{ animationPlayState: player.isPaused ? 'paused' : 'running' }"></div>
            <div class="w-[2px] bg-starlight-cyan rounded-full animate-wave-3" :style="{ animationPlayState: player.isPaused ? 'paused' : 'running' }"></div>
        </div>
      </div>
    </div>

    <div class="absolute top-[-15%] right-[-10%] w-[600px] h-[600px] rounded-full pointer-events-none z-0 animate-float-slow opacity-70"
         style="background: radial-gradient(circle at 30% 30%, rgba(189, 52, 254, 0.4) 0%, rgba(80, 20, 120, 0.1) 60%, transparent 100%); box-shadow: inset -20px -20px 50px rgba(0,0,0,0.5); filter: blur(40px);"></div>
    <div class="absolute bottom-[-20%] left-[-15%] w-[700px] h-[700px] rounded-full pointer-events-none z-0 animate-float-slower opacity-60"
         style="background: radial-gradient(circle at 70% 30%, rgba(100, 255, 218, 0.3) 0%, rgba(20, 120, 100, 0.05) 60%, transparent 100%); box-shadow: inset 20px 20px 50px rgba(0,0,0,0.5); filter: blur(50px);"></div>
    <div class="absolute inset-0 bg-[url('https://www.transparenttextures.com/patterns/stardust.png')] opacity-20 mix-blend-overlay pointer-events-none z-0"></div>

    <div class="relative z-10 flex w-full h-full backdrop-blur-[1px]">
      
      <aside class="flex flex-col w-20 h-full border-r border-white/5 bg-cosmos-950/40 backdrop-blur-md z-50" data-tauri-drag-region>
        <div class="flex items-center justify-center h-20 text-starlight-cyan pointer-events-none"><Disc3 :size="32" class="animate-spin-slow" /></div>
        <nav class="flex flex-col items-center gap-6 mt-10">
          <button @click="switchTab('dashboard')" class="group relative w-10 h-10 flex items-center justify-center rounded-xl transition-all duration-300 ease-out no-drag-btn no-outline" :class="activeTab === 'dashboard' ? 'bg-white/10 text-white shadow-[0_0_15px_rgba(255,255,255,0.1)] scale-105' : 'text-white/40 hover:text-white hover:bg-white/5'"><LayoutDashboard :size="20" /><div v-if="activeTab === 'dashboard'" class="absolute inset-0 bg-white/5 rounded-xl blur-sm"></div></button>
          <button @click="switchTab('likes')" class="group relative w-10 h-10 flex items-center justify-center rounded-xl transition-all duration-300 ease-out no-drag-btn no-outline" :class="activeTab === 'likes' ? 'bg-white/10 text-white shadow-[0_0_15px_rgba(255,255,255,0.1)] scale-105' : 'text-white/40 hover:text-white hover:bg-white/5'"><Heart :size="20" /></button>
          <button @click="switchTab('settings')" class="mt-auto mb-8 w-10 h-10 flex items-center justify-center rounded-xl transition-all duration-300 ease-out no-drag-btn no-outline" :class="activeTab === 'settings' ? 'bg-starlight-purple/20 text-starlight-purple shadow-[0_0_15px_rgba(189,52,254,0.4)] scale-105' : 'text-white/40 hover:text-white hover:bg-white/5'"><Settings :size="20" /></button>
        </nav>
      </aside>

      <section class="flex flex-col flex-1 relative z-20">
        <header class="h-16 flex items-center justify-between px-8 border-b border-white/5 bg-cosmos-900/20 cursor-move" data-tauri-drag-region>
          <div class="text-xs font-mono tracking-[0.3em] text-starlight-cyan/50 pointer-events-none opacity-50">/// ASTRAL_CORE_V1</div>
          <div class="flex gap-3">
            <button @click="closeWindow" class="w-3.5 h-3.5 rounded-full bg-red-500/20 border border-red-500/50 hover:bg-red-500 transition-all flex items-center justify-center group no-drag-btn no-outline"><span class="opacity-0 group-hover:opacity-100 text-[8px] text-white">✕</span></button>
            <button @click="minimize" class="w-3.5 h-3.5 rounded-full bg-yellow-500/20 border border-yellow-500/50 hover:bg-yellow-500 transition-all flex items-center justify-center group no-drag-btn no-outline"><span class="opacity-0 group-hover:opacity-100 text-[8px] text-white">−</span></button>
            <button @click="toggleMaximize" class="w-3.5 h-3.5 rounded-full bg-green-500/20 border border-green-500/50 hover:bg-green-500 transition-all flex items-center justify-center group no-drag-btn no-outline"><span class="opacity-0 group-hover:opacity-100 text-[6px] text-white">□</span></button>
          </div>
        </header>

        <div class="flex-1 relative overflow-hidden w-full">
          
          <div v-if="activeTab === 'likes'" class="absolute inset-0 z-20 flex flex-col p-10 overflow-y-auto scrollbar-hide">
             <h2 class="text-4xl font-bold font-orbitron text-white mb-8 flex items-center gap-4"><Heart :size="32" class="text-red-500 fill-red-500" /> LIKED TRACKS</h2>
             <div class="grid grid-cols-1 gap-2">
                <div v-for="track in player.likedQueue" :key="track.id" @dblclick="player.playTrack(track)" class="flex items-center gap-4 p-4 rounded-xl bg-white/5 hover:bg-white/10 transition-all group cursor-pointer">
                   <div class="relative w-12 h-12 rounded-lg overflow-hidden"><img :src="track.cover" class="w-full h-full object-cover" /><div class="absolute inset-0 bg-black/50 opacity-0 group-hover:opacity-100 flex items-center justify-center transition-all"><Play :size="20" class="text-white fill-white"/></div></div>
                   <div class="flex-1"><div class="text-white font-bold">{{ track.title }}</div><div class="text-white/40 text-xs">{{ track.artist }}</div></div>
                   <button @click.stop="player.toggleLike(track)" class="text-red-500 hover:scale-110 transition-transform"><Heart :size="20" class="fill-red-500"/></button>
                </div>
                <div v-if="player.likedQueue.length === 0" class="text-white/30 text-center mt-20">NO LIKED TRACKS YET</div>
             </div>
          </div>

          <div v-else-if="activeTab === 'dashboard'" class="absolute inset-0 z-20 transition-all duration-500" :class="showSettings ? 'opacity-0 scale-95 pointer-events-none blur-sm' : 'opacity-100 scale-100 blur-0'">
              
              <div v-if="!player.hasStarted || !player.currentTrack" class="flex flex-col items-center justify-center h-full gap-6 animate-fade-in">
                  <div class="relative w-48 h-48 flex items-center justify-center">
                      <div class="absolute inset-0 rounded-full border-[1px] border-starlight-purple/20 animate-spin-slow-reverse"></div>
                      <div class="absolute inset-4 rounded-full border-[1px] border-starlight-cyan/20 border-t-transparent border-l-transparent animate-spin-slow"></div>
                      <div class="absolute inset-8 rounded-full border-[1px] border-starlight-purple/30 animate-pulse-slow"></div>
                      <Radio :size="24" class="text-starlight-cyan/50 absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2"/>
                  </div>
                  <div class="text-center space-y-2">
                    <h1 class="text-3xl font-bold font-orbitron tracking-wider text-white drop-shadow-lg">No Track Selected</h1>
                    <p class="text-sm text-cosmos-300 font-mono tracking-[0.3em] uppercase opacity-70">IDLE</p>
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
                            NO LYRICS FOUND
                         </div>

                         <div v-else ref="lyricsContainerRef" class="w-full h-full relative overflow-hidden"> 
                             <div 
                                ref="lyricsWrapperRef"
                                class="absolute left-0 w-full flex flex-col items-center gap-6 transition-transform duration-700 cubic-bezier(0.25, 0.46, 0.45, 0.94)"
                                :style="{ transform: `translateY(${scrollOffset}px)`, top: '50%' }"
                             >
                                 <div 
                                    v-for="(line, index) in lyricsLines"
                                    :key="index"
                                    class="lyric-line px-10 py-2 select-none text-center cursor-pointer transition-all duration-500"
                                    :class="index === activeLineIndex ? 'active' : (Math.abs(index - activeLineIndex) <= 1 ? 'near' : 'far')"
                                    @click.stop="player.seekTo((line.time / player.currentTrack!.duration) * 100)"
                                 >
                                    <span 
                                      class="kugou-text relative block font-bold font-sans tracking-wider leading-relaxed"
                                      :data-text="line.text"
                                      :style="index === activeLineIndex ? { '--prog': lineProgress + '%' } : {}"
                                    >
                                      {{ line.text }}
                                    </span>
                                 </div>
                             </div>
                         </div>
                    </div>
                </Transition>
              </div>
          </div>

          <Transition name="slide-right">
            <div v-if="player.showPlaylist" class="absolute top-0 right-0 bottom-0 w-80 bg-cosmos-950/95 backdrop-blur-xl border-l border-white/10 z-40 flex flex-col shadow-2xl">
              <div class="p-4 border-b border-white/5 flex justify-between items-center bg-black/20">
                  <h3 class="font-orbitron text-white text-sm tracking-widest">PLAYLIST</h3>
                  <button @click="player.togglePlaylist" class="text-white/50 hover:text-white transition-colors no-outline">✕</button>
              </div>
              <div class="flex-1 overflow-y-auto scrollbar-hide p-2">
                <div v-for="(track, index) in player.queue" :key="track.id" @dblclick="player.playTrack(track)" class="flex items-center gap-3 p-3 rounded-lg cursor-pointer group border-b border-white/5 transition-all mb-1 hover:bg-white/5">
                  <img :src="track.cover" class="w-8 h-8 rounded object-cover opacity-80" />
                  <div class="flex-1 min-w-0"><div class="text-white font-bold text-xs truncate" :class="player.currentIndex === index ? 'text-starlight-cyan' : ''">{{ track.title }}</div><div class="text-white/40 text-[10px] truncate">{{ track.artist }}</div></div>
                  <button @click.stop="player.toggleLike(track)" class="opacity-0 group-hover:opacity-100 transition-opacity text-white/50 hover:text-red-500 mr-2"><Heart :size="14" :class="{ 'fill-red-500 text-red-500 opacity-100': player.isLiked(track) }" /></button>
                  <div v-if="player.currentIndex === index" class="text-starlight-cyan"><AudioLines :size="14" :class="player.isPlaying ? 'animate-pulse' : 'opacity-50'" /></div>
                </div>
              </div>
              <div class="p-4 border-t border-white/10"><button @click="player.importTracks" class="w-full py-3 bg-white/5 hover:bg-starlight-cyan/20 border border-white/10 hover:border-starlight-cyan/50 text-white rounded-lg flex items-center justify-center gap-2 transition-all group no-drag-btn no-outline"><PlusCircle :size="16" class="group-hover:text-starlight-cyan"/><span class="text-xs font-bold tracking-widest group-hover:text-starlight-cyan">ADD LOCAL FILES</span></button></div>
            </div>
          </Transition>

          <Transition name="fade">
            <div v-if="showSettings" class="absolute inset-0 z-30 flex bg-cosmos-950/95 backdrop-blur-xl">
              <div class="w-64 h-full bg-black/20 flex flex-col p-6 z-10 border-r border-white/5">
                <h2 class="text-xl font-orbitron font-bold text-white mb-8 flex items-center gap-2"><Settings :size="20" class="text-starlight-purple"/> SETTINGS</h2>
                <nav class="space-y-2 flex-1">
                  <button @click="activeSettingTab = 'core'" class="w-full flex items-center gap-3 p-3 rounded-lg transition-all text-sm font-bold tracking-wider no-drag-btn no-outline" :class="activeSettingTab === 'core' ? 'bg-starlight-cyan/10 text-starlight-cyan' : 'text-white/40 hover:text-white hover:bg-white/5'"><Terminal :size="18" /> CORE SYSTEM</button>
                  <button @click="activeSettingTab = 'audio'" class="w-full flex items-center gap-3 p-3 rounded-lg transition-all text-sm font-bold tracking-wider no-drag-btn no-outline" :class="activeSettingTab === 'audio' ? 'bg-starlight-cyan/10 text-starlight-cyan' : 'text-white/40 hover:text-white hover:bg-white/5'"><Sliders :size="18" /> AUDIO MIXER</button>
                  <button @click="activeSettingTab = 'display'" class="w-full flex items-center gap-3 p-3 rounded-lg transition-all text-sm font-bold tracking-wider no-drag-btn no-outline" :class="activeSettingTab === 'display' ? 'bg-starlight-cyan/10 text-starlight-cyan' : 'text-white/40 hover:text-white hover:bg-white/5'"><Monitor :size="18" /> HOLOGRAM UI</button>
                </nav>
                <button @click="switchToMain" class="flex items-center gap-2 text-xs text-white/30 hover:text-red-400 mt-auto pt-4 border-t border-white/5 transition-colors no-drag-btn no-outline"><LogOut :size="14" /> EXIT CONFIGURATION</button>
              </div>
              
              <div class="flex-1 h-full overflow-hidden p-10 relative z-10">
                <Transition name="slide-fade" mode="out-in">
                  <div v-if="activeSettingTab === 'core'" class="h-full overflow-y-auto scrollbar-hide max-w-4xl mx-auto">
                    <div class="mb-8 flex items-end justify-between">
                        <div>
                            <h3 class="text-2xl font-bold text-white mb-2">Decoding Engine</h3>
                            <p class="text-sm text-white/40">Select the audio core driver for signal processing.</p>
                        </div>
                        <div class="flex items-center gap-2 bg-black/40 p-2 px-3 rounded border border-white/5">
                            <Activity :size="14" class="text-starlight-cyan" />
                            <span class="text-xs font-mono text-starlight-cyan/80">LATENCY: NORMAL</span>
                        </div>
                    </div>

                    <div class="grid grid-cols-2 gap-4">
                      <div v-for="engine in engines" :key="engine.id" @click="selectEngine(engine.id)" 
                        class="relative p-5 rounded-xl border bg-cosmos-900/40 backdrop-blur-sm cursor-pointer transition-all duration-300 group hover:bg-white/5 no-drag-btn no-outline overflow-hidden" 
                        :class="[
                           (targetEngineId === engine.id && engineState === 'failed') ? 'border-red-500 bg-red-500/10' :
                           (targetEngineId === engine.id && engineState === 'switching') ? 'border-yellow-400 bg-yellow-400/10' :
                           (player.activeEngine === engine.id && engineState === 'idle') ? `bg-opacity-20 ${engine.border} ${engine.glow}` :
                           'border-white/5 hover:border-white/20',
                           (engineState === 'switching' && targetEngineId !== engine.id) ? 'opacity-50 grayscale' : 'opacity-100'
                        ]">
                        
                        <div v-if="player.activeEngine === engine.id && engineState === 'idle'" class="absolute top-0 right-0 p-3">
                            <div class="flex items-center gap-2">
                                <span class="text-[10px] font-bold tracking-widest" :class="engine.color">ACTIVE</span>
                                <div class="w-2 h-2 rounded-full animate-pulse" :class="engine.color.replace('text-', 'bg-')"></div>
                            </div>
                        </div>

                        <div v-if="targetEngineId === engine.id" class="absolute top-4 right-4">
                            <Loader2 v-if="engineState === 'switching'" :size="18" class="text-yellow-400 animate-spin" />
                            <AlertCircle v-else-if="engineState === 'failed'" :size="18" class="text-red-500" />
                            <CheckCircle2 v-else-if="engineState === 'success'" :size="18" class="text-starlight-cyan drop-shadow-[0_0_8px_cyan]" />
                        </div>

                        <div class="mb-3 p-2 rounded-lg w-fit transition-colors bg-black/60 relative z-10"><component :is="engine.icon" :size="24" :class="player.activeEngine === engine.id || targetEngineId === engine.id ? engine.color : 'text-white/30'" /></div>
                        <h4 class="text-base font-bold text-white mb-0.5 relative z-10">{{ engine.name }}</h4>
                        <p class="text-[10px] font-mono mb-2 uppercase opacity-80 relative z-10" :class="engine.color">{{ engine.sub }}</p>
                        <p class="text-xs text-white/40 leading-relaxed relative z-10">{{ engine.desc }}</p>
                      </div>
                    </div>
                  </div>
                  
                  <div v-else-if="activeSettingTab === 'audio'" class="h-full overflow-y-auto scrollbar-hide max-w-4xl mx-auto">
                      <h3 class="text-2xl font-bold text-white mb-2">Audio Channels</h3>
                      <p class="text-sm text-white/40 mb-8">Configure output mapping for surround sound systems.</p>
                      <div class="mb-8 p-4 bg-white/5 rounded-xl border border-white/5">
                          <label class="text-xs font-bold text-starlight-cyan tracking-widest mb-2 block">OUTPUT DEVICE</label>
                          <select @change="selectOutputDevice" class="w-full bg-black/40 border border-white/10 rounded p-2 text-white text-sm focus:border-starlight-cyan outline-none"><option v-for="dev in player.availableDevices" :key="dev" :value="dev" :selected="player.activeDevice === dev">{{ dev }}</option></select>
                      </div>
                      <div class="grid grid-cols-3 gap-4">
                          <button @click="setChannel(2)" class="p-6 rounded-xl border flex flex-col items-center gap-4 transition-all no-drag-btn outline-none" :class="currentChannel === 2 ? 'bg-starlight-cyan/10 border-starlight-cyan text-white' : 'bg-white/5 border-white/5 text-white/40 hover:bg-white/10'"><Speaker :size="32" /><span class="font-bold tracking-widest text-xs">STEREO (2.0)</span></button>
                          <button @click="setChannel(6)" class="p-6 rounded-xl border flex flex-col items-center gap-4 transition-all no-drag-btn outline-none" :class="currentChannel === 6 ? 'bg-starlight-cyan/10 border-starlight-cyan text-white' : 'bg-white/5 border-white/5 text-white/40 hover:bg-white/10'"><Speaker :size="32" /><span class="font-bold tracking-widest text-xs">SURROUND (5.1)</span></button>
                          <button @click="setChannel(8)" class="p-6 rounded-xl border flex flex-col items-center gap-4 transition-all no-drag-btn outline-none" :class="currentChannel === 8 ? 'bg-starlight-cyan/10 border-starlight-cyan text-white' : 'bg-white/5 border-white/5 text-white/40 hover:bg-white/10'"><Speaker :size="32" /><span class="font-bold tracking-widest text-xs">SURROUND (7.1)</span></button>
                      </div>
                  </div>
                   <div v-else-if="activeSettingTab === 'display'" class="h-full flex flex-col items-center justify-center opacity-30"><Monitor :size="48" class="mb-4 text-white"/><p class="text-white font-mono">HOLOGRAM UI CALIBRATION REQUIRED</p></div>
                </Transition>
              </div>
            </div>
          </Transition>
        </div>
        
        <div class="h-28 px-8 pb-4 bg-gradient-to-t from-cosmos-950 via-cosmos-900/90 to-transparent flex flex-col justify-end relative z-40">
          
          <div class="w-full h-6 mb-4 flex items-center cursor-default group relative no-drag-btn">
             <div class="absolute w-full h-1 bg-white/10 rounded-full overflow-hidden pointer-events-none">
                <div class="absolute h-full top-0 left-0 bg-gradient-to-r from-starlight-purple to-starlight-cyan transition-all duration-100"
                     :class="player.isBuffering ? 'animate-pulse opacity-50' : ''"
                     :style="{ width: (player.isDragging ? localProgress : player.progress) + '%' }">
                </div>
             </div>
             <input type="range" min="0" max="100" step="0.1"
                    :value="player.isDragging ? localProgress : player.progress"
                    @input="onProgressInput"
                    @change="onProgressChange"
                    class="w-full h-6 opacity-0 cursor-pointer z-10" />
             <div class="absolute h-3 w-3 bg-white rounded-full shadow-[0_0_10px_white] pointer-events-none transition-opacity duration-200"
                  :class="player.isDragging ? 'opacity-100 scale-125' : 'opacity-0 group-hover:opacity-100'"
                  :style="{ left: `calc(${(player.isDragging ? localProgress : player.progress)}% - 6px)` }">
             </div>
          </div>

          <div class="flex items-center justify-between">
            <div class="flex items-center gap-4 w-1/3 group cursor-pointer" 
                 :class="{ 'opacity-0': !player.hasStarted && !player.currentTrack }"
                 @click="toggleLyricsView()">
                
                <div class="relative w-12 h-12 rounded border border-white/10 overflow-hidden transition-transform duration-300 group-hover:scale-110">
                    <img :src="player.currentTrack?.cover" class="w-full h-full object-cover" />
                    
                    <div class="absolute inset-0 bg-black/60 opacity-0 group-hover:opacity-100 flex items-center justify-center transition-all duration-300 backdrop-blur-[1px]">
                       <component :is="showLyrics ? ChevronDown : ChevronUp" :size="20" class="text-starlight-cyan animate-bounce" />
                    </div>
                </div>

                <div class="text-sm overflow-hidden">
                    <div class="text-white font-bold max-w-[150px] truncate group-hover:text-starlight-cyan transition-colors">{{ player.currentTrack?.title || 'No Track' }}</div>
                    <div class="text-xs text-white/40 group-hover:text-white/60">{{ player.currentTrack?.artist || 'Unknown' }}</div>
                </div>
            </div>
            
            <div class="flex items-center gap-6">
              <button class="text-white/40 hover:text-white transition-colors no-drag-btn no-outline" @click="player.toggleMode"><Shuffle v-if="player.playMode === 'shuffle'" :size="20" class="text-starlight-cyan"/><Repeat1 v-else-if="player.playMode === 'loop'" :size="20" class="text-starlight-cyan"/><Repeat v-else :size="20"/></button>
              <button class="text-white hover:text-starlight-cyan transition-colors no-drag-btn no-outline" @click="player.prevTrack"><SkipBack :size="28" fill="currentColor"/></button>
              <button @click="player.togglePlay" class="w-14 h-14 rounded-full bg-white text-cosmos-950 flex items-center justify-center hover:scale-110 active:scale-95 no-drag-btn no-outline"><Pause v-if="player.isPlaying && !player.isPaused" fill="currentColor"/><Play v-else fill="currentColor" class="ml-1"/></button>
              <button class="text-white hover:text-starlight-cyan transition-colors no-drag-btn no-outline" @click="player.nextTrack"><SkipForward :size="28" fill="currentColor"/></button>
              <button class="transition-colors no-drag-btn no-outline" :class="player.showPlaylist ? 'text-starlight-cyan' : 'text-white/40 hover:text-white'" @click="player.togglePlaylist"><ListMusic :size="20"/></button>
            </div>
            <div class="flex items-center justify-end gap-3 w-1/3 group select-none">
              <button @click="toggleMute" class="outline-none no-drag-btn no-outline"><component :is="VolumeIcon" :size="20" class="text-white/60 hover:text-starlight-cyan transition-colors cursor-pointer"/></button>
              <div class="relative w-24 h-4 flex items-center cursor-pointer no-drag-btn" @mousedown="startVolumeDrag"><div ref="volumeBarRef" class="w-full h-1 bg-white/10 rounded-full overflow-hidden pointer-events-none"><div class="h-full bg-starlight-cyan" :class="{ 'transition-[width] duration-150 ease-out': !isDraggingVol }" :style="{ width: player.volume + '%' }"></div></div><div class="absolute h-3 w-3 bg-white rounded-full shadow-[0_0_10px_white] opacity-0 group-hover:opacity-100 transition-opacity pointer-events-none" :style="{ left: `calc(${player.volume}% - 6px)` }"></div></div>
            </div>
          </div>
        </div>

      </section>
    </div>
  </main>
</template>

<style scoped>
.rotate-center { animation: rotate-record 10s linear infinite; }
@keyframes rotate-record { from { transform: rotate(0deg); } to { transform: rotate(360deg); } }
.animate-spin-slow { animation: spin 8s linear infinite; }
.animate-spin-slow-reverse { animation: spin 12s linear infinite reverse; }
.animate-pulse-slow { animation: pulse 4s cubic-bezier(0.4, 0, 0.6, 1) infinite; }
@keyframes float-slow { 0%, 100% { transform: translateY(0) rotate(0deg); } 50% { transform: translateY(-20px) rotate(2deg); } }
@keyframes float-slower { 0%, 100% { transform: translateY(0) rotate(0deg); } 50% { transform: translateY(30px) rotate(-3deg); } }
.animate-float-slow { animation: float-slow 15s ease-in-out infinite; }
.animate-float-slower { animation: float-slower 20s ease-in-out infinite reverse; }

[data-tauri-drag-region] { -webkit-app-region: drag; cursor: default; user-select: none; }
button, input, [role="button"], .no-drag-btn { -webkit-app-region: no-drag; }

.animate-fade-in { animation: fadeIn 0.5s ease-out; }
@keyframes fadeIn { from { opacity: 0; transform: scale(0.95); } to { opacity: 1; transform: scale(1); } }
@keyframes wave { 0%, 100% { height: 4px; } 50% { height: 12px; } }
.animate-wave-1 { animation: wave 0.8s infinite ease-in-out; }
.animate-wave-2 { animation: wave 1.1s infinite ease-in-out; }
.animate-wave-3 { animation: wave 0.9s infinite ease-in-out; }

.slide-right-enter-active, .slide-right-leave-active { transition: transform 0.3s ease-in-out; }
.slide-right-enter-from, .slide-right-leave-to { transform: translateX(100%); }

.slide-fade-enter-active { transition: all 0.3s ease-out; }
.slide-fade-leave-active { transition: all 0.2s cubic-bezier(1, 0.5, 0.8, 1); }
.slide-fade-enter-from { transform: translateX(20px); opacity: 0; }
.slide-fade-leave-to { transform: translateX(-20px); opacity: 0; }
.fade-enter-active { transition: opacity 0.3s ease-out; }
.fade-leave-active { transition: opacity 0.2s ease-in; }
.fade-enter-from, .fade-leave-to { opacity: 0; }

.slide-up-enter-active, .slide-up-leave-active { transition: all 0.4s cubic-bezier(0.2, 0.8, 0.2, 1); }
.slide-up-enter-from, .slide-up-leave-to { opacity: 0; transform: translateY(20px); filter: blur(5px); }

.mask-gradient {
  mask-image: linear-gradient(to bottom, transparent 0%, black 15%, black 85%, transparent 100%);
  -webkit-mask-image: linear-gradient(to bottom, transparent 0%, black 15%, black 85%, transparent 100%);
}
.scrollbar-hide::-webkit-scrollbar { display: none; }
.scrollbar-hide { -ms-overflow-style: none; scrollbar-width: none; }

.shadow-glow { box-shadow: 0 0 15px rgba(255,255,255,0.1); scale: 1.05; }
.shadow-glow-purple { box-shadow: 0 0 15px rgba(189,52,254,0.4); scale: 1.05; }

/* --- 酷狗级歌词核心样式 (Kugou Style) --- */
.kugou-text {
  /* 默认底色：冷白 (半透明度提升到 0.6) */
  color: rgba(255, 255, 255, 0.6); 
  position: relative;
  z-index: 1;
}

/* 激活态：逐字填充 */
.lyric-line.active .kugou-text {
  /* 渐变：左侧高亮(亮白+青) -> 右侧透明 (透出下方的 ::after 底色) */
  background-image: linear-gradient(to right, #ffffff var(--prog), transparent var(--prog));
  -webkit-background-clip: text;
  background-clip: text;
  color: transparent;
}

/* 底色层：保证未唱到的部分显示为灰色，与上层透明部分重叠 */
.lyric-line.active .kugou-text::after {
  content: attr(data-text);
  position: absolute;
  left: 0; top: 0;
  z-index: -1;
  color: rgba(255, 255, 255, 0.6); 
}

/* 聚焦效果 */
.lyric-line.active {
  transform: scale(1.15);
  filter: drop-shadow(0 0 12px rgba(100, 255, 218, 0.4)); /* 冰蓝光晕 */
  opacity: 1;
}
.lyric-line.near {
  transform: scale(0.95);
  opacity: 0.8;
}
.lyric-line.far {
  transform: scale(0.85);
  opacity: 0.4;
}
</style>