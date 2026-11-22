import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type Status = "queued" | "connecting" | "downloading" | "paused" | "completed" | "failed";
export type Category = "compressed" | "documents" | "music" | "video" | "programs" | "images" | "other";

export interface Segment {
  start: number;
  end: number;
  downloaded: number;
}

export interface Download {
  id: string;
  url: string;
  finalUrl: string | null;
  fileName: string;
  saveDir: string;
  totalSize: number | null;
  downloaded: number;
  resumable: boolean;
  status: Status;
  error: string | null;
  segments: Segment[];
  connections: number;
  category: Category;
  referer: string | null;
  contentType: string | null;
  createdAt: number;
  completedAt: number | null;
  queuePosition: number;
}

export interface Settings {
  downloadDir: string;
  categoryFolders: boolean;
  maxConcurrent: number;
  connections: number;
  speedLimit: number;
  maxRetries: number;
  userAgent: string;
  notifyOnComplete: boolean;
  closeToTray: boolean;
  watchClipboard: boolean;
  theme: "system" | "light" | "dark";
}

export interface ProbeInfo {
  url: string;
  finalUrl: string;
  fileName: string;
  totalSize: number | null;
  resumable: boolean;
  contentType: string | null;
  category: Category;
}

export interface ProgressItem {
  id: string;
  downloaded: number;
  totalSize: number | null;
  speed: number;
  eta: number | null;
  activeConnections: number;
  /** [start, end, done] per segment */
  segments: [number, number, number][];
}

export interface AddRequest {
  url: string;
  fileName?: string;
  saveDir?: string;
  connections?: number;
  referer?: string;
  start: boolean;
}

export type EngineEvent =
  | { type: "upsert"; download: Download }
  | { type: "removed"; ids: string[] }
  | { type: "progress"; items: ProgressItem[]; totalSpeed: number }
  | { type: "settings"; settings: Settings };

export const api = {
  snapshot: () => invoke<{ downloads: Download[]; settings: Settings }>("get_snapshot"),
  probe: (url: string, referer?: string) => invoke<ProbeInfo>("probe_url", { url, referer }),
  add: (requests: AddRequest[]) => invoke<Download[]>("add_downloads", { requests }),
  pause: (ids: string[]) => invoke<void>("pause_downloads", { ids }),
  resume: (ids: string[]) => invoke<void>("resume_downloads", { ids }),
  remove: (ids: string[], deleteFiles: boolean) => invoke<void>("remove_downloads", { ids, deleteFiles }),
  reorder: (id: string, beforeId: string | null) => invoke<void>("reorder_download", { id, beforeId }),
  updateSettings: (settings: Settings) => invoke<Settings>("update_settings", { settings }),
  open: (id: string) => invoke<void>("open_download", { id }),
  reveal: (id: string) => invoke<void>("reveal_download", { id }),
};

export const onEngine = (cb: (e: EngineEvent) => void) => listen<EngineEvent>("engine", (e) => cb(e.payload));

export const errorText = (e: unknown) => (typeof e === "string" ? e : e instanceof Error ? e.message : String(e));
