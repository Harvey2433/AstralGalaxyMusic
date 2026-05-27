import { Ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Track } from './types';
import { preconvertCover } from './playlist';

const DEFAULT_COVER = 'https://picui.ogmua.cn/s1/2026/03/09/69aeb0db3989e.webp';

export function useImporter(deps: {
    queue: Ref<Track[]>;
    isImporting: Ref<boolean>;
    importCount: Ref<number>;
    importTotal: Ref<number>;
    importProgress: Ref<number>;
    notifyUI: Ref<((msg: string, type?: 'info' | 'error' | 'cooling') => void) | null>;
}) {
    const pathSet = new Set<string>();
    let listenersBound = false;

    const setupImportListeners = async () => {
        if (listenersBound) return;
        listenersBound = true;

        // Sync pathSet with existing queue
        for (const track of deps.queue.value) {
            if (track.path) pathSet.add(track.path);
        }

        await listen<number>('import-start', (e) => {
            deps.importTotal.value = e.payload;
            deps.importCount.value = 0;
            deps.importProgress.value = 0;
        });

        await listen<Track>('import-track', (e) => {
            const t = e.payload;
            deps.importCount.value++;
            if (deps.importTotal.value > 0) {
                deps.importProgress.value = (deps.importCount.value / deps.importTotal.value) * 100;
            }

            if (!pathSet.has(t.path)) {
                pathSet.add(t.path);
                deps.queue.value.push({
                    ...t,
                    id: Date.now().toString() + Math.random().toString(36).substring(2, 8),
                    cover: t.cover === 'DEFAULT_COVER' ? DEFAULT_COVER : preconvertCover(t.cover),
                    isAvailable: true
                });
            }
        });

        await listen('import-finish', () => {
            deps.isImporting.value = false;
            setTimeout(() => deps.notifyUI.value?.('Library updated'), 400);
        });

        await listen('import-cancel', () => {
            deps.isImporting.value = false;
        });
    };

    const importTracks = async () => {
        if (deps.isImporting.value) return;
        await setupImportListeners();
        deps.isImporting.value = true;
        deps.importProgress.value = 0;
        deps.importCount.value = 0;
        deps.importTotal.value = 0;
        try {
            await invoke('import_music');
        } catch (e) {
            deps.isImporting.value = false;
        }
    };

    const initCheck = async () => {
        await setupImportListeners();
        deps.queue.value.forEach(track => {
            invoke('check_file_exists', { path: track.path })
                .then((exists) => { track.isAvailable = exists as boolean; })
                .catch(() => { track.isAvailable = false; });
        });
    };

    return {
        importTracks,
        initCheck,
        setupImportListeners,
    };
}