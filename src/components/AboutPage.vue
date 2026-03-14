<script setup lang="ts">
import { Disc3 } from 'lucide-vue-next';
import { usePlayerStore } from '../stores/player';

const player = usePlayerStore();
</script>

<template>
  <div class="w-full h-full flex flex-col items-center justify-center bg-black/20 animate-fade-in relative z-20">
    
    <div class="mb-10 relative">
       <div class="absolute inset-0 bg-starlight-cyan/10 blur-3xl rounded-full scale-150 animate-pulse"></div>
       <Disc3 :size="80" class="text-starlight-cyan animate-spin-slow relative z-10" />
    </div>
    
    <h1 class="text-4xl font-orbitron font-bold text-white tracking-[0.2em] mb-2">Astral Galaxy</h1>
    <p class="text-xs font-mono text-starlight-cyan/60 tracking-[0.4em] mb-12 uppercase">Hyperion Audio System</p>
    
    <div class="w-80 space-y-4 text-[11px] font-mono tracking-widest">
        <div class="flex justify-between border-b border-white/5 pb-2">
            <span class="text-white/30 uppercase">Version</span>
            <span class="text-white">2.3.0-Alpha</span>
        </div>
        <div class="flex justify-between border-b border-white/5 pb-2">
            <span class="text-white/30 uppercase">Codename</span>
            <span class="text-starlight-purple">Hyperion</span>
        </div>
        <div class="flex justify-between border-b border-white/5 pb-2">
            <span class="text-white/30 uppercase">Channel</span>
            <span class="text-yellow-500">Insider Dev</span>
        </div>
        <div class="flex justify-between border-b border-white/5 pb-2">
            <span class="text-white/30 uppercase">Build Time</span>
            <span class="text-white">2026-03-13</span>
        </div>
    </div>

    <div class="absolute bottom-10 flex gap-8 text-[10px] font-bold tracking-[0.3em] text-white/20">
        <a href="#" class="hover:text-starlight-cyan transition-colors">隐私政策</a>
        <button @click="player.startCredits()" class="hover:text-starlight-cyan transition-colors uppercase outline-none no-drag-btn">贡献</button>
    </div>

  </div>
</template>

<style scoped>
.animate-spin-slow { animation: spin 12s linear infinite; }
.animate-fade-in { animation: fadeIn 0.8s cubic-bezier(0.2, 0.8, 0.2, 1); }
@keyframes fadeIn { from { opacity: 0; transform: translateY(10px); } to { opacity: 1; transform: translateY(0); } }
</style>