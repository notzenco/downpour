# Testing

## Automated tests

`pnpm check` runs everything CI runs, on Windows, macOS and Linux:

| Suite | What it covers |
| --- | --- |
| Engine end-to-end (Rust) | Real downloads against an in-process HTTP server that simulates range support, no ranges, unknown length, missing files, a per-client connection cap and a cookie-protected file: segmented downloads, pause and resume from saved offsets, recovery after a restart, queue limits, file-name collisions, adapting to refused connections, custom headers on every request |
| Engine units (Rust) | Range planning and splitting, the speed limiter, `Content-Disposition` parsing, file-name sanitising, header filtering |
| IPC contract (Rust) | Every command called through Tauri's mock runtime with the exact JSON `src/api.ts` sends |
| Browser helper (Rust) | Native messaging framing, rejecting non-web links, parsing handed-over downloads, registering only with installed browsers |
| Interface and extension (Vitest) | Formatting helpers, the extension's capture rules and message format, and that the extension's key produces the ID the helper allows |

`cargo fmt --check` and Clippy with warnings as errors also run in CI.

## Real-network benchmark

Skipped by default because it downloads from the internet:

```sh
cd src-tauri
DOWNPOUR_BENCH_URL=<large file URL> DOWNPOUR_BENCH_CONNECTIONS=8 \
  cargo test --locked bench_real_url -- --ignored --nocapture
```

## End-to-end checks

Run by hand on macOS (Apple silicon) in November 2026 against the release
build.

| Check | Result |
| --- | --- |
| Download the 141 MB Linux 6.12 tarball from cdn.kernel.org | SHA-256 matches kernel.org's published checksum; 52 MB/s with 8 connections against 2.4 MB/s for single-connection curl |
| Force-kill the app at 44.7 MB, then relaunch | Resumed from the saved offset, not from zero; checksum matches |
| Server that refuses more than about two connections (HTTP 429) | Completes with fewer connections instead of failing |
| Extension in Brave (Chromium 158), download started by the browser | Browser cancelled its copy before saving anything; Downpour downloaded over 5 parallel ranges; checksum matches |
| Extension with a file that requires a session cookie | The cookie was sent on Downpour's size check and on every range request |
| Extension when the browser starts with a download link | Caught |
| Native helper launched the way a browser launches it | Answers a ping, refuses `file://` links, opens the add dialog for web links |
| v0.1.0 release assets | The Apple silicon DMG mounts with a native arm64 app; the extension zip contains only the extension's files |

## Repeating the browser extension check

Use a throwaway browser profile so your own profile is untouched.

1. Build and start the app (`pnpm tauri build`, then open the built app). On
   launch it registers its native messaging helper with installed browsers.
2. Serve a file of at least 256 KB that a browser will download, for example
   with `Content-Disposition: attachment`.
3. Create a profile folder. Copy the helper manifest the app wrote (for Brave on
   macOS, `~/Library/Application Support/BraveSoftware/Brave-Browser/NativeMessagingHosts/cc.zenco.downpour.json`)
   into `<profile>/NativeMessagingHosts/`, because Chromium looks there when it
   runs with `--user-data-dir`. To keep the browser's own copy out of your
   Downloads folder if capture fails, set `download.default_directory` in
   `<profile>/Default/Preferences`.
4. Launch the browser with the extension and open the file's URL:

   ```sh
   open -na "Brave Browser" --args --user-data-dir=<profile> \
     --load-extension="$PWD/extension" \
     --disable-features=DisableLoadExtensionCommandLineSwitch \
     --no-first-run <file URL>
   ```

   On Linux, run the browser binary with the same arguments.
5. The download should appear in Downpour (or its add dialog, depending on the
   **Ask before downloading links from the browser** setting), and the
   browser's download folder should stay empty.

## Not yet verified

- The extension running in Firefox, which uses a different path (pause the
  download, then hand it over).
- The right-click menu item and the toolbar on/off button, clicked by hand.
- The app running on Windows and Linux. CI builds and tests it there, but no
  one has opened it, so the registry and folder registration of the browser
  helper has not run for real.
- The released Windows and Linux installers.
- Chrome, Edge and Vivaldi. They run the same extension code as Brave.

## Known limitations

- A download that starts while the extension is first being installed stays in
  the browser.
- Installers are not code-signed or notarized, so macOS and Windows warn on
  first launch.
- Firefox keeps the unsigned extension only until it restarts.
