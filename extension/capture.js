// Decisions the extension makes, kept free of browser APIs so they can be tested.

/** Native messaging host name registered by the Downpour app. */
export const HOST = "cc.zenco.downpour";

/** Downloads known to be smaller than this stay in the browser; they finish faster there. */
export const MIN_BYTES = 256 * 1024;

/**
 * Whether a browser download should be handed to Downpour.
 * @param {{url?: string, finalUrl?: string, incognito?: boolean, byExtensionId?: string, totalBytes?: number}} item
 * @param {{enabled: boolean}} settings
 */
export function shouldCapture(item, settings) {
  if (!settings.enabled || item.incognito || item.byExtensionId) return false;
  if (!isWebUrl(item.finalUrl || item.url)) return false;
  if (item.totalBytes > 0 && item.totalBytes < MIN_BYTES) return false;
  return true;
}

/** Only http(s) links can be downloaded by the app; blob:, data: and file: stay in the browser. */
export function isWebUrl(url) {
  return typeof url === "string" && /^https?:\/\//i.test(url);
}

/** @param {{name: string, value: string}[]} cookies */
export function cookieHeader(cookies) {
  return cookies
    .filter((c) => c.name)
    .map((c) => `${c.name}=${c.value}`)
    .join("; ");
}

/** The file name from a path the browser suggested, on any platform. */
export function baseName(path) {
  const name = (path ?? "").split(/[\\/]/).pop();
  return name || undefined;
}

/** The message the native helper expects for one download. */
export function downloadMessage({ url, referrer, fileName, cookies = [], userAgent }) {
  const headers = {};
  const cookie = cookieHeader(cookies);
  if (cookie) headers.Cookie = cookie;
  if (userAgent) headers["User-Agent"] = userAgent;
  return { type: "download", url, referer: referrer || undefined, fileName: fileName || undefined, headers };
}
