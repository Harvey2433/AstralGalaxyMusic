import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';

export function useEngine() {
    const activeEngine = ref('galaxy'); 
    const isDownloadingFFmpeg = ref(false);
    const ffmpegProgress = ref(0);
    const isSmtcEnabled = ref(JSON.parse(localStorage.getItem('smtc_enabled') || 'true'));
    
    const isEngineSwitching = ref(false);
    const hasAudioInitialized = ref(false);
    const engineCoolingRemaining = ref(0);
    const lastEngineSwitchTime = ref(0);
    const lastMixerActionTime = ref(0);

    const channelMode = ref(Number(localStorage.getItem('channel_mode') || '2'));
    const isTrueSurround = ref(JSON.parse(localStorage.getItem('true_surround') || 'false'));

    const availableDevices = ref<string[]>([]);
    const activeDevice = ref('Default');

    const fetchDevices = async () => { 
        try { 
            const realDevices = await invoke<string[]>('get_output_devices');
            availableDevices.value = ['Default', ...realDevices];
        } catch (e) { 
            availableDevices.value = ['Default']; 
        } 
    };

    return { 
        activeEngine, isDownloadingFFmpeg, ffmpegProgress, isSmtcEnabled, 
        isEngineSwitching, hasAudioInitialized, engineCoolingRemaining, 
        lastEngineSwitchTime, lastMixerActionTime, channelMode, isTrueSurround, 
        availableDevices, activeDevice, fetchDevices 
    };
}