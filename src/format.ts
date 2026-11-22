const UNITS = ["B", "KB", "MB", "GB", "TB"];

export function formatBytes(bytes: number | null | undefined): string {
  if (bytes == null || !Number.isFinite(bytes)) return "Unknown size";
  let v = bytes;
  let unit = 0;
  while (v >= 1000 && unit < UNITS.length - 1) {
    v /= 1024;
    unit++;
  }
  const digits = unit === 0 ? 0 : v < 10 ? 2 : v < 100 ? 1 : 0;
  return `${v.toFixed(digits)} ${UNITS[unit]}`;
}

export function formatSpeed(bytesPerSec: number): string {
  return `${formatBytes(bytesPerSec)}/s`;
}

export function formatEta(seconds: number | null | undefined): string {
  if (seconds == null || !Number.isFinite(seconds)) return "";
  if (seconds < 5) return "a few seconds left";
  if (seconds < 60) return `${Math.round(seconds)} sec left`;
  const m = Math.round(seconds / 60);
  if (m < 60) return `${m} min left`;
  const h = Math.floor(seconds / 3600);
  const rm = Math.round((seconds % 3600) / 60);
  if (h < 24) return rm ? `${h} hr ${rm} min left` : `${h} hr left`;
  return `${Math.round(h / 24)} days left`;
}

export function formatAgo(ms: number, now = Date.now()): string {
  const s = Math.max(0, (now - ms) / 1000);
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} hr ago`;
  const d = Math.floor(s / 86400);
  if (d === 1) return "yesterday";
  if (d < 7) return `${d} days ago`;
  return new Date(ms).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" });
}

export function percent(done: number, total: number | null | undefined): number | null {
  if (!total) return null;
  return Math.min(100, (done / total) * 100);
}

export function hostOf(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

/** Extracts http(s) URLs from pasted or dropped text, one per line or whitespace-separated. */
export function extractUrls(text: string): string[] {
  const seen = new Set<string>();
  for (const token of text.split(/\s+/)) {
    const t = token.trim().replace(/^[<("']+|[>)"',.;]+$/g, "");
    if (!/^https?:\/\//i.test(t)) continue;
    try {
      seen.add(new URL(t).toString());
    } catch {
      /* not a URL */
    }
  }
  return [...seen];
}

export function nameFromUrl(url: string): string {
  try {
    const seg = new URL(url).pathname.split("/").filter(Boolean).pop();
    return seg ? decodeURIComponent(seg) : new URL(url).host;
  } catch {
    return url;
  }
}
