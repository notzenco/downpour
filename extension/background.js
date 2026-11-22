import { HOST, baseName, downloadMessage, isWebUrl, shouldCapture } from "./capture.js";

const MENU_ID = "downpour-download";

async function settings() {
  const { enabled = true } = await chrome.storage.local.get("enabled");
  return { enabled };
}

/** Sends one download to the app with the site's cookies. Resolves true if the app accepted it. */
async function send({ url, referrer, fileName, storeId }) {
  const cookies = await chrome.cookies.getAll(storeId ? { url, storeId } : { url }).catch(() => []);
  const message = downloadMessage({ url, referrer, fileName, cookies, userAgent: navigator.userAgent });
  try {
    const reply = await chrome.runtime.sendNativeMessage(HOST, message);
    return reply?.ok === true;
  } catch {
    return false;
  }
}

async function updateBadge() {
  const { enabled } = await settings();
  await chrome.action.setBadgeBackgroundColor({ color: "#6f7d8a" });
  await chrome.action.setBadgeText({ text: enabled ? "" : "off" });
  await chrome.action.setTitle({
    title: enabled
      ? "Downpour is catching downloads. Click to let the browser handle them."
      : "The browser is handling downloads. Click to send them to Downpour.",
  });
}

async function showUnavailable() {
  await chrome.action.setBadgeBackgroundColor({ color: "#c2413a" });
  await chrome.action.setBadgeText({ text: "!" });
  await chrome.action.setTitle({ title: "Downpour isn't installed or couldn't start, so the browser kept this download." });
  setTimeout(updateBadge, 8000);
}

async function handOver(item) {
  return send({ url: item.finalUrl || item.url, referrer: item.referrer, fileName: baseName(item.filename) });
}

if (chrome.downloads.onDeterminingFilename) {
  // Chromium waits for this listener before writing anything, so nothing is downloaded twice.
  chrome.downloads.onDeterminingFilename.addListener((item, suggest) => {
    (async () => {
      if (!shouldCapture(item, await settings())) return suggest();
      if (await handOver(item)) {
        await chrome.downloads.cancel(item.id).catch(() => {});
        await chrome.downloads.erase({ id: item.id }).catch(() => {});
      } else {
        suggest();
        showUnavailable();
      }
    })();
    return true;
  });
} else {
  // Firefox has no such hook: pause the download as soon as it appears.
  chrome.downloads.onCreated.addListener(async (item) => {
    if (!shouldCapture(item, await settings())) return;
    await chrome.downloads.pause(item.id).catch(() => {});
    if (await handOver(item)) {
      await chrome.downloads.cancel(item.id).catch(() => {});
      await chrome.downloads.erase({ id: item.id }).catch(() => {});
    } else {
      await chrome.downloads.resume(item.id).catch(() => {});
      showUnavailable();
    }
  });
}

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  const url = info.linkUrl || info.srcUrl;
  if (info.menuItemId !== MENU_ID || !isWebUrl(url)) return;
  const ok = await send({ url, referrer: info.pageUrl, storeId: tab?.cookieStoreId });
  if (!ok) showUnavailable();
});

chrome.action.onClicked.addListener(async () => {
  const { enabled } = await settings();
  await chrome.storage.local.set({ enabled: !enabled });
  await updateBadge();
});

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({
    id: MENU_ID,
    title: "Download with Downpour",
    contexts: ["link", "video", "audio", "image"],
  });
  updateBadge();
});

chrome.runtime.onStartup.addListener(updateBadge);
