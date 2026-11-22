import { readText } from "@tauri-apps/plugin-clipboard-manager";
import { open as pickFolder } from "@tauri-apps/plugin-dialog";
import { Check, FolderInput, LoaderCircle, TriangleExclamationPoint } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { api, errorText, type Category, type Download, type ProbeInfo } from "../api";
import { extractUrls, formatBytes, hostOf, nameFromUrl } from "../format";
import { getState, useStore } from "../store";
import { FileIcon } from "./FileIcon";
import { Modal } from "./Modal";

const FOLDER: Record<Category, string> = {
  compressed: "Compressed",
  documents: "Documents",
  music: "Music",
  video: "Video",
  programs: "Programs",
  images: "Images",
  other: "Other",
};

type ProbeState = { kind: "idle" } | { kind: "loading"; url: string } | { kind: "ok"; info: ProbeInfo } | { kind: "error"; url: string; message: string };

interface Props {
  initialText?: string;
  onClose: () => void;
  onAdded: (downloads: Download[]) => void;
}

export function AddDialog({ initialText, onClose, onAdded }: Props) {
  const settings = useStore((s) => s.settings)!;
  const [text, setText] = useState(initialText ?? "");
  const [probe, setProbe] = useState<ProbeState>({ kind: "idle" });
  const [fileName, setFileName] = useState("");
  const nameEdited = useRef(false);
  const [saveDir, setSaveDir] = useState<string | null>(null);
  const [connections, setConnections] = useState(settings.connections);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  const urls = useMemo(() => extractUrls(text), [text]);
  const single = urls.length === 1 ? urls[0] : null;

  useEffect(() => {
    if (initialText) return;
    readText()
      .then((clip) => {
        if (extractUrls(clip ?? "").length > 0) {
          setText(clip.trim());
          requestAnimationFrame(() => inputRef.current?.select());
        }
      })
      .catch(() => {});
  }, [initialText]);

  useEffect(() => {
    if (!single) {
      setProbe({ kind: "idle" });
      return;
    }
    let stale = false;
    const t = setTimeout(() => {
      setProbe({ kind: "loading", url: single });
      api
        .probe(single)
        .then((info) => {
          if (stale) return;
          setProbe({ kind: "ok", info });
          if (!nameEdited.current) setFileName(info.fileName);
        })
        .catch((e) => !stale && setProbe({ kind: "error", url: single, message: errorText(e) }));
    }, 350);
    return () => {
      stale = true;
      clearTimeout(t);
    };
  }, [single]);

  useEffect(() => {
    if (single && !nameEdited.current) setFileName(nameFromUrl(single));
  }, [single]);

  const info = probe.kind === "ok" ? probe.info : null;
  const sep = navigator.userAgent.includes("Windows") ? "\\" : "/";
  const category: Category = info?.category ?? "other";
  const autoDir = settings.categoryFolders && info ? `${settings.downloadDir}${sep}${FOLDER[category]}` : settings.downloadDir;
  const shownDir = saveDir ?? autoDir;
  const duplicates = useMemo(() => {
    const all = Object.values(getState().downloads);
    return urls.filter((u) => all.some((d) => d.url === u || d.finalUrl === u));
  }, [urls]);

  const submit = async (start: boolean) => {
    if (urls.length === 0 || submitting) return;
    setSubmitting(true);
    setError(null);
    try {
      const requests = urls.map((url) => ({
        url,
        start,
        connections,
        saveDir: saveDir ?? undefined,
        fileName: single && (nameEdited.current || info) && fileName.trim() ? fileName.trim() : undefined,
      }));
      onAdded(await api.add(requests));
    } catch (e) {
      setError(errorText(e));
      setSubmitting(false);
    }
  };

  const chooseFolder = async () => {
    const picked = await pickFolder({ directory: true, defaultPath: shownDir });
    if (typeof picked === "string") setSaveDir(picked);
  };

  const action = urls.length > 1 ? `Download ${urls.length} files` : "Download";

  return (
    <Modal title="Add download" onClose={onClose} className="add-dialog">
      <form
        onSubmit={(e) => {
          e.preventDefault();
          submit(true);
        }}
      >
        <label className="field">
          <span className="field-label">Link</span>
          <textarea
            ref={inputRef}
            className="url-input"
            value={text}
            autoFocus
            rows={Math.min(5, Math.max(1, text.split("\n").length))}
            placeholder="https://example.com/file.zip"
            spellCheck={false}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                submit(true);
              }
            }}
          />
          <span className="field-hint">Paste one link, or several on separate lines.</span>
        </label>

        {single && (
          <div className="probe" aria-live="polite">
            <FileIcon category={category} size={22} />
            <div className="probe-main">
              <input
                className="probe-name"
                value={fileName}
                aria-label="File name"
                onChange={(e) => {
                  setFileName(e.target.value);
                  nameEdited.current = true;
                }}
              />
              <div className="probe-meta">
                {probe.kind === "loading" || probe.kind === "idle" ? (
                  <span className="probe-pending">
                    <LoaderCircle size={13} className="spin" /> Checking {hostOf(single)}
                  </span>
                ) : probe.kind === "error" ? (
                  <span className="probe-error">
                    <TriangleExclamationPoint size={13} /> {probe.message}. You can still add it and it will retry.
                  </span>
                ) : (
                  <>
                    <span>{formatBytes(info!.totalSize)}</span>
                    {info!.resumable ? (
                      <span className="probe-ok">
                        <Check size={13} /> Resumable, splits across connections
                      </span>
                    ) : (
                      <span className="probe-warn">Can't resume, pausing restarts it</span>
                    )}
                  </>
                )}
              </div>
            </div>
          </div>
        )}

        {urls.length > 1 && (
          <ul className="batch-list">
            {urls.map((u) => (
              <li key={u}>
                <span className="batch-name">{nameFromUrl(u)}</span>
                <span className="batch-host">{hostOf(u)}</span>
              </li>
            ))}
          </ul>
        )}

        {duplicates.length > 0 && (
          <p className="notice">
            {duplicates.length === 1 && urls.length === 1
              ? "This link is already in your list. Adding it again saves a second copy."
              : `${duplicates.length} of these links are already in your list.`}
          </p>
        )}

        <div className="field-row">
          <div className="field grow">
            <span className="field-label">Save to</span>
            <button type="button" className="folder-btn" onClick={chooseFolder} title={shownDir}>
              <FolderInput size={15} />
              <span className="folder-path">{"\u200e" + shownDir}</span>
              <span className="folder-change">Change</span>
            </button>
            {saveDir && (
              <button type="button" className="link-btn" onClick={() => setSaveDir(null)}>
                Use default folder
              </button>
            )}
          </div>
          <label className="field">
            <span className="field-label">Connections</span>
            <select value={connections} onChange={(e) => setConnections(Number(e.target.value))}>
              {[1, 2, 4, 8, 12, 16, 24, 32].map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </select>
          </label>
        </div>

        {error && <p className="form-error">{error}</p>}

        <footer className="modal-actions">
          <button type="button" className="btn" disabled={urls.length === 0 || submitting} onClick={() => submit(false)}>
            Add paused
          </button>
          <button type="submit" className="btn btn-primary" disabled={urls.length === 0 || submitting}>
            {action}
          </button>
        </footer>
      </form>
    </Modal>
  );
}
