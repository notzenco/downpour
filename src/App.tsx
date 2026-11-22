import { readText } from "@tauri-apps/plugin-clipboard-manager";
import { ArrowDownToLine, Pause, Play, Plus, Search, Trash, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, errorText, type Download } from "./api";
import { AddDialog } from "./components/AddDialog";
import { DetailPanel } from "./components/DetailPanel";
import { DownloadRow } from "./components/DownloadRow";
import { RemoveDialog } from "./components/RemoveDialog";
import { SettingsDialog } from "./components/SettingsDialog";
import { Sidebar } from "./components/Sidebar";
import { extractUrls, nameFromUrl } from "./format";
import { getState, useStore } from "./store";
import { CATEGORIES, matches, sortDownloads, type View } from "./views";

const isMac = navigator.userAgent.includes("Mac");
const mod = (e: KeyboardEvent | React.KeyboardEvent) => (isMac ? e.metaKey : e.ctrlKey);

interface Toast {
  id: number;
  text: string;
  action?: { label: string; run: () => void };
}

type Overlay = { kind: "add"; text?: string } | { kind: "settings" } | { kind: "remove"; ids: string[] } | null;

const VIEW_TITLE: Record<Exclude<View["kind"], "category">, string> = {
  all: "All downloads",
  active: "Downloading",
  paused: "Paused",
  completed: "Finished",
  failed: "Failed",
};

const EMPTY_TEXT: Record<View["kind"], string> = {
  all: "",
  active: "Nothing is downloading right now.",
  paused: "Nothing is paused.",
  completed: "Finished downloads appear here.",
  failed: "No failed downloads.",
  category: "No downloads of this type yet.",
};

const isRunning = (d: Download) => d.status === "downloading" || d.status === "connecting" || d.status === "queued";

export default function App() {
  const downloads = useStore((s) => s.downloads);
  const live = useStore((s) => s.live);
  const ready = useStore((s) => s.ready);
  const theme = useStore((s) => s.settings?.theme ?? "system");
  const [view, setView] = useState<View>({ kind: "all" });
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const anchor = useRef<string | null>(null);
  const [overlay, setOverlay] = useState<Overlay>(null);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [dragging, setDragging] = useState(false);
  const searchRef = useRef<HTMLInputElement>(null);
  const lastClipboard = useRef<string | null>(null);

  const all = useMemo(() => Object.values(downloads), [downloads]);
  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    return sortDownloads(all.filter((d) => matches(d, view) && (!q || d.fileName.toLowerCase().includes(q) || d.url.toLowerCase().includes(q))));
  }, [all, view, query]);
  const queueIndex = useMemo(() => {
    const queued = all.filter((d) => d.status === "queued").sort((a, b) => a.queuePosition - b.queuePosition);
    return new Map(queued.map((d, i) => [d.id, i + 1]));
  }, [all]);

  const selectedList = useMemo(() => visible.filter((d) => selected.has(d.id)), [visible, selected]);
  const focused = selectedList.length === 1 ? selectedList[0] : null;

  useEffect(() => {
    const root = document.documentElement;
    if (theme === "system") delete root.dataset.theme;
    else root.dataset.theme = theme;
  }, [theme]);

  // Drop selections that disappear (removed, or filtered out by view/search).
  useEffect(() => {
    setSelected((prev) => {
      const ids = new Set(visible.map((d) => d.id));
      const next = new Set([...prev].filter((id) => ids.has(id)));
      return next.size === prev.size ? prev : next;
    });
  }, [visible]);

  const toast = useCallback((text: string, action?: Toast["action"], ms = 4000) => {
    const id = Date.now() + Math.random();
    setToasts((t) => [...t.slice(-2), { id, text, action }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), ms);
  }, []);

  const run = useCallback((p: Promise<unknown>) => p.catch((e) => toast(errorText(e))), [toast]);

  const toggle = useCallback(
    (d: Download) => run(isRunning(d) ? api.pause([d.id]) : api.resume([d.id])),
    [run],
  );
  const openFile = useCallback((d: Download) => run(api.open(d.id)), [run]);
  const reveal = useCallback((d: Download) => run(api.reveal(d.id)), [run]);
  const askRemove = useCallback((d: Download) => setOverlay({ kind: "remove", ids: [d.id] }), []);

  const onSelect = useCallback(
    (id: string, e: React.MouseEvent) => {
      if (e.button !== 0) return;
      const additive = isMac ? e.metaKey : e.ctrlKey;
      if (e.shiftKey && anchor.current) {
        const ids = visible.map((d) => d.id);
        const [a, b] = [ids.indexOf(anchor.current), ids.indexOf(id)].sort((x, y) => x - y);
        if (a >= 0) {
          setSelected(new Set(ids.slice(a, b + 1)));
          return;
        }
      }
      anchor.current = id;
      setSelected((prev) => {
        if (!additive) return new Set([id]);
        const next = new Set(prev);
        if (next.has(id)) next.delete(id);
        else next.add(id);
        return next;
      });
    },
    [visible],
  );

  const offerAdd = useCallback((text: string) => {
    if (extractUrls(text).length > 0) setOverlay({ kind: "add", text });
  }, []);

  // Keyboard shortcuts for the list. Inputs and open dialogs keep their own keys.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const typing = e.target instanceof Element && e.target.closest("input, textarea, select, [contenteditable]");
      if (mod(e) && e.key.toLowerCase() === "n") {
        e.preventDefault();
        setOverlay({ kind: "add" });
        return;
      }
      if (mod(e) && e.key === ",") {
        e.preventDefault();
        setOverlay({ kind: "settings" });
        return;
      }
      if (overlay || typing) return;
      if (mod(e) && e.key.toLowerCase() === "f") {
        e.preventDefault();
        searchRef.current?.focus();
      } else if (mod(e) && e.key.toLowerCase() === "a") {
        e.preventDefault();
        setSelected(new Set(visible.map((d) => d.id)));
      } else if (e.key === "Escape") {
        setSelected(new Set());
      } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        if (!visible.length) return;
        const ids = visible.map((d) => d.id);
        const current = anchor.current ? ids.indexOf(anchor.current) : -1;
        const next = Math.min(ids.length - 1, Math.max(0, current + (e.key === "ArrowDown" ? 1 : -1)));
        anchor.current = ids[next];
        setSelected((prev) => (e.shiftKey ? new Set([...prev, ids[next]]) : new Set([ids[next]])));
        document.querySelector(`[data-row="${ids[next]}"]`)?.scrollIntoView({ block: "nearest" });
      } else if (e.key === " " && selectedList.length) {
        e.preventDefault();
        const anyRunning = selectedList.some(isRunning);
        const ids = selectedList.filter((d) => d.status !== "completed").map((d) => d.id);
        run(anyRunning ? api.pause(ids) : api.resume(ids));
      } else if ((e.key === "Delete" || e.key === "Backspace") && selectedList.length) {
        e.preventDefault();
        setOverlay({ kind: "remove", ids: selectedList.map((d) => d.id) });
      } else if (e.key === "Enter" && focused) {
        if (focused.status === "completed") openFile(focused);
        else toggle(focused);
      }
    };
    const onPaste = (e: ClipboardEvent) => {
      if (overlay || (e.target instanceof Element && e.target.closest("input, textarea"))) return;
      const text = e.clipboardData?.getData("text") ?? "";
      if (extractUrls(text).length) {
        e.preventDefault();
        offerAdd(text);
      }
    };
    window.addEventListener("keydown", onKey);
    document.addEventListener("paste", onPaste);
    return () => {
      window.removeEventListener("keydown", onKey);
      document.removeEventListener("paste", onPaste);
    };
  }, [overlay, visible, selectedList, focused, run, openFile, toggle, offerAdd]);

  // Offer copied links when the user switches back to the app.
  useEffect(() => {
    const onFocus = async () => {
      const s = getState();
      if (!s.settings?.watchClipboard || overlay) return;
      let text = "";
      try {
        text = (await readText()) ?? "";
      } catch {
        return;
      }
      const url = extractUrls(text)[0];
      if (!url || url === lastClipboard.current) return;
      lastClipboard.current = url;
      if (Object.values(s.downloads).some((d) => d.url === url)) return;
      toast(`Copied link: ${nameFromUrl(url)}`, { label: "Download", run: () => setOverlay({ kind: "add", text: url }) }, 8000);
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [overlay, toast]);

  const title = view.kind === "category" ? (CATEGORIES.find((c) => c.id === view.category)?.label ?? "") : VIEW_TITLE[view.kind];
  const anyRunning = all.some(isRunning);
  const anyResumable = all.some((d) => d.status === "paused" || d.status === "failed");
  const selRunning = selectedList.some(isRunning);
  const selIds = selectedList.filter((d) => d.status !== "completed").map((d) => d.id);

  if (!ready) return <div className="app app-loading" />;

  return (
    <div
      className="app"
      onDragOver={(e) => {
        if (e.dataTransfer.types.some((t) => t === "text/uri-list" || t === "text/plain")) {
          e.preventDefault();
          setDragging(true);
        }
      }}
      onDragLeave={(e) => e.currentTarget === e.target && setDragging(false)}
      onDrop={(e) => {
        e.preventDefault();
        setDragging(false);
        offerAdd(e.dataTransfer.getData("text/uri-list") || e.dataTransfer.getData("text/plain"));
      }}
    >
      <Sidebar view={view} onView={(v) => { setView(v); setSelected(new Set()); }} onSettings={() => setOverlay({ kind: "settings" })} isMac={isMac} />

      <main className="main">
        <header className="toolbar" data-tauri-drag-region>
          <h1 data-tauri-drag-region>{title}</h1>
          <div className="search">
            <Search size={15} aria-hidden />
            <input ref={searchRef} value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search" aria-label="Search downloads" onKeyDown={(e) => e.key === "Escape" && (setQuery(""), e.currentTarget.blur())} />
            {query && (
              <button className="icon-btn" onClick={() => setQuery("")} aria-label="Clear search">
                <X size={14} />
              </button>
            )}
          </div>
          <div className="toolbar-actions">
            {selectedList.length > 1 ? (
              <>
                <span className="selection-count">{selectedList.length} selected</span>
                {selIds.length > 0 && (
                  <button className="btn" onClick={() => run(selRunning ? api.pause(selIds) : api.resume(selIds))}>
                    {selRunning ? <Pause size={15} /> : <Play size={15} />} {selRunning ? "Pause" : "Resume"}
                  </button>
                )}
                <button className="btn" onClick={() => setOverlay({ kind: "remove", ids: selectedList.map((d) => d.id) })}>
                  <Trash size={15} /> Remove
                </button>
              </>
            ) : anyRunning ? (
              <button className="btn" onClick={() => run(api.pause(all.filter(isRunning).map((d) => d.id)))}>
                <Pause size={15} /> Pause all
              </button>
            ) : anyResumable ? (
              <button className="btn" onClick={() => run(api.resume(all.filter((d) => d.status === "paused" || d.status === "failed").map((d) => d.id)))}>
                <Play size={15} /> Resume all
              </button>
            ) : null}
            <button className="btn btn-primary" onClick={() => setOverlay({ kind: "add" })} title={isMac ? "Add download (⌘N)" : "Add download (Ctrl+N)"}>
              <Plus size={16} /> Add
            </button>
          </div>
        </header>

        <div className="content">
          {visible.length > 0 ? (
            <ul className="list" role="listbox" aria-label={title} aria-multiselectable>
              {visible.map((d) => (
                  <DownloadRow
                    key={d.id}
                    download={d}
                    live={live[d.id]}
                    selected={selected.has(d.id)}
                    queueIndex={queueIndex.get(d.id) ?? null}
                    onSelect={onSelect}
                    onToggle={toggle}
                    onOpen={openFile}
                    onReveal={reveal}
                    onRemove={askRemove}
                  />
              ))}
            </ul>
          ) : all.length === 0 ? (
            <div className="empty empty-first">
              <div className="empty-mark" aria-hidden>
                <ArrowDownToLine size={28} strokeWidth={1.5} />
              </div>
              <h2>Paste a link to start downloading</h2>
              <p>
                Press {isMac ? "⌘V" : "Ctrl+V"} anywhere in this window, or drag a link here from your browser. Large files are split across connections, and
                every download can be paused and resumed.
              </p>
              <button className="btn btn-primary" onClick={() => setOverlay({ kind: "add" })}>
                <Plus size={16} /> Add download
              </button>
            </div>
          ) : (
            <div className="empty">
              <p>{query ? `Nothing matches “${query}”.` : EMPTY_TEXT[view.kind]}</p>
            </div>
          )}

          {focused && (
            <DetailPanel
              key={focused.id}
              download={focused}
              live={live[focused.id]}
              onClose={() => setSelected(new Set())}
              onToggle={toggle}
              onOpen={openFile}
              onReveal={reveal}
              onRemove={askRemove}
              onToast={toast}
            />
          )}
        </div>
      </main>

      {dragging && (
        <div className="drop-overlay" aria-hidden>
          <div>Drop the link to download it</div>
        </div>
      )}

      <div className="toasts" role="status" aria-live="polite">
        {toasts.map((t) => (
          <div key={t.id} className="toast">
            <span>{t.text}</span>
            {t.action && (
              <button
                className="toast-action"
                onClick={() => {
                  t.action!.run();
                  setToasts((all) => all.filter((x) => x.id !== t.id));
                }}
              >
                {t.action.label}
              </button>
            )}
          </div>
        ))}
      </div>

      {overlay?.kind === "add" && (
        <AddDialog
          initialText={overlay.text}
          onClose={() => setOverlay(null)}
          onAdded={(ds) => {
            setOverlay(null);
            setSelected(new Set(ds.length === 1 ? [ds[0].id] : []));
            if (ds.length > 1) toast(`Added ${ds.length} downloads`);
          }}
        />
      )}
      {overlay?.kind === "settings" && <SettingsDialog onClose={() => setOverlay(null)} />}
      {overlay?.kind === "remove" && (
        <RemoveDialog
          downloads={all.filter((d) => overlay.ids.includes(d.id))}
          onCancel={() => setOverlay(null)}
          onConfirm={(deleteFiles) => {
            run(api.remove(overlay.ids, deleteFiles));
            setOverlay(null);
          }}
        />
      )}
    </div>
  );
}
