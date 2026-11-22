import { describe, expect, it } from "vitest";
import { extractUrls, formatBytes, formatEta, nameFromUrl } from "./format";

describe("formatBytes", () => {
  it("scales with sensible precision", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1536)).toBe("1.50 KB");
    expect(formatBytes(50 * 1024 * 1024)).toBe("50.0 MB");
    expect(formatBytes(4.7 * 1024 ** 3)).toBe("4.70 GB");
    expect(formatBytes(null)).toBe("Unknown size");
  });
});

describe("formatEta", () => {
  it("reads naturally", () => {
    expect(formatEta(3)).toBe("a few seconds left");
    expect(formatEta(42)).toBe("42 sec left");
    expect(formatEta(600)).toBe("10 min left");
    expect(formatEta(3600 + 1200)).toBe("1 hr 20 min left");
    expect(formatEta(null)).toBe("");
  });
});

describe("extractUrls", () => {
  it("pulls unique http links out of messy text", () => {
    const text = "grab <https://a.com/x.zip>, and http://b.org/y.iso.\nftp://no.pe https://a.com/x.zip";
    expect(extractUrls(text)).toEqual(["https://a.com/x.zip", "http://b.org/y.iso"]);
    expect(extractUrls("no links here")).toEqual([]);
  });
});

describe("nameFromUrl", () => {
  it("decodes the last path segment", () => {
    expect(nameFromUrl("https://x.org/a/My%20File.iso?q=1")).toBe("My File.iso");
    expect(nameFromUrl("https://x.org/")).toBe("x.org");
  });
});
