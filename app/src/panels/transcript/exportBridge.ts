import { invoke } from '@tauri-apps/api/core';
import { save } from '@tauri-apps/plugin-dialog';
import { inTauri } from '../../sheets/hostBridge';

// Where an exported transcript goes: the native save dialog, then `export_write` (`app/src-tauri/src/export.rs`)
// writes to the path the user picked. The browser (dev server) downloads the file instead. Tests inject a bridge.

export interface ExportBridge {
  /** Save `content` as the user chooses; the saved path, or `null` when they cancelled. */
  save(file: { fileName: string; content: string }): Promise<string | null>;
}

const tauriBridge: ExportBridge = {
  async save({ fileName, content }) {
    const path = await save({ title: 'Export transcript', defaultPath: fileName });
    if (path === null) return null;
    await invoke('export_write', { path, content });
    return path;
  },
};

const browserBridge: ExportBridge = {
  async save({ fileName, content }) {
    const url = URL.createObjectURL(new Blob([content], { type: 'text/plain' }));
    const a = document.createElement('a');
    a.href = url;
    a.download = fileName;
    a.click();
    URL.revokeObjectURL(url);
    return fileName;
  },
};

let override: ExportBridge | null = null;

/** Tests inject a bridge; `null` restores the default. */
export function setExportBridge(b: ExportBridge | null): void {
  override = b;
}

export const exportBridge = (): ExportBridge => override ?? (inTauri() ? tauriBridge : browserBridge);
