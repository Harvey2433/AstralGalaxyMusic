<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue';
import { LayoutDashboard, Heart, Settings, Disc3, Menu, Info } from 'lucide-vue-next';

defineProps<{ activeTab: string }>();
const emit = defineEmits(['switch']);

const showMoreMenu = ref(false);
const menuRef = ref<HTMLElement | null>(null);
const moreBtnRef = ref<HTMLElement | null>(null);

const toggleMoreMenu = () => {
  showMoreMenu.value = !showMoreMenu.value;
};

// 🔥 无感点击外部拦截核心：不阻断事件冒泡，只负责关闭菜单
const closeMenuOnClickOutside = (e: MouseEvent) => {
  if (!showMoreMenu.value) return;
  if (moreBtnRef.value && moreBtnRef.value.contains(e.target as Node)) return;
  if (menuRef.value && menuRef.value.contains(e.target as Node)) return;
  showMoreMenu.value = false;
};

onMounted(() => {
  document.addEventListener('mousedown', closeMenuOnClickOutside);
});

onUnmounted(() => {
  document.removeEventListener('mousedown', closeMenuOnClickOutside);
});

const selectAbout = () => {
  emit('switch', 'about');
  showMoreMenu.value = false;
};
</script>

<template>
  <aside class="flex flex-col w-20 h-full border-r border-white/5 bg-cosmos-950/40 backdrop-blur-md z-50" data-tauri-drag-region>
    <div class="flex items-center justify-center h-20 text-starlight-cyan pointer-events-none transition-transform duration-700 hover:scale-110">
      <Disc3 :size="32" class="animate-spin-slow" />
    </div>
    
    <nav class="flex flex-col items-center gap-6 mt-10 w-full flex-1">
      <button @click="$emit('switch', 'dashboard')" class="group relative w-12 h-12 flex items-center justify-center rounded-2xl transition-all duration-400 ease-[cubic-bezier(0.2,0.8,0.2,1)] active:scale-75 no-drag-btn no-outline" :class="activeTab === 'dashboard' ? 'bg-white/10 text-white shadow-[0_0_20px_rgba(255,255,255,0.15)] scale-110' : 'text-white/40 hover:text-white hover:bg-white/10 hover:scale-105'">
        <LayoutDashboard :size="22" class="transition-transform duration-300 group-hover:scale-110" />
        <div v-if="activeTab === 'dashboard'" class="absolute inset-0 bg-white/5 rounded-2xl blur-md"></div>
      </button>
      
      <button @click="$emit('switch', 'likes')" class="group relative w-12 h-12 flex items-center justify-center rounded-2xl transition-all duration-400 ease-[cubic-bezier(0.2,0.8,0.2,1)] active:scale-75 no-drag-btn no-outline" :class="activeTab === 'likes' ? 'bg-white/10 text-white shadow-[0_0_20px_rgba(255,255,255,0.15)] scale-110' : 'text-white/40 hover:text-white hover:bg-white/10 hover:scale-105'">
        <Heart :size="22" class="transition-transform duration-300 group-hover:scale-110" />
      </button>
      
      <button @click="$emit('switch', 'settings')" class="mt-auto group relative w-12 h-12 flex items-center justify-center rounded-2xl transition-all duration-400 ease-[cubic-bezier(0.2,0.8,0.2,1)] active:scale-75 no-drag-btn no-outline" :class="activeTab === 'settings' ? 'bg-white/10 text-white shadow-[0_0_20px_rgba(255,255,255,0.15)] scale-110' : 'text-white/40 hover:text-white hover:bg-white/10 hover:scale-105'">
        <Settings :size="22" class="transition-transform duration-300 group-hover:scale-110" />
      </button>

      <div class="relative mb-8 w-full flex justify-center">
        <button ref="moreBtnRef" @click.stop="toggleMoreMenu" class="group relative w-12 h-12 flex items-center justify-center rounded-2xl transition-all duration-400 ease-[cubic-bezier(0.2,0.8,0.2,1)] active:scale-75 no-drag-btn no-outline" :class="(activeTab === 'about' || showMoreMenu) ? 'bg-white/10 text-white shadow-[0_0_20px_rgba(255,255,255,0.15)] scale-110' : 'text-white/40 hover:text-white hover:bg-white/10 hover:scale-105'">
          <Menu :size="22" class="transition-transform duration-300 group-hover:scale-110" />
        </button>

        <Transition name="menu-fade">
          <div v-if="showMoreMenu" ref="menuRef" class="absolute left-[70px] bottom-0 w-44 bg-cosmos-900/95 backdrop-blur-xl border border-white/10 rounded-2xl p-2 shadow-[15px_15px_40px_rgba(0,0,0,0.8)] z-[100] origin-bottom-left">
            <button @click.stop="selectAbout" class="w-full flex items-center gap-3 p-3 rounded-xl text-white/60 hover:text-white hover:bg-white/10 transition-colors duration-300 active:scale-95 no-outline">
              <Info :size="18" />
              <span class="text-sm font-bold tracking-widest">About</span>
            </button>
          </div>
        </Transition>
      </div>

    </nav>
  </aside>
</template>

<style scoped>
.menu-fade-enter-active, .menu-fade-leave-active { transition: all 0.3s cubic-bezier(0.2, 0.8, 0.2, 1); }
.menu-fade-enter-from, .menu-fade-leave-to { opacity: 0; transform: scale(0.9) translateX(-10px); }
</style>