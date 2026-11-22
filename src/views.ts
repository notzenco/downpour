import type { Category, Download, ProgressItem } from "./api";

export type View =
  | { kind: "all" }
  | { kind: "active" }
  | { kind: "paused" }
  | { kind: "completed" }
  | { kind: "failed" }
  | { kind: "category"; category: Category };

export const CATEGORIES: { id: Category; label: string }[] = [
  { id: "compressed", label: "Archives" },
  { id: "documents", label: "Documents" },
  { id: "video", label: "Video" },
  { id: "music", label: "Music" },
  { id: "images", label: "Images" },
  { id: "programs", label: "Programs" },
  { id: "other", label: "Other" },
];

export function matches(d: Download, v: View): boolean {
  switch (v.kind) {
    case "all":
      return true;
    case "active":
      return d.status === "downloading" || d.status === "connecting" || d.status === "queued";
    case "paused":
      return d.status === "paused";
    case "completed":
      return d.status === "completed";
    case "failed":
      return d.status === "failed";
    case "category":
      return d.category === v.category;
  }
}

export function sameView(a: View, b: View) {
  return a.kind === b.kind && (a.kind !== "category" || a.category === (b as typeof a).category);
}

const RANK: Record<Download["status"], number> = {
  downloading: 0,
  connecting: 0,
  queued: 1,
  failed: 2,
  paused: 2,
  completed: 3,
};

/** Running work first, then the queue in order, then stopped items, then history newest first. */
export function sortDownloads(list: Download[]): Download[] {
  return [...list].sort((a, b) => {
    const r = RANK[a.status] - RANK[b.status];
    if (r) return r;
    if (a.status === "completed") return (b.completedAt ?? 0) - (a.completedAt ?? 0);
    if (a.status === "paused" || a.status === "failed") return b.createdAt - a.createdAt;
    return a.queuePosition - b.queuePosition;
  });
}

export interface Progress {
  downloaded: number;
  total: number | null;
  speed: number;
  eta: number | null;
  activeConnections: number;
  segments: [number, number, number][];
}

export function progressOf(d: Download, live?: ProgressItem): Progress {
  if (live) {
    return {
      downloaded: live.downloaded,
      total: live.totalSize,
      speed: live.speed,
      eta: live.eta,
      activeConnections: live.activeConnections,
      segments: live.segments,
    };
  }
  const total = d.totalSize;
  return {
    downloaded: d.status === "completed" ? (total ?? d.downloaded) : d.downloaded,
    total,
    speed: 0,
    eta: null,
    activeConnections: 0,
    segments:
      total && d.status !== "completed"
        ? d.segments.map((s) => [s.start, s.end, Math.min(s.downloaded, s.end - s.start)] as [number, number, number])
        : [],
  };
}
