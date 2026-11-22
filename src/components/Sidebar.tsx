import { ArrowDownToLine, CircleCheck, CircleExclamationPoint, CirclePause, Gauge, Layers, Settings as SettingsIcon } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { Download } from "../api";
import { formatSpeed } from "../format";
import { saveSettings, useStore } from "../store";
import { CATEGORIES, matches, sameView, type View } from "../views";
import { FileIcon } from "./FileIcon";
import { SpeedGraph } from "./SpeedGraph";

interface Props {
  view: View;
  onView: (v: View) => void;
  onSettings: () => void;
  isMac: boolean;
}

const LIMITS = [0, 256 * 1024, 1024 * 1024, 5 * 1024 * 1024, 10 * 1024 * 1024, 25 * 1024 * 1024];

export function Sidebar({ view, onView, onSettings, isMac }: Props) {
  const downloads = useStore((s) => s.downloads);
  const totalSpeed = useStore((s) => s.totalSpeed);
  const speedLimit = useStore((s) => s.settings?.speedLimit ?? 0);
  const list = useMemo(() => Object.values(downloads), [downloads]);
  const count = (v: View) => list.filter((d: Download) => matches(d, v)).length;

  const main: { view: View; label: string; icon: React.ReactNode }[] = [
    { view: { kind: "all" }, label: "All downloads", icon: <Layers size={17} /> },
    { view: { kind: "active" }, label: "Downloading", icon: <ArrowDownToLine size={17} /> },
    { view: { kind: "paused" }, label: "Paused", icon: <CirclePause size={17} /> },
    { view: { kind: "completed" }, label: "Finished", icon: <CircleCheck size={17} /> },
  ];
  const failed = count({ kind: "failed" });
  if (failed > 0 || view.kind === "failed") {
    main.push({ view: { kind: "failed" }, label: "Failed", icon: <CircleExclamationPoint size={17} /> });
  }
  const categories = CATEGORIES.map((c) => ({ ...c, n: count({ kind: "category", category: c.id }) })).filter(
    (c) => c.n > 0 || (view.kind === "category" && view.category === c.id),
  );

  return (
    <aside className={`sidebar${isMac ? " sidebar-mac" : ""}`}>
      <div className="sidebar-drag" data-tauri-drag-region />
      <nav aria-label="Downloads">
        <ul className="nav">
          {main.map((m) => (
            <NavItem key={m.view.kind} active={sameView(view, m.view)} onClick={() => onView(m.view)} icon={m.icon} label={m.label} n={count(m.view)} tone={m.view.kind === "failed" ? "danger" : undefined} />
          ))}
        </ul>
        {categories.length > 0 && (
          <>
            <h2 className="nav-heading">File types</h2>
            <ul className="nav">
              {categories.map((c) => {
                const v: View = { kind: "category", category: c.id };
                return (
                  <NavItem key={c.id} active={sameView(view, v)} onClick={() => onView(v)} icon={<FileIcon category={c.id} size={14} />} label={c.label} n={c.n} />
                );
              })}
            </ul>
          </>
        )}
      </nav>

      <div className="sidebar-foot">
        <div className="throughput">
          <div className="throughput-head">
            <span className="throughput-speed">{totalSpeed > 0 ? formatSpeed(totalSpeed) : "Idle"}</span>
            <SpeedLimit limit={speedLimit} />
          </div>
          <SpeedGraph />
        </div>
        <button className="nav-item settings-btn" onClick={onSettings}>
          <SettingsIcon size={17} />
          <span>Settings</span>
          <kbd>{isMac ? "⌘," : "Ctrl+,"}</kbd>
        </button>
      </div>
    </aside>
  );
}

function NavItem(p: { active: boolean; onClick: () => void; icon: React.ReactNode; label: string; n: number; tone?: "danger" }) {
  return (
    <li>
      <button className={`nav-item${p.active ? " is-active" : ""}${p.tone ? ` tone-${p.tone}` : ""}`} onClick={p.onClick} aria-current={p.active ? "page" : undefined}>
        {p.icon}
        <span>{p.label}</span>
        {p.n > 0 && <span className="nav-count">{p.n}</span>}
      </button>
    </li>
  );
}

function SpeedLimit({ limit }: { limit: number }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && setOpen(false);
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", esc);
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("keydown", esc);
    };
  }, [open]);

  return (
    <div className="limit" ref={ref}>
      <button className={`limit-btn${limit ? " is-limited" : ""}`} onClick={() => setOpen(!open)} aria-expanded={open} title="Speed limit">
        <Gauge size={14} />
        {limit ? `Max ${formatSpeed(limit)}` : "No limit"}
      </button>
      {open && (
        <ul className="menu limit-menu" role="menu">
          {LIMITS.map((l) => (
            <li key={l}>
              <button
                role="menuitemradio"
                aria-checked={l === limit}
                className={l === limit ? "is-checked" : ""}
                onClick={() => {
                  saveSettings({ speedLimit: l });
                  setOpen(false);
                }}
              >
                {l === 0 ? "No limit" : formatSpeed(l)}
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
