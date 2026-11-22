import { open as pickFolder } from "@tauri-apps/plugin-dialog";
import { FolderInput } from "lucide-react";
import { useEffect, useState } from "react";
import { api, errorText, type Settings } from "../api";
import { saveSettings, useStore } from "../store";
import { Modal } from "./Modal";

const MB = 1024 * 1024;
const listFormat = new Intl.ListFormat(undefined, { type: "conjunction" });

export function SettingsDialog({ onClose }: { onClose: () => void }) {
  const settings = useStore((s) => s.settings)!;
  const [error, setError] = useState<string | null>(null);
  const [browsers, setBrowsers] = useState<string[] | null>(null);

  useEffect(() => {
    api.browserStatus().then(setBrowsers, () => setBrowsers([]));
  }, []);
  const [limitText, setLimitText] = useState(settings.speedLimit ? String(+(settings.speedLimit / MB).toFixed(2)) : "");

  const update = (patch: Partial<Settings>) => {
    setError(null);
    saveSettings(patch).catch((e) => setError(errorText(e)));
  };

  const chooseFolder = async () => {
    const picked = await pickFolder({ directory: true, defaultPath: settings.downloadDir });
    if (typeof picked === "string") update({ downloadDir: picked });
  };

  return (
    <Modal title="Settings" onClose={onClose} className="settings-dialog">
      <div className="settings">
        <section className="settings-group">
          <h3>Saving files</h3>
          <div className="setting">
            <div className="setting-text">
              <span>Download folder</span>
            </div>
            <button type="button" className="folder-btn" onClick={chooseFolder} title={settings.downloadDir}>
              <FolderInput size={15} />
              <span className="folder-path">{"\u200e" + settings.downloadDir}</span>
              <span className="folder-change">Change</span>
            </button>
          </div>
          <Toggle
            label="Sort into folders by type"
            hint="Puts videos in Video, archives in Compressed, and so on, inside the download folder."
            checked={settings.categoryFolders}
            onChange={(v) => update({ categoryFolders: v })}
          />
          <Toggle label="Notify when a download finishes" checked={settings.notifyOnComplete} onChange={(v) => update({ notifyOnComplete: v })} />
        </section>

        <section className="settings-group">
          <h3>Speed</h3>
          <Stepper
            label="Connections per download"
            hint="More connections are usually faster. Some servers limit how many they allow."
            value={settings.connections}
            min={1}
            max={32}
            onChange={(v) => update({ connections: v })}
          />
          <Stepper label="Downloads at the same time" hint="Others wait in the queue." value={settings.maxConcurrent} min={1} max={20} onChange={(v) => update({ maxConcurrent: v })} />
          <div className="setting">
            <div className="setting-text">
              <span>Speed limit</span>
              <span className="setting-hint">Total across all downloads. Leave empty for no limit.</span>
            </div>
            <label className="unit-input">
              <input
                inputMode="decimal"
                value={limitText}
                placeholder="No limit"
                aria-label="Speed limit in megabytes per second"
                onChange={(e) => setLimitText(e.target.value)}
                onBlur={() => {
                  const n = parseFloat(limitText);
                  update({ speedLimit: Number.isFinite(n) && n > 0 ? Math.round(n * MB) : 0 });
                  if (!(n > 0)) setLimitText("");
                }}
              />
              <span>MB/s</span>
            </label>
          </div>
          <Stepper label="Retries per connection" hint="How often a dropped connection is retried before the download fails." value={settings.maxRetries} min={0} max={50} onChange={(v) => update({ maxRetries: v })} />
        </section>

        <section className="settings-group">
          <h3>Behaviour</h3>
          <Toggle
            label="Keep running when the window is closed"
            hint="Downloads continue from the menu bar or system tray."
            checked={settings.closeToTray}
            onChange={(v) => update({ closeToTray: v })}
          />
          <Toggle
            label="Offer to download copied links"
            hint="When you switch to Downpour with a link on the clipboard."
            checked={settings.watchClipboard}
            onChange={(v) => update({ watchClipboard: v })}
          />
          <div className="setting">
            <div className="setting-text">
              <span>Appearance</span>
            </div>
            <div className="segmented" role="radiogroup" aria-label="Appearance">
              {(["system", "light", "dark"] as const).map((t) => (
                <button key={t} role="radio" aria-checked={settings.theme === t} className={settings.theme === t ? "is-on" : ""} onClick={() => update({ theme: t })}>
                  {t === "system" ? "Match system" : t === "light" ? "Light" : "Dark"}
                </button>
              ))}
            </div>
          </div>
        </section>

        <section className="settings-group">
          <h3>Browser</h3>
          <div className="setting">
            <div className="setting-text">
              <span>Browser extension</span>
              <span className="setting-hint">
                {browsers === null
                  ? "Checking installed browsers…"
                  : browsers.length
                    ? `Ready for ${listFormat.format(browsers)}. Add the extension to send downloads and links here.`
                    : "No supported browser found. The extension works with Chrome, Edge, Brave, Vivaldi and Firefox."}
              </span>
            </div>
            <button type="button" className="btn" onClick={() => api.openExtensionHelp().catch((e) => setError(errorText(e)))}>
              Get the extension
            </button>
          </div>
          <Toggle
            label="Ask before downloading links from the browser"
            hint="When off, links the extension sends start downloading straight away."
            checked={settings.confirmBrowserDownloads}
            onChange={(v) => update({ confirmBrowserDownloads: v })}
          />
        </section>

        <section className="settings-group">
          <h3>Advanced</h3>
          <label className="setting setting-stack">
            <div className="setting-text">
              <span>User agent</span>
              <span className="setting-hint">Sent to servers with every request. Some sites only serve files to browsers.</span>
            </div>
            <input className="text-input" defaultValue={settings.userAgent} spellCheck={false} onBlur={(e) => e.target.value !== settings.userAgent && update({ userAgent: e.target.value })} />
          </label>
        </section>
        {error && <p className="form-error">{error}</p>}
      </div>
    </Modal>
  );
}

function Toggle(p: { label: string; hint?: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="setting">
      <div className="setting-text">
        <span>{p.label}</span>
        {p.hint && <span className="setting-hint">{p.hint}</span>}
      </div>
      <input type="checkbox" role="switch" className="switch" checked={p.checked} onChange={(e) => p.onChange(e.target.checked)} />
    </label>
  );
}

function Stepper(p: { label: string; hint?: string; value: number; min: number; max: number; onChange: (v: number) => void }) {
  const set = (v: number) => p.onChange(Math.min(p.max, Math.max(p.min, v)));
  return (
    <div className="setting">
      <div className="setting-text">
        <span>{p.label}</span>
        {p.hint && <span className="setting-hint">{p.hint}</span>}
      </div>
      <div className="stepper">
        <button type="button" onClick={() => set(p.value - 1)} disabled={p.value <= p.min} aria-label={`Decrease ${p.label}`}>
          −
        </button>
        <span aria-live="polite">{p.value}</span>
        <button type="button" onClick={() => set(p.value + 1)} disabled={p.value >= p.max} aria-label={`Increase ${p.label}`}>
          +
        </button>
      </div>
    </div>
  );
}
