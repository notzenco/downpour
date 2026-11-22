import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import manifest from "./manifest.json";
import { HOST, MIN_BYTES, baseName, cookieHeader, downloadMessage, shouldCapture } from "./capture.js";

const on = { enabled: true };
const item = { url: "https://example.com/big.iso", totalBytes: 0 };

describe("shouldCapture", () => {
  it("hands ordinary web downloads to the app", () => {
    expect(shouldCapture(item, on)).toBe(true);
    expect(shouldCapture({ ...item, totalBytes: MIN_BYTES }, on)).toBe(true);
  });

  it("leaves everything else to the browser", () => {
    expect(shouldCapture(item, { enabled: false })).toBe(false);
    expect(shouldCapture({ ...item, incognito: true }, on)).toBe(false);
    expect(shouldCapture({ ...item, byExtensionId: "other" }, on)).toBe(false);
    expect(shouldCapture({ ...item, url: "blob:https://example.com/1" }, on)).toBe(false);
    expect(shouldCapture({ ...item, url: "data:text/plain,hi" }, on)).toBe(false);
    expect(shouldCapture({ ...item, totalBytes: 1024 }, on)).toBe(false);
  });

  it("judges the final address after redirects", () => {
    expect(shouldCapture({ url: "https://example.com/go", finalUrl: "blob:x" }, on)).toBe(false);
  });
});

describe("downloadMessage", () => {
  it("carries cookies and the browser's user agent as headers", () => {
    const message = downloadMessage({
      url: "https://example.com/a.zip",
      referrer: "https://example.com/page",
      fileName: "a.zip",
      cookies: [
        { name: "session", value: "abc" },
        { name: "", value: "ignored" },
        { name: "theme", value: "dark" },
      ],
      userAgent: "Mozilla/5.0",
    });
    expect(message).toEqual({
      type: "download",
      url: "https://example.com/a.zip",
      referer: "https://example.com/page",
      fileName: "a.zip",
      headers: { Cookie: "session=abc; theme=dark", "User-Agent": "Mozilla/5.0" },
    });
  });

  it("omits what the browser did not provide", () => {
    expect(JSON.parse(JSON.stringify(downloadMessage({ url: "https://x.org/f" })))).toEqual({
      type: "download",
      url: "https://x.org/f",
      headers: {},
    });
    expect(cookieHeader([])).toBe("");
  });
});

describe("baseName", () => {
  it("takes the last path part on any platform", () => {
    expect(baseName("/Users/me/Downloads/a.zip")).toBe("a.zip");
    expect(baseName("C:\\Users\\me\\a.zip")).toBe("a.zip");
    expect(baseName("")).toBeUndefined();
  });
});

describe("manifest", () => {
  it("matches the native host the app registers", () => {
    expect(HOST).toBe("cc.zenco.downpour");
    expect(manifest.permissions).toContain("nativeMessaging");
    expect(manifest.browser_specific_settings.gecko.id).toBe("downpour@zenco.cc");
  });

  it("pins the Chrome extension ID the app's helper allows", () => {
    // Chrome derives the ID from the first 128 bits of the key's SHA-256, written with the letters a-p.
    const hex = createHash("sha256").update(Buffer.from(manifest.key, "base64")).digest("hex").slice(0, 32);
    const id = [...hex].map((c) => String.fromCharCode(97 + parseInt(c, 16))).join("");
    const rust = readFileSync(new URL("../src-tauri/src/browser.rs", import.meta.url), "utf8");
    expect(rust).toContain(`CHROME_EXTENSION_ID: &str = "${id}"`);
    expect(rust).toContain(`FIREFOX_EXTENSION_ID: &str = "${manifest.browser_specific_settings.gecko.id}"`);
  });
});
