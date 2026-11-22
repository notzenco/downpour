import { useSyncExternalStore } from "react";
import { api, type Download, type EngineEvent, type ProgressItem, type Settings } from "./api";

export interface AppState {
  ready: boolean;
  downloads: Record<string, Download>;
  live: Record<string, ProgressItem>;
  settings: Settings | null;
  totalSpeed: number;
  /** Total speed samples, one per progress tick (0.5 s), newest last. */
  speedHistory: number[];
}

const HISTORY = 120;

let state: AppState = {
  ready: false,
  downloads: {},
  live: {},
  settings: null,
  totalSpeed: 0,
  speedHistory: Array(HISTORY).fill(0),
};
const listeners = new Set<() => void>();

function set(next: Partial<AppState>) {
  state = { ...state, ...next };
  listeners.forEach((l) => l());
}

export function getState() {
  return state;
}

export function useStore<T>(selector: (s: AppState) => T): T {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => selector(state),
  );
}

export async function loadInitial() {
  const snap = await api.snapshot();
  set({
    ready: true,
    settings: snap.settings,
    downloads: Object.fromEntries(snap.downloads.map((d) => [d.id, d])),
  });
}

let lastProgressAt = 0;

export function applyEvent(e: EngineEvent) {
  switch (e.type) {
    case "upsert": {
      const live = { ...state.live };
      if (e.download.status !== "downloading") delete live[e.download.id];
      set({ downloads: { ...state.downloads, [e.download.id]: e.download }, live });
      break;
    }
    case "removed": {
      const downloads = { ...state.downloads };
      const live = { ...state.live };
      for (const id of e.ids) {
        delete downloads[id];
        delete live[id];
      }
      set({ downloads, live });
      break;
    }
    case "progress": {
      lastProgressAt = Date.now();
      const live: Record<string, ProgressItem> = {};
      for (const item of e.items) live[item.id] = item;
      set({
        live,
        totalSpeed: e.totalSpeed,
        speedHistory: [...state.speedHistory.slice(1 - HISTORY), e.totalSpeed],
      });
      break;
    }
    case "settings":
      set({ settings: e.settings });
      break;
  }
}

// Keep the speed graph moving while idle, when the engine sends no ticks.
setInterval(() => {
  if (Date.now() - lastProgressAt > 900) {
    set({ totalSpeed: 0, speedHistory: [...state.speedHistory.slice(1 - HISTORY), 0] });
  }
}, 500);

export async function saveSettings(patch: Partial<Settings>) {
  if (!state.settings) return;
  const saved = await api.updateSettings({ ...state.settings, ...patch });
  set({ settings: saved });
}
