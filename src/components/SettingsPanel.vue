<script setup lang="ts">
import { ref } from 'vue';
import { usePlayerStore } from '../stores/player';
import { 
  Settings, Terminal, Sliders, Monitor, LogOut, 
  Activity, Cpu, Zap, HardDrive, Film, 
  Loader2, AlertCircle, CheckCircle2, Speaker 
} from 'lucide-vue-next';

const emit = defineEmits<{ (e: 'close'): void; (e: 'notify', text: string, type?: 'info' | 'error' | 'cooling'): void; }>();
const player = usePlayerStore();

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
  if (id === 'ffmpeg' && player.isDownloadingFFmpeg) { emit('notify', 'FFmpeg is currently downloading', 'error'); return; }
  
  if (player.engineCoolingRemaining > 0) {
      targetEngineId.value = id;
      engineState.value = 'failed';
      emit('notify', `System cooling: ${player.engineCoolingRemaining}s`, 'cooling');
      setTimeout(() => { engineState.value = 'idle'; targetEngineId.value = ''; }, 2000);
      return;
  }

  targetEngineId.value = id; engineState.value = 'switching'; emit('notify', `Initializing ${id}...`);
  const result = await player.switchEngine(id);
  if (result === 'SUCCESS') { engineState.value = 'success'; emit('notify', `${id.charAt(0).toUpperCase() + id.slice(1)} engine ready`); setTimeout(() => { engineState.value = 'idle'; targetEngineId.value = ''; }, 1500); } 
  else if (result === 'DOWNLOADING') { engineState.value = 'idle'; targetEngineId.value = ''; } 
  else if (result === 'COOLING') { engineState.value = 'failed'; setTimeout(() => { engineState.value = 'idle'; targetEngineId.value = ''; }, 2000); }
  else { engineState.value = 'failed'; emit('notify', `Failed to load ${id}`, 'error'); setTimeout(() => { engineState.value = 'idle'; targetEngineId.value = ''; }, 2000); }
};

const setChannel = async (ch: number) => { 
    if (player.isEngineSwitching || player.isDownloadingFFmpeg) {
        emit('notify', 'System busy: Engine locked', 'error');
        return;
    }

    const res = await player.setChannelMode(ch); 
    if (res === 'SUCCESS') {
        emit('notify', `Audio output: ${ch === 2 ? 'Stereo' : ch.toFixed(1) + ' Surround'}`); 
    }
};

const selectOutputDevice = async (e: Event) => { 
    const target = e.target as HTMLSelectElement;
    if (player.isEngineSwitching || player.isDownloadingFFmpeg) {
        target.value = player.activeDevice;
        emit('notify', 'System busy: Engine locked', 'error');
        return;
    }

    const res = await player.setOutputDevice(target.value); 
    if (res === 'THROTTLED' || res === 'FAILED') {
        target.value = player.activeDevice; 
    } else if (res === 'SUCCESS') {
        emit('notify', `Output: ${target.value}`);
    }
};

const toggleTrueSurround = async () => {
    if (player.channelMode === 2) return; 
    
    if (player.isEngineSwitching || player.isDownloadingFFmpeg) {
        emit('notify', 'System busy: Engine locked', 'error');
        return;
    }

    const res = await player.toggleTrueSurround();
    if (res === 'SUCCESS') {
        emit('notify', player.isTrueSurround ? 'True surround enabled' : 'Virtual surround enabled');
    }
};

const toggleSMTC = () => {
    player.isSmtcEnabled = !player.isSmtcEnabled;
    localStorage.setItem('smtc_enabled', JSON.stringify(player.isSmtcEnabled));
    emit('notify', player.isSmtcEnabled ? 'Native SMTC enabled' : 'Native SMTC disabled');
};
</script>

<template>
  <Transition name="panel-fade">
    <div class="absolute inset-0 z-30 flex bg-cosmos-950/80 backdrop-blur-xl">
      
      <div class="w-64 h-full bg-black/40 flex flex-col p-6 z-20 border-r border-white/5 shadow-[10px_0_30px_rgba(0,0,0,0.5)]">
        <h2 class="text-xl font-orbitron font-bold text-white mb-8 flex items-center gap-2"><Settings :size="20" class="text-starlight-purple"/> SETTINGS</h2>
        <nav class="space-y-2 flex-1">
          <button @click="activeSettingTab = 'core'" class="w-full flex items-center gap-3 p-3 rounded-xl transition-all duration-400 ease-[cubic-bezier(0.2,0.8,0.2,1)] text-sm font-bold tracking-wider active:scale-95 no-drag-btn no-outline" :class="activeSettingTab === 'core' ? 'bg-starlight-cyan/15 text-starlight-cyan shadow-[0_4px_15px_rgba(100,255,218,0.1)]' : 'text-white/40 hover:text-white hover:bg-white/10'"><Terminal :size="18" /> CORE SYSTEM</button>
          <button @click="activeSettingTab = 'audio'" class="w-full flex items-center gap-3 p-3 rounded-xl transition-all duration-400 ease-[cubic-bezier(0.2,0.8,0.2,1)] text-sm font-bold tracking-wider active:scale-95 no-drag-btn no-outline" :class="activeSettingTab === 'audio' ? 'bg-starlight-cyan/15 text-starlight-cyan shadow-[0_4px_15px_rgba(100,255,218,0.1)]' : 'text-white/40 hover:text-white hover:bg-white/10'"><Sliders :size="18" /> AUDIO MIXER</button>
          <button @click="activeSettingTab = 'display'" class="w-full flex items-center gap-3 p-3 rounded-xl transition-all duration-400 ease-[cubic-bezier(0.2,0.8,0.2,1)] text-sm font-bold tracking-wider active:scale-95 no-drag-btn no-outline" :class="activeSettingTab === 'display' ? 'bg-starlight-cyan/15 text-starlight-cyan shadow-[0_4px_15px_rgba(100,255,218,0.1)]' : 'text-white/40 hover:text-white hover:bg-white/10'"><Monitor :size="18" /> HOLOGRAM UI</button>
        </nav>
        <button @click="emit('close')" class="flex items-center justify-center w-full gap-2 p-3 rounded-xl bg-white/5 text-xs text-white/50 hover:text-white hover:bg-red-500/20 hover:border-red-500/50 border border-transparent mt-auto transition-all duration-400 ease-[cubic-bezier(0.2,0.8,0.2,1)] active:scale-95 no-drag-btn no-outline"><LogOut :size="14" /> EXIT CONFIGURATION</button>
      </div>
      
      <div class="flex-1 h-full relative z-10">
        <Transition name="slide-up-fade" mode="out-in">
          
          <div v-if="activeSettingTab === 'core'" class="absolute inset-0 overflow-y-auto scrollbar-hide p-10">
            <div class="max-w-4xl">
                <div class="mb-8 flex items-end justify-between">
                    <div><h3 class="text-2xl font-bold text-white mb-2">Decoding Engine</h3><p class="text-sm text-white/40">Select the audio core driver for signal processing.</p></div>
                    <div class="flex items-center gap-2 bg-black/40 p-2 px-3 rounded-lg border border-white/5"><Activity :size="14" class="text-starlight-cyan" /><span class="text-xs font-mono text-starlight-cyan/80">LATENCY: NORMAL</span></div>
                </div>

                <div class="grid grid-cols-2 gap-5 p-2 -ml-2">
                  <div v-for="engine in engines" :key="engine.id" @click="selectEngine(engine.id)" 
                    class="relative p-6 rounded-2xl border bg-cosmos-900/40 backdrop-blur-sm cursor-pointer transition-all duration-500 ease-[cubic-bezier(0.2,0.8,0.2,1)] group hover:bg-white/5 active:scale-95 no-drag-btn no-outline overflow-hidden" 
                    :class="[
                       (targetEngineId === engine.id && engineState === 'failed') ? 'border-red-500 bg-red-500/10' :
                       (targetEngineId === engine.id && engineState === 'switching') ? 'border-yellow-400 bg-yellow-400/10' :
                       (player.activeEngine === engine.id && engineState === 'idle') ? `bg-opacity-20 ${engine.border} ${engine.glow} z-10 scale-[1.02]` :
                       (engine.id === 'ffmpeg' && player.isDownloadingFFmpeg) ? 'border-yellow-400 bg-yellow-400/10' :
                       'border-white/5 hover:border-white/20 hover:shadow-lg z-0',
                       (engineState === 'switching' && targetEngineId !== engine.id) ? 'opacity-50 grayscale scale-[0.98]' : 'opacity-100'
                    ]">
                    
                    <div v-if="player.activeEngine === engine.id && engineState === 'idle'" class="absolute top-0 right-0 p-4">
                        <div class="flex items-center gap-2"><span class="text-[10px] font-bold tracking-widest" :class="engine.color">ACTIVE</span><div class="w-2 h-2 rounded-full animate-pulse" :class="engine.color.replace('text-', 'bg-')"></div></div>
                    </div>

                    <div v-if="targetEngineId === engine.id || (engine.id === 'ffmpeg' && player.isDownloadingFFmpeg)" class="absolute top-4 right-4">
                        <Loader2 v-if="engineState === 'switching' || (engine.id === 'ffmpeg' && player.isDownloadingFFmpeg)" :size="18" class="text-yellow-400 animate-spin" />
                        <AlertCircle v-else-if="engineState === 'failed'" :size="18" class="text-red-500" />
                        <CheckCircle2 v-else-if="engineState === 'success'" :size="18" class="text-starlight-cyan drop-shadow-[0_0_8px_cyan]" />
                    </div>

                    <div class="mb-4 p-3 rounded-xl w-fit transition-colors duration-400 bg-black/60 relative z-10 group-hover:scale-110 ease-[cubic-bezier(0.2,0.8,0.2,1)]"><component :is="engine.icon" :size="24" :class="player.activeEngine === engine.id || targetEngineId === engine.id || (engine.id === 'ffmpeg' && player.isDownloadingFFmpeg) ? engine.color : 'text-white/30'" /></div>
                    <h4 class="text-lg font-bold text-white mb-0.5 relative z-10 transition-transform duration-400">{{ engine.name }}</h4>
                    <p class="text-[10px] font-mono mb-3 uppercase opacity-80 relative z-10 transition-colors" :class="engine.color">{{ engine.sub }}</p>
                    <p class="text-xs text-white/40 leading-relaxed relative z-10">{{ engine.desc }}</p>
                  </div>
                </div>
            </div>
          </div>
          
          <div v-else-if="activeSettingTab === 'audio'" class="absolute inset-0 overflow-y-auto scrollbar-hide p-10">
              <div class="max-w-4xl">
                  <h3 class="text-2xl font-bold text-white mb-2">Audio Channels</h3>
                  <p class="text-sm text-white/40 mb-8">Configure output mapping for surround sound systems.</p>

                  <div class="mb-8 p-5 bg-white/5 rounded-2xl border border-white/5 transition-all duration-400 hover:border-white/20">
                      <label class="text-xs font-bold text-starlight-cyan tracking-widest mb-3 block">OUTPUT DEVICE</label>
                      <select @change="selectOutputDevice" class="w-full bg-black/50 border border-white/10 rounded-xl p-3 text-white text-sm focus:border-starlight-cyan outline-none transition-colors duration-300"><option v-for="dev in player.availableDevices" :key="dev" :value="dev" :selected="player.activeDevice === dev">{{ dev }}</option></select>
                  </div>
                  
                  <div class="grid grid-cols-3 gap-5 mb-6 p-2 -ml-2">
                      <button @click="setChannel(2)" class="relative py-6 px-4 rounded-2xl border flex flex-col items-center justify-center gap-4 transition-all duration-400 ease-[cubic-bezier(0.2,0.8,0.2,1)] active:scale-95 no-drag-btn outline-none" :class="player.channelMode === 2 ? 'bg-starlight-cyan/15 border-starlight-cyan text-white shadow-[0_0_20px_rgba(100,255,218,0.15)] scale-[1.05] z-10' : 'bg-white/5 border-white/5 text-white/40 hover:bg-white/10 hover:scale-[1.02] z-0'"><Speaker :size="32" class="transition-transform duration-400" :class="player.channelMode === 2 ? 'scale-110' : ''" /><span class="font-bold tracking-widest text-xs">STEREO (2.0)</span></button>
                      <button @click="setChannel(6)" class="relative py-6 px-4 rounded-2xl border flex flex-col items-center justify-center gap-4 transition-all duration-400 ease-[cubic-bezier(0.2,0.8,0.2,1)] active:scale-95 no-drag-btn outline-none" :class="player.channelMode === 6 ? 'bg-starlight-cyan/15 border-starlight-cyan text-white shadow-[0_0_20px_rgba(100,255,218,0.15)] scale-[1.05] z-10' : 'bg-white/5 border-white/5 text-white/40 hover:bg-white/10 hover:scale-[1.02] z-0'"><Speaker :size="32" class="transition-transform duration-400" :class="player.channelMode === 6 ? 'scale-110' : ''" /><span class="font-bold tracking-widest text-xs">SURROUND (5.1)</span></button>
                      <button @click="setChannel(8)" class="relative py-6 px-4 rounded-2xl border flex flex-col items-center justify-center gap-4 transition-all duration-400 ease-[cubic-bezier(0.2,0.8,0.2,1)] active:scale-95 no-drag-btn outline-none" :class="player.channelMode === 8 ? 'bg-starlight-cyan/15 border-starlight-cyan text-white shadow-[0_0_20px_rgba(100,255,218,0.15)] scale-[1.05] z-10' : 'bg-white/5 border-white/5 text-white/40 hover:bg-white/10 hover:scale-[1.02] z-0'"><Speaker :size="32" class="transition-transform duration-400" :class="player.channelMode === 8 ? 'scale-110' : ''" /><span class="font-bold tracking-widest text-xs">SURROUND (7.1)</span></button>
                  </div>

                  <div class="p-5 rounded-2xl border transition-all duration-400 flex items-center justify-between"
                       :class="player.channelMode === 2 ? 'opacity-40 grayscale pointer-events-none bg-black/20 border-white/5' : (player.isTrueSurround ? 'bg-starlight-cyan/10 border-starlight-cyan shadow-[0_0_15px_rgba(100,255,218,0.1)]' : 'bg-white/5 border-white/5 hover:border-white/20')">
                      <div>
                          <h4 class="text-white font-bold tracking-widest text-sm" :class="player.isTrueSurround && player.channelMode !== 2 ? 'text-starlight-cyan' : ''">PHYSICAL TRUE SURROUND</h4>
                          <p class="text-[10px] text-white/40 mt-1 uppercase font-mono">
                              <span v-if="player.channelMode === 2">NOT AVAILABLE IN STEREO MODE</span>
                              <span v-else-if="player.isTrueSurround">DIRECT OUTPUT TO 5.1/7.1 HOME THEATER</span>
                              <span v-else>HRTF VIRTUAL DOWNMIX FOR HEADPHONES</span>
                          </p>
                      </div>
                      <button 
                          @click="toggleTrueSurround"
                          class="w-12 h-6 rounded-full transition-all duration-500 relative no-drag-btn outline-none"
                          :class="player.isTrueSurround && player.channelMode !== 2 ? 'bg-starlight-cyan' : 'bg-white/10'"
                      >
                          <div class="absolute top-1 w-4 h-4 bg-white rounded-full transition-all duration-500"
                               :class="player.isTrueSurround && player.channelMode !== 2 ? 'left-7' : 'left-1'"></div>
                      </button>
                  </div>
              </div>
          </div>
          
           <div v-else-if="activeSettingTab === 'display'" class="absolute inset-0 overflow-y-auto scrollbar-hide p-10">
               <div class="max-w-4xl">
                   <h3 class="text-2xl font-bold text-white mb-2">Hologram UI & System Integration</h3>
                   <p class="text-sm text-white/40 mb-8">Configure system-level UI integrations and display features.</p>
                   
                   <div class="mb-6 p-5 bg-white/5 rounded-2xl border border-white/5 transition-all duration-400 hover:border-white/20 flex items-center justify-between">
                      <div>
                          <h4 class="text-white font-bold tracking-widest text-sm">NATIVE WINDOWS SMTC</h4>
                          <p class="text-[10px] text-white/40 mt-1 uppercase">Sync track metadata to system media overlay.</p>
                      </div>
                      <button 
                          @click="toggleSMTC"
                          class="w-12 h-6 rounded-full transition-all duration-500 relative no-drag-btn outline-none"
                          :class="player.isSmtcEnabled ? 'bg-starlight-cyan' : 'bg-white/10'"
                      >
                          <div class="absolute top-1 w-4 h-4 bg-white rounded-full transition-all duration-500"
                               :class="player.isSmtcEnabled ? 'left-7' : 'left-1'"></div>
                      </button>
                  </div>

                  <div class="mt-20 flex flex-col items-center justify-center opacity-30 pointer-events-none">
                      <Monitor :size="48" class="mb-4 text-white animate-pulse-slow"/>
                      <p class="text-white font-mono tracking-widest text-xs">MORE UI SETTINGS COMING SOON...</p>
                  </div>
               </div>
           </div>

        </Transition>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.scrollbar-hide::-webkit-scrollbar { display: none; }
.scrollbar-hide { -ms-overflow-style: none; scrollbar-width: none; }
.panel-fade-enter-active, .panel-fade-leave-active { transition: opacity 0.5s cubic-bezier(0.2, 0.8, 0.2, 1); }
.panel-fade-enter-from, .panel-fade-leave-to { opacity: 0; }
.slide-up-fade-enter-active { transition: all 0.5s cubic-bezier(0.2, 0.8, 0.2, 1); }
.slide-up-fade-leave-active { transition: all 0.3s cubic-bezier(0.2, 0.8, 0.2, 1); }
.slide-up-fade-enter-from { transform: translateY(20px) scale(0.98); opacity: 0; filter: blur(4px); }
.slide-up-fade-leave-to { transform: translateY(-10px) scale(0.98); opacity: 0; filter: blur(4px); }
</style>