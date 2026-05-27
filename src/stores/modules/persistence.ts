import { Ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { Track, NotificationCallback } from './types';
import { preconvertCover, getOriginalCover } from './playlist';

export function usePersistence(deps: {
    volume: Ref<number>;
    lastActiveVolume: Ref<number>;
    activeEngine: Ref<string>;
    channelMode: Ref<number>;
    isTrueSurround: Ref<boolean>;
    activeDevice: Ref<string>;
    likedQueue: Ref<Track[]>;
    likedIdSet: Set<string>;
    needsInitialization: Ref<boolean>;
    notifyUI: Ref<NotificationCallback | null>;
}) {
    let persistenceTimeout: any = null;

    // Debounced snapshot writer
    const startPersistenceWatch = () => {
        watch(
            () => [
                deps.volume.value,
                deps.activeEngine.value,
                deps.channelMode.value,
                deps.isTrueSurround.value,
                deps.activeDevice.value,
                deps.likedQueue.value.length,
            ],
            () => {
                if (persistenceTimeout) clearTimeout(persistenceTimeout);
                persistenceTimeout = setTimeout(() => {
                    const likedForSave = deps.likedQueue.value.map(t => ({
                        ...t,
                        cover: getOriginalCover(t.cover)
                    }));
                    invoke('update_persistence_snapshot', {
                        data: {
                            settings: {
                                volume: deps.volume.value,
                                engine_id: deps.activeEngine.value,
                                channel_mode: deps.channelMode.value,
                                is_true_surround: deps.isTrueSurround.value,
                                output_device: deps.activeDevice.value
                            },
                            liked_tracks: likedForSave
                        }
                    }).catch(() => {});
                }, 1000);
            }
        );
    };

    const restoreData = async () => {
        try {
            const status = await invoke<string>('init_persistence_layer');
            if (status === 'CORRUPT' || status === 'NO_FILE') {
                if (status === 'CORRUPT') deps.notifyUI.value?.('Configuration corrupted, reset to default', 'error');
                deps.needsInitialization.value = true;
                return null;
            }
            if (status === 'SUCCESS') {
                const data = await invoke<any>('load_astral_data');
                if (data && data.settings) {
                    deps.volume.value = data.settings.volume;
                    deps.lastActiveVolume.value = deps.volume.value;
                    deps.activeEngine.value = data.settings.engine_id;
                    deps.activeDevice.value = data.settings.output_device;
                    deps.channelMode.value = data.settings.channel_mode;
                    deps.isTrueSurround.value = data.settings.is_true_surround;

                    const restoredLiked = data.liked_tracks || [];
                    deps.likedQueue.value = restoredLiked.map((t: Track) => ({
                        ...t,
                        cover: preconvertCover(t.cover)
                    }));
                    deps.likedIdSet.clear();
                    for (const t of deps.likedQueue.value) {
                        if (t.id) deps.likedIdSet.add(t.id);
                    }

                    return data.settings;
                }
            }
        } catch (e) {
            deps.needsInitialization.value = true;
        }
        return null;
    };

    return {
        startPersistenceWatch,
        restoreData,
    };
}