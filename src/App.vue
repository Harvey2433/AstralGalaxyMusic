<script setup lang="ts">
import { ref, onMounted, watch } from 'vue';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event'; 
import { usePlayerStore } from './stores/player'; 
import { Play, Heart } from 'lucide-vue-next';

// 导入模块化组件
import TheIsland from './components/TheIsland.vue';
import SideNavigation from './components/SideNavigation.vue';
import PlaylistDrawer from './components/PlaylistDrawer.vue';
import SettingsPanel from './components/SettingsPanel.vue';
import PlayerDashboard from './components/PlayerDashboard.vue';
import PlayerControls from './components/PlayerControls.vue';
import AboutPage from './components/AboutPage.vue'; 
import CreditsPage from './components/CreditsPage.vue'; // 🔥 仅新增：引入贡献滚动页

const player = usePlayerStore();
const appWindow = getCurrentWindow();

// ==========================================
// 窗口原生控制
// ==========================================
const closeWindow = () => appWindow.close();
const minimize = () => appWindow.minimize();
const toggleMaximize = async () => { 
  const isMax = await appWindow.isMaximized(); 
  isMax ? appWindow.unmaximize() : appWindow.maximize(); 
};

// ==========================================
// 顶层导航状态
// ==========================================
const activeTab = ref('dashboard');
const showSettings = ref(false);
const showLyrics = ref(false);

const switchTab = (t: string) => { 
  activeTab.value = t; 
  showSettings.value = t === 'settings'; 
  player.showPlaylist = false; 
};

const switchToMain = () => { 
  showSettings.value = false; 
  activeTab.value = 'dashboard'; 
  player.showPlaylist = false;
};

// ==========================================
// 灵动岛控制与通知
// ==========================================
const islandRef = ref<InstanceType<typeof TheIsland> | null>(null);
const notify = (text: string, type: 'info' | 'error' | 'cooling' = 'info') => {
  islandRef.value?.notify(text, type);
};

// ==========================================
// 🔥 Windows SMTC 状态实时同步 (终极懒加载分离版)
// ==========================================
let isSmtcActiveInBackend = false; 

watch(
  () => [player.currentTrack, player.isPlaying, player.isSmtcEnabled, player.hasStarted], 
  async ([newTrack, isPlaying, isEnabled, hasStarted]) => {
    
    if (!isEnabled || !hasStarted) {
        if (isSmtcActiveInBackend) {
            invoke('toggle_smtc_active', { enable: false }).catch(e => console.error(e));
            isSmtcActiveInBackend = false;
        }
        return;
    }

    if (!isSmtcActiveInBackend) {
        await invoke('toggle_smtc_active', { enable: true }).catch(e => console.error(e));
        isSmtcActiveInBackend = true; 
    }

    if (newTrack && (newTrack as any).id) {
        invoke('sync_smtc_metadata', {
          title: (newTrack as any).title || 'Unknown',
          artist: (newTrack as any).artist || 'Unknown',
          cover: (newTrack as any).cover || ''
        }).catch(e => console.error(e));
    }

    invoke('sync_smtc_status', { isPlaying: !!isPlaying }).catch(e => console.error(e));
    
  }, 
  { deep: true } 
);

// ==========================================
// 🚀 初始化与系统环境封印 (修复窗口消失 Bug)
// ==========================================
onMounted(() => { 
  document.oncontextmenu = (e) => { e.preventDefault(); return false; };
  document.addEventListener('keydown', (e) => { 
    if ((e.ctrlKey || e.metaKey) && ['+', '-', '=', '0'].includes(e.key)) e.preventDefault();
  });
  document.addEventListener('wheel', (e) => {
      if (e.ctrlKey || e.metaKey) e.preventDefault();
  }, { passive: false });

  setTimeout(() => {
    
    appWindow.show().then(() => {
    }).catch(err => console.error("[TAURI] Failed to show window:", err));

    Promise.all([
        listen('smtc-toggle', () => player.togglePlay()),
        listen('smtc-next', () => player.nextTrack()),
        listen('smtc-prev', () => player.prevTrack())
    ]).catch(e => console.error("[TAURI] Event listener bind failed:", e));

    notify('Astral Galaxy Music Player'); 
    player.setNotifier(notify); 
    
    player.initCheck(); 
    player.fetchDevices();
    
  }, 300);
});
</script>

<template>
  <main class="relative flex w-screen h-screen overflow-hidden text-cosmos-100 bg-[#05080a] font-sans rounded-xl border border-white/10">
    <TheIsland ref="islandRef" />

    <div class="absolute top-[-15%] right-[-10%] w-[600px] h-[600px] rounded-full pointer-events-none z-0 animate-float-slow opacity-70" 
      style="background: radial-gradient(circle at 30% 30%, rgba(189, 52, 254, 0.4) 0%, rgba(80, 20, 120, 0.1) 60%, transparent 100%); box-shadow: inset -20px -20px 50px rgba(0,0,0,0.5); filter: blur(40px);"></div>
    <div class="absolute bottom-[-20%] left-[-15%] w-[700px] h-[700px] rounded-full pointer-events-none z-0 animate-float-slower opacity-60" 
      style="background: radial-gradient(circle at 70% 30%, rgba(100, 255, 218, 0.3) 0%, rgba(20, 120, 100, 0.05) 60%, transparent 100%); box-shadow: inset 20px 20px 50px rgba(0,0,0,0.5); filter: blur(50px);"></div>
    <div class="absolute inset-0 bg-[url('https://www.transparenttextures.com/patterns/stardust.png')] opacity-20 mix-blend-overlay pointer-events-none z-0"></div>

    <div class="relative z-10 flex w-full h-full backdrop-blur-[1px]">
      <SideNavigation :activeTab="activeTab" @switch="switchTab" />

      <section class="flex flex-col flex-1 relative z-20">
        <header class="h-16 flex items-center justify-between px-8 border-b border-white/5 bg-cosmos-900/20 cursor-move" data-tauri-drag-region>
          <div class="text-xs font-mono tracking-[0.3em] text-starlight-cyan/50 pointer-events-none opacity-50 select-none">Astral Galaxy Music</div>
          <div class="flex gap-3">
            <button @click="minimize" class="w-3.5 h-3.5 rounded-full bg-yellow-500/20 border border-yellow-500/50 hover:bg-yellow-500 transition-all duration-300 ease-[cubic-bezier(0.2,0.8,0.2,1)] hover:scale-125 active:scale-90 flex items-center justify-center group no-drag-btn no-outline">
              <span class="opacity-0 group-hover:opacity-100 text-[8px] text-white">−</span>
            </button>
            <button @click="toggleMaximize" class="w-3.5 h-3.5 rounded-full bg-green-500/20 border border-green-500/50 hover:bg-green-500 transition-all duration-300 ease-[cubic-bezier(0.2,0.8,0.2,1)] hover:scale-125 active:scale-90 flex items-center justify-center group no-drag-btn no-outline">
              <span class="opacity-0 group-hover:opacity-100 text-[6px] text-white">□</span>
            </button>
            <button @click="closeWindow" class="w-3.5 h-3.5 rounded-full bg-red-500/20 border border-red-500/50 hover:bg-red-500 transition-all duration-300 ease-[cubic-bezier(0.2,0.8,0.2,1)] hover:scale-125 active:scale-90 flex items-center justify-center group no-drag-btn no-outline">
              <span class="opacity-0 group-hover:opacity-100 text-[8px] text-white">✕</span>
            </button>
          </div>
        </header>

        <div class="flex-1 relative overflow-hidden w-full">
          <Transition name="page-spring">
              <div v-if="activeTab === 'likes'" class="absolute inset-0 z-20 flex flex-col p-10 overflow-y-auto scrollbar-hide">
                 <h2 class="text-4xl font-bold font-orbitron text-white mb-8 flex items-center gap-4"><Heart :size="32" class="text-red-500 fill-red-500" /> LIKED TRACKS</h2>
                 <div class="grid grid-cols-1 gap-2">
                    <div v-for="track in player.likedQueue" :key="track.id" @dblclick="player.playTrack(track)" 
                         class="flex items-center gap-4 p-4 rounded-xl bg-white/5 hover:bg-white/10 active:scale-[0.98] transition-all duration-400 ease-[cubic-bezier(0.2,0.8,0.2,1)] group cursor-pointer">
                       <div class="relative w-12 h-12 rounded-lg overflow-hidden">
                          <img :src="track.cover" class="w-full h-full object-cover transition-transform duration-500 group-hover:scale-110" />
                          <div class="absolute inset-0 bg-black/50 opacity-0 group-hover:opacity-100 flex items-center justify-center transition-all duration-300">
                             <Play :size="20" class="text-white fill-white transition-transform duration-300 active:scale-75"/>
                          </div>
                       </div>
                       <div class="flex-1">
                         <div class="text-white font-bold">{{ track.title }}</div>
                         <div class="text-white/40 text-xs">{{ track.artist }}</div>
                       </div>
                       <button @click.stop="player.toggleLike(track)" class="text-red-500 hover:scale-125 active:scale-75 transition-all duration-300 ease-[cubic-bezier(0.2,0.8,0.2,1)]">
                         <Heart :size="20" :class="player.isLiked(track) ? 'fill-red-500' : ''" />
                       </button>
                    </div>
                    <div v-if="player.likedQueue.length === 0" class="text-white/30 text-center mt-20 font-orbitron tracking-widest">EMPTY_VAULT</div>
                 </div>
              </div>
              
              <div v-else-if="activeTab === 'about'" class="absolute inset-0 z-20 flex flex-col">
                  <AboutPage />
              </div>
          </Transition>

          <div v-show="activeTab === 'dashboard' || activeTab === 'settings'" 
               class="absolute inset-0 z-20 transition-all duration-700 ease-[cubic-bezier(0.2,0.8,0.2,1)]" 
               :class="showSettings ? 'opacity-0 scale-95 pointer-events-none blur-md' : 'opacity-100 scale-100 blur-0'">
            <PlayerDashboard :showLyrics="showLyrics" />
          </div>

          <PlaylistDrawer />

          <SettingsPanel v-if="showSettings" @close="switchToMain" @notify="notify" />
        </div>

        <PlayerControls :showLyrics="showLyrics" @toggle-lyrics="showLyrics = !showLyrics" />
      </section>
    </div>

    <Transition name="credits-fade">
      <CreditsPage v-if="player.showCredits" @close="player.endCredits" />
    </Transition>

  </main>
</template>

<style>
/* 基础封印与全局字体设置 */
html { 
  font-size: 16px !important; 
  -webkit-text-size-adjust: 100%; 
  text-size-adjust: 100%; 
  overflow: hidden;
}

body {
  margin: 0;
  background: black;
}

* { 
  user-select: none; 
  -webkit-user-select: none; 
  -webkit-user-drag: none; 
}

/* 页面级物理弹簧动画 */
.page-spring-enter-active, 
.page-spring-leave-active { 
  transition: opacity 0.5s cubic-bezier(0.2, 0.8, 0.2, 1), transform 0.6s cubic-bezier(0.2, 0.8, 0.2, 1); 
}
.page-spring-enter-from { opacity: 0; transform: scale(0.96) translateY(15px); }
.page-spring-leave-to { opacity: 0; transform: scale(1.04) translateY(-15px); }

/* 全局辅助动效 */
.animate-spin-slow { animation: spin 8s linear infinite; }
.animate-spin-slow-reverse { animation: spin 12s linear infinite reverse; }
.animate-pulse-slow { animation: pulse 4s cubic-bezier(0.4, 0, 0.6, 1) infinite; }

@keyframes float-slow { 
  0%, 100% { transform: translateY(0) rotate(0deg); } 
  50% { transform: translateY(-20px) rotate(2deg); } 
}
@keyframes float-slower { 
  0%, 100% { transform: translateY(0) rotate(0deg); } 
  50% { transform: translateY(30px) rotate(-3deg); } 
}

.animate-float-slow { animation: float-slow 15s ease-in-out infinite; }
.animate-float-slower { animation: float-slower 20s ease-in-out infinite reverse; }

/* 拖拽区域支持 */
[data-tauri-drag-region] { -webkit-app-region: drag; cursor: default; }
button, input, select, [role="button"], .no-drag-btn { -webkit-app-region: no-drag; }

/* 隐藏滚动条但保留功能 */
.scrollbar-hide::-webkit-scrollbar { display: none; }
.scrollbar-hide { -ms-overflow-style: none; scrollbar-width: none; }

/* 清除 Focus 轮廓线 */
.no-outline:focus { outline: none; }

/* 🔥 仅新增：贡献者界面的黑幕淡入淡出动画 */
.credits-fade-enter-active, .credits-fade-leave-active { 
  transition: opacity 1.5s ease-in-out; 
}
.credits-fade-enter-from, .credits-fade-leave-to { 
  opacity: 0; 
}
</style>