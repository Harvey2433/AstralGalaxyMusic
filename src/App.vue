<script setup lang="ts">
import { ref, onMounted, onUnmounted, computed, watch } from 'vue';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { invoke } from '@tauri-apps/api/core';
import { usePlayerStore } from './stores/player'; 
import { 
  Play, Pause, SkipForward, SkipBack, ListMusic, Disc3, Settings, 
  Heart, Mic2, Shuffle, Repeat, Volume1, VolumeX, Volume2,
  Cpu, Zap, HardDrive, Film, CheckCircle2, Terminal, Loader2, AlertCircle,
  Monitor, Sliders, LogOut, LayoutDashboard, ScanEye, Repeat1, AlertTriangle, PlusCircle, AudioLines, Speaker
} from 'lucide-vue-next';

const player = usePlayerStore();

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
  // 修复：将 isSeeking 也纳入 Loading 状态显示，给用户反馈
  if (player.isDragging || player.isSeeking) return 'loading'; 
  if (player.isPlaying) return 'media';
  return 'idle';
});

// --- 声道与设备控制 ---
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

// --- 引擎 ---
const activeSettingTab = ref('core');
const engineStatus = ref('Ready');
const statusType = ref<'idle' | 'loading' | 'success' | 'error'>('success');
const engineLatency = ref<number>(0);
const targetEngineId = ref(''); // 正在尝试初始化的引擎 ID

const engines = [
  { id: 'galaxy', name: 'GalaxyCore', sub: 'HYPERION', icon: Cpu, color: 'text-starlight-cyan', border: 'border-starlight-cyan', desc: 'Native Rust (RAM Accel)' },
  { id: 'bass', name: 'BASS Audio', sub: 'AUDIOPHILE', icon: Zap, color: 'text-yellow-400', border: 'border-yellow-400', desc: 'Audiophile Grade' },
  { id: 'mci', name: 'Windows MCI', sub: 'LEGACY', icon: HardDrive, color: 'text-blue-400', border: 'border-blue-400', desc: 'Legacy System' },
  { id: 'ffmpeg', name: 'FFmpeg', sub: 'UNIVERSAL', icon: Film, color: 'text-purple-400', border: 'border-purple-400', desc: 'Universal Format' }
];

const selectEngine = async (id: string) => {
  if (statusType.value === 'loading' || player.activeEngine === id) return;
  
  targetEngineId.value = id; 
  statusType.value = 'loading';
  engineStatus.value = `INITIALIZING ${id.toUpperCase()}...`;
  notify(`SWITCHING TO ${id.toUpperCase()}...`);
  
  const startTime = performance.now();
  const result = await player.switchEngine(id);
  
  if (result === true) {
    statusType.value = 'success';
    engineStatus.value = `${id.toUpperCase()} ONLINE`;
    engineLatency.value = Math.round(performance.now() - startTime);
    notify(`${id.toUpperCase()} ENGINE READY`);
    targetEngineId.value = ''; // 成功，清除目标，UI回退到 activeEngine 渲染
  } else {
    statusType.value = 'error';
    engineStatus.value = 'INIT FAILED';
    notify(`FAILED TO LOAD ${id.toUpperCase()}`, 'error');
    // 修复：失败2秒后自动复位状态，消除“双重选中”的UI残留
    setTimeout(() => {
        if (targetEngineId.value === id) {
            targetEngineId.value = '';
            statusType.value = 'idle';
            engineStatus.value = 'Ready';
        }
    }, 2000);
  }
};

// --- 控制 ---
const volumeBarRef = ref<HTMLElement | null>(null);
const progressBarRef = ref<HTMLElement | null>(null);
const isDraggingVol = ref(false);
const VolumeIcon = computed(() => { if(player.volume===0)return VolumeX; if(player.volume<50)return Volume1; return Volume2; });

const updateVolume = (e: MouseEvent) => { if(!volumeBarRef.value)return; const rect = volumeBarRef.value.getBoundingClientRect(); player.volume = Math.max(0, Math.min(100, ((e.clientX - rect.left) / rect.width) * 100)); };
const startVolumeDrag = (e: MouseEvent) => { isDraggingVol.value = true; updateVolume(e); window.addEventListener('mousemove', onVolumeDrag); window.addEventListener('mouseup', stopVolumeDrag); };
const onVolumeDrag = (e: MouseEvent) => { if(isDraggingVol.value) updateVolume(e); };
const stopVolumeDrag = () => { isDraggingVol.value = false; window.removeEventListener('mousemove', onVolumeDrag); window.removeEventListener('mouseup', stopVolumeDrag); };

const isDraggingProg = ref(false);
const localProgress = ref(0);
const calculateProgress = (e: MouseEvent): number => {
  if (!progressBarRef.value) return 0;
  const rect = progressBarRef.value.getBoundingClientRect();
  return Math.max(0, Math.min(100, ((e.clientX - rect.left) / rect.width) * 100));
};
const startProgressDrag = (e: MouseEvent) => { 
  player.isDragging = true; 
  isDraggingProg.value = true; 
  localProgress.value = calculateProgress(e); 
  window.addEventListener('mousemove', onProgressDrag); 
  window.addEventListener('mouseup', stopProgressDrag); 
};
const onProgressDrag = (e: MouseEvent) => { if (isDraggingProg.value) localProgress.value = calculateProgress(e); };
const stopProgressDrag = (e: MouseEvent) => { 
  if (!isDraggingProg.value) return;
  isDraggingProg.value = false; 
  const finalPercent = calculateProgress(e);
  // 修复：无论何时松开鼠标，都尝试 seek，不依赖 buffering 状态
  player.seekTo(finalPercent);
  setTimeout(() => { player.isDragging = false; }, 100); 
  window.removeEventListener('mousemove', onProgressDrag); 
  window.removeEventListener('mouseup', stopProgressDrag); 
};
const toggleMute = () => { player.volume = player.volume > 0 ? 0 : 50; };

// --- 动画 ---
const starCanvas = ref<HTMLCanvasElement | null>(null);
let animationFrameId: number;
class Star { x!: number; y!: number; size!: number; opacity!: number; vx!: number; vy!: number; constructor(public w: number, public h: number) { this.reset(w, h); } reset(w:number,h:number){this.x=Math.random()*w;this.y=Math.random()*h;this.size=Math.random()*1.5;this.opacity=Math.random()*0.5+0.1;this.vx=(Math.random()-0.5)*0.2;this.vy=(Math.random()-0.5)*0.2;} draw(ctx:CanvasRenderingContext2D,w:number,h:number){this.x+=this.vx;this.y+=this.vy;if(this.x<0||this.x>w||this.y<0||this.y>h)this.reset(w,h);ctx.fillStyle=`rgba(255,255,255,${this.opacity})`;ctx.beginPath();ctx.arc(this.x,this.y,this.size,0,Math.PI*2);ctx.fill();}}
const initCanvas = () => { const canvas = starCanvas.value; if(!canvas)return; const ctx = canvas.getContext('2d'); if(!ctx)return; canvas.width = window.innerWidth; canvas.height = window.innerHeight; const stars = Array.from({length:150},()=>new Star(canvas.width,canvas.height)); const animate = () => { ctx.clearRect(0,0,canvas.width,canvas.height); stars.forEach(s=>s.draw(ctx,canvas.width,canvas.height)); animationFrameId = requestAnimationFrame(animate); }; animate(); };

onMounted(() => { 
  initCanvas(); window.addEventListener('resize', initCanvas); notify('ASTRAL_SYSTEM ONLINE'); player.setNotifier(notify); player.initCheck(); player.fetchDevices();
  document.oncontextmenu = (e) => { e.preventDefault(); return false; };
  document.onkeydown = (e) => { if(e.key === 'F12' || (e.ctrlKey && e.key === 'r')) { e.preventDefault(); } };
});
onUnmounted(() => cancelAnimationFrame(animationFrameId));
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
        <span class="text-[10px] font-mono font-bold tracking-[0.1em] text-white whitespace-nowrap">SEEKING...</span>
      </div>

      <div class="col-start-1 row-start-1 transition-opacity duration-300 flex items-center gap-3 w-full justify-center" :class="currentIslandMode === 'notification' ? 'opacity-100 z-10' : 'opacity-0 z-0 pointer-events-none'">
        <ScanEye :size="16" class="text-starlight-cyan animate-pulse shrink-0" />
        <span class="text-[10px] font-mono font-bold tracking-[0.1em] text-white whitespace-nowrap overflow-hidden text-ellipsis">{{ notificationText }}</span>
      </div>

      <div class="col-start-1 row-start-1 transition-opacity duration-300 flex items-center gap-3 w-full justify-center" :class="currentIslandMode === 'error' ? 'opacity-100 z-10' : 'opacity-0 z-0 pointer-events-none'">
        <AlertTriangle :size="16" class="text-red-500 animate-pulse shrink-0" />
        <span class="text-[10px] font-mono font-bold tracking-[0.1em] text-red-100 whitespace-nowrap">{{ notificationText }}</span>
      </div>

      <div class="col-start-1 row-start-1 transition-opacity duration-300 flex items-center gap-4 w-full justify-between" :class="currentIslandMode === 'media' ? 'opacity-100 z-10' : 'opacity-0 z-0 pointer-events-none'">
        <div class="w-8 h-8 rounded-full overflow-hidden border border-white/20 relative shrink-0"><img :src="player.currentTrack?.cover" class="w-full h-full object-cover animate-spin-slow" /></div>
        <div class="flex flex-col justify-center flex-1 min-w-0 py-1"><span class="text-xs font-bold text-white leading-tight break-words truncate text-left">{{ player.currentTrack?.title }}</span></div>
        <div class="flex items-end gap-[2px] h-4 shrink-0 ml-auto"><div class="w-[2px] bg-starlight-cyan rounded-full animate-wave-1"></div><div class="w-[2px] bg-starlight-purple rounded-full animate-wave-2"></div><div class="w-[2px] bg-starlight-cyan rounded-full animate-wave-3"></div></div>
      </div>
    </div>

    <canvas ref="starCanvas" class="absolute top-0 left-0 w-full h-full pointer-events-none z-0"></canvas>
    <div class="absolute top-[-10%] right-[-10%] w-[500px] h-[500px] bg-starlight-purple/20 blur-[120px] rounded-full pointer-events-none"></div>
    <div class="absolute bottom-[-10%] left-[-10%] w-[500px] h-[500px] bg-starlight-cyan/10 blur-[100px] rounded-full pointer-events-none"></div>

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
                <div v-for="(track, index) in player.likedQueue" :key="track.id" @dblclick="player.playTrack(track)" class="flex items-center gap-4 p-4 rounded-xl bg-white/5 hover:bg-white/10 transition-all group cursor-pointer">
                   <div class="relative w-12 h-12 rounded-lg overflow-hidden"><img :src="track.cover" class="w-full h-full object-cover" /><div class="absolute inset-0 bg-black/50 opacity-0 group-hover:opacity-100 flex items-center justify-center transition-all"><Play :size="20" class="text-white fill-white"/></div></div>
                   <div class="flex-1"><div class="text-white font-bold">{{ track.title }}</div><div class="text-white/40 text-xs">{{ track.artist }}</div></div>
                   <button @click.stop="player.toggleLike(track)" class="text-red-500 hover:scale-110 transition-transform"><Heart :size="20" class="fill-red-500"/></button>
                </div>
                <div v-if="player.likedQueue.length === 0" class="text-white/30 text-center mt-20">NO LIKED TRACKS YET</div>
             </div>
          </div>

          <div v-else-if="activeTab === 'dashboard'" class="absolute inset-0 flex flex-col items-center justify-center gap-8 p-10 z-20 transition-all duration-500" :class="showSettings ? 'opacity-0 scale-95 pointer-events-none blur-sm' : 'opacity-100 scale-100 blur-0'">
              <div class="relative group">
                <div class="absolute inset-0 rounded-full border border-starlight-cyan/30 scale-110 opacity-0 group-hover:scale-125 group-hover:opacity-100 transition-all duration-700"></div>
                <div class="absolute inset-0 rounded-full border border-starlight-purple/30 scale-105 animate-pulse"></div>
                <div class="w-64 h-64 rounded-full border-4 border-cosmos-800 shadow-[0_0_50px_rgba(0,0,0,0.5)] overflow-hidden animate-spin-slow" :style="{ animationPlayState: player.isPlaying && !player.isBuffering ? 'running' : 'paused' }">
                  <img :src="player.currentTrack?.cover || 'https://images.unsplash.com/photo-1614728853913-6591d801d643?q=80&w=400&auto=format&fit=crop'" class="w-full h-full object-cover opacity-90 select-none" />
                  <div class="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-16 h-16 bg-cosmos-950 rounded-full border border-white/10 flex items-center justify-center">
                    <div class="w-2 h-2 bg-starlight-cyan rounded-full" :class="{ 'animate-ping': player.isPlaying && !player.isBuffering }"></div>
                  </div>
                </div>
              </div>
              <div class="text-center space-y-2 z-10 mt-8">
                <h1 class="text-4xl font-bold font-orbitron tracking-wider text-transparent bg-clip-text bg-gradient-to-r from-white via-starlight-cyan to-white drop-shadow-lg">{{ player.currentTrack?.title || 'No Track Selected' }}</h1>
                <p class="text-lg text-cosmos-300 font-light tracking-widest uppercase">{{ player.currentTrack?.artist || 'Idle' }}</p>
                </div>
          </div>

          <div v-if="player.showPlaylist" class="fixed inset-0 z-20" @click="player.togglePlaylist"></div>
          <Transition name="slide-right">
            <div v-if="player.showPlaylist" class="absolute top-0 right-0 bottom-0 w-80 bg-cosmos-950/90 backdrop-blur-xl border-l border-white/10 z-30 flex flex-col shadow-2xl" @click.stop>
              <div class="p-4 border-b border-white/5 flex justify-between items-center"><h3 class="font-orbitron text-white text-sm tracking-widest">PLAYLIST</h3><span class="text-xs text-starlight-cyan font-mono">{{ player.queue.length }} TRACKS</span></div>
              <div class="flex-1 overflow-y-auto scrollbar-hide p-2">
                <div v-for="(track, index) in player.queue" :key="track.id" @dblclick="player.currentIndex = index; player.loadAndPlay()" class="flex items-center gap-3 p-3 rounded-lg cursor-pointer group border-b border-white/5 transition-all mb-1 hover:bg-white/5">
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
                    <div class="mb-8">
                      <h3 class="text-2xl font-bold text-white mb-2">Decoding Engine</h3>
                      <div class="mt-4 flex items-center gap-3 bg-black/40 p-3 rounded border border-white/5 w-fit"><span class="text-[10px] text-starlight-cyan/60 tracking-widest">STATUS:</span><span class="text-xs font-mono transition-colors duration-300" :class="{'text-green-400': statusType === 'success', 'text-yellow-400 animate-pulse': statusType === 'loading', 'text-red-500': statusType === 'error'}">[{{ engineStatus }}]</span></div>
                    </div>
                    <div class="grid grid-cols-2 gap-4">
                      <div v-for="engine in engines" :key="engine.id" @click="selectEngine(engine.id)" class="relative p-5 rounded-xl border bg-cosmos-900/40 backdrop-blur-sm cursor-pointer transition-all duration-200 group hover:bg-white/5 no-drag-btn no-outline" 
                      :class="[
                        (targetEngineId === engine.id && statusType === 'error') ? 'border-red-500 bg-red-500/10' :
                        (targetEngineId === engine.id && statusType === 'loading') ? 'border-yellow-400 bg-yellow-400/10' :
                        (player.activeEngine === engine.id && (!targetEngineId || targetEngineId !== engine.id)) ? 'border-starlight-cyan bg-starlight-cyan/10' :
                        'border-white/5 hover:border-white/20 bg-cosmos-900/40',
                        
                        (statusType === 'loading' && targetEngineId && targetEngineId !== engine.id) ? 'opacity-50 pointer-events-none' : 'opacity-100'
                      ]">
                        <div v-if="targetEngineId === engine.id || (targetEngineId === '' && player.activeEngine === engine.id)" class="absolute top-4 right-4">
                            <Loader2 v-if="statusType === 'loading' && targetEngineId === engine.id" :size="18" class="text-yellow-400 animate-spin" />
                            <AlertCircle v-else-if="statusType === 'error' && targetEngineId === engine.id" :size="18" class="text-red-500" />
                            <CheckCircle2 v-else-if="player.activeEngine === engine.id && !targetEngineId" :size="18" class="text-starlight-cyan drop-shadow-[0_0_8px_cyan]" />
                        </div>
                        <div class="mb-3 p-2 rounded-lg w-fit transition-colors bg-black/60"><component :is="engine.icon" :size="24" :class="player.activeEngine === engine.id || targetEngineId === engine.id ? engine.color : 'text-white/30'" /></div>
                        <h4 class="text-base font-bold text-white mb-0.5">{{ engine.name }}</h4>
                        <p class="text-[10px] font-mono mb-2 uppercase opacity-80" :class="engine.color">{{ engine.sub }}</p>
                        <p class="text-xs text-white/40 leading-relaxed">{{ engine.desc }}</p>
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
          <div class="w-full h-6 mb-4 flex items-center cursor-default group relative no-drag-btn" @mousedown="startProgressDrag" ref="progressBarRef">
            <div class="w-full h-1 bg-white/10 rounded-full overflow-hidden relative pointer-events-none">
              <div class="absolute h-full rounded-full group-hover:h-2 transition-all top-1/2 -translate-y-1/2" 
                   :class="player.isBuffering ? 'bg-starlight-purple animate-pulse' : 'bg-gradient-to-r from-starlight-purple to-starlight-cyan'"
                   :style="{ width: (isDraggingProg ? localProgress : player.progress) + '%' }">
                <div class="absolute right-0 top-1/2 -translate-y-1/2 w-3 h-3 bg-white rounded-full shadow-[0_0_10px_white] opacity-0 group-hover:opacity-100 transition-opacity"></div>
              </div>
            </div>
          </div>
          <div class="flex items-center justify-between">
            <div class="flex items-center gap-4 w-1/3"><div class="w-12 h-12 rounded bg-white/5 border border-white/10 flex items-center justify-center"><Disc3 class="text-white/20" /></div><div class="text-sm"><div class="text-white max-w-[150px] truncate">{{ player.currentTrack?.title || 'No Track' }}</div><div class="text-xs text-white/40">{{ player.currentTrack?.artist || 'Unknown' }}</div></div></div>
            <div class="flex items-center gap-6">
              <button class="text-white/40 hover:text-white transition-colors no-drag-btn no-outline" @click="player.toggleMode"><Shuffle v-if="player.playMode === 'shuffle'" :size="20" class="text-starlight-cyan"/><Repeat1 v-else-if="player.playMode === 'loop'" :size="20" class="text-starlight-cyan"/><Repeat v-else :size="20"/></button>
              <button class="text-white hover:text-starlight-cyan transition-colors no-drag-btn no-outline" @click="player.prevTrack"><SkipBack :size="28" fill="currentColor"/></button>
              <button @click="player.togglePlay" class="w-14 h-14 rounded-full bg-white text-cosmos-950 flex items-center justify-center hover:scale-110 active:scale-95 no-drag-btn no-outline"><Pause v-if="player.isPlaying" fill="currentColor"/><Play v-else fill="currentColor" class="ml-1"/></button>
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
[data-tauri-drag-region] { -webkit-app-region: drag; cursor: default; user-select: none; }
button, input, [role="button"], .no-drag-btn { -webkit-app-region: no-drag; }

.animate-fade-in { animation: fadeIn 0.3s ease-out; }
@keyframes fadeIn { from { opacity: 0; transform: translateY(5px); } to { opacity: 1; transform: translateY(0); } }
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
</style>

<style>
:root { --focus-ring: none !important; }
*, *::before, *::after { -webkit-tap-highlight-color: transparent; outline: none !important; }
button:focus, button:active, button:focus-visible, .no-outline:focus { outline: none !important; box-shadow: none !important; border-color: transparent !important; }
button::-moz-focus-inner { border: 0; }
</style>