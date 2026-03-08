<script setup lang="ts">
import { LayoutDashboard, Heart, Settings, Disc3 } from 'lucide-vue-next';
defineProps<{ activeTab: string }>();
defineEmits(['switch']);
</script>

<template>
  <aside class="flex flex-col w-20 h-full border-r border-white/5 bg-cosmos-950/40 backdrop-blur-md z-50" data-tauri-drag-region>
    <div class="flex items-center justify-center h-20 text-starlight-cyan pointer-events-none transition-transform duration-700 hover:scale-110"><Disc3 :size="32" class="animate-spin-slow" /></div>
    
    <nav class="flex flex-col items-center gap-6 mt-10">
      <button @click="$emit('switch', 'dashboard')" class="group relative w-12 h-12 flex items-center justify-center rounded-2xl transition-all duration-400 ease-[cubic-bezier(0.2,0.8,0.2,1)] active:scale-75 no-drag-btn no-outline" :class="activeTab === 'dashboard' ? 'bg-white/10 text-white shadow-[0_0_20px_rgba(255,255,255,0.15)] scale-110' : 'text-white/40 hover:text-white hover:bg-white/10 hover:scale-105'"><LayoutDashboard :size="22" class="transition-transform duration-300 group-hover:scale-110" /><div v-if="activeTab === 'dashboard'" class="absolute inset-0 bg-white/5 rounded-2xl blur-md"></div></button>
      
      <button @click="$emit('switch', 'likes')" class="group relative w-12 h-12 flex items-center justify-center rounded-2xl transition-all duration-400 ease-[cubic-bezier(0.2,0.8,0.2,1)] active:scale-75 no-drag-btn no-outline" :class="activeTab === 'likes' ? 'bg-white/10 text-white shadow-[0_0_20px_rgba(255,255,255,0.15)] scale-110' : 'text-white/40 hover:text-white hover:bg-white/10 hover:scale-105'"><Heart :size="22" class="transition-transform duration-300 group-hover:scale-110" /></button>
      
      <button @click="$emit('switch', 'settings')" class="mt-auto mb-8 w-12 h-12 flex items-center justify-center rounded-2xl transition-all duration-400 ease-[cubic-bezier(0.2,0.8,0.2,1)] active:scale-75 no-drag-btn no-outline" :class="activeTab === 'settings' ? 'bg-starlight-purple/20 text-starlight-purple shadow-[0_0_20px_rgba(189,52,254,0.4)] scale-110' : 'text-white/40 hover:text-white hover:bg-white/10 hover:scale-105'"><Settings :size="22" class="transition-transform duration-300 group-hover:rotate-45 group-hover:scale-110" /></button>
    </nav>
  </aside>
</template>