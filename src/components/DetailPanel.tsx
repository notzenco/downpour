import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { Copy, ExternalLink, FolderOpen, Pause, Play, RotateCw, Trash, X } from "lucide-react";
import type { Download, ProgressItem } from "../api";
import { formatAgo, formatBytes, formatEta, formatSpeed, percent } from "../format";
import { progressOf } from "../views";
import { FileIcon } from "./FileIcon";
import { SegmentBar } from "./SegmentBar";

interface Props {
  download: Download;
  live?: ProgressItem;
  onClose: () => void;
  onToggle: (d: Download) => void;
  onOpen: (d: Download) => void;
  onReveal: (d: Download) => void;
  onRemove: (d: Download) => void;
  onToast: (text: string) => void;
}

const STATUS_TEXT: Record<Download["status"], string> = {
  queued: "Waiting in queue",
  connecting: "Connecting",
  downloading: "Downloading",
  paused: "Paused",
  completed: "Finished",
  failed: "Failed",
};

export function DetailPanel({ download: d, live, onClose, onToggle, onOpen, onReveal, onRemove, onToast }: Props) {
  const prog = progressOf(d, live);
  const pct = percent(prog.downloaded, prog.total);
  const running = d.status === "downloading" || d.status === "connecting" || d.status === "queued";
  const lanes = [...prog.segments].sort((a, b) => a[0] - b[0]);
  const sep = navigator.userAgent.includes("Windows") ? "\\" : "/";

  const copy = async (text: string, what: string) => {
    await writeText(text);
    onToast(`Copied ${what}`);
  };

  return (
    <aside className="detail" aria-label="Download details">
      <header className="detail-head">
        <FileIcon category={d.category} size={22} />
        <div className="detail-title">
          <h2 title={d.fileName}>{d.fileName}</h2>
          <p className={`detail-status status-${d.status}`}>
            {STATUS_TEXT[d.status]}
            {pct != null && d.status !== "completed" ? `, ${pct.toFixed(1)}%` : ""}
          </p>
        </div>
        <button className="icon-btn" onClick={onClose} aria-label="Close details" title="Close details">
          <X size={16} />
        </button>
      </header>

      {d.status === "failed" && d.error && <p className="detail-error">{d.error}</p>}

      {d.status !== "completed" && (
        <section className="detail-section">
          <SegmentBar status={d.status} downloaded={prog.downloaded} total={prog.total} segments={prog.segments} large />
          {lanes.length > 1 && prog.total ? (
            <div className="lanes">
              <div className="lanes-head">
                <span>Connections</span>
                <span>{d.status === "downloading" ? `${prog.activeConnections} of ${d.connections} active` : `${lanes.length} segments`}</span>
              </div>
              <ol className="lanes-list">
                {lanes.slice(0, 32).map(([start, end, done]) => {
                  const total = prog.total!;
                  const complete = done >= end - start;
                  return (
                    <li key={start} className={complete ? "lane is-complete" : "lane"}>
                      <div className="lane-track">
                        <div className="lane-range" style={{ left: `${(start / total) * 100}%`, width: `${((end - start) / total) * 100}%` }}>
                          <div className="lane-fill" style={{ width: `${(done / Math.max(1, end - start)) * 100}%` }} />
                        </div>
                      </div>
                      <span className="lane-size">{formatBytes(end - start)}</span>
                    </li>
                  );
                })}
              </ol>
            </div>
          ) : null}
        </section>
      )}

      <dl className="facts">
        <Fact label="Size" value={formatBytes(prog.total)} />
        {d.status !== "completed" && <Fact label="Downloaded" value={formatBytes(prog.downloaded)} />}
        {d.status === "downloading" && <Fact label="Speed" value={formatSpeed(prog.speed)} />}
        {d.status === "downloading" && prog.eta != null && <Fact label="Time left" value={formatEta(prog.eta).replace(/ left$/, "")} />}
        <Fact label="Resume" value={d.totalSize == null && d.status !== "completed" ? "Checking…" : d.resumable ? "Supported" : "Not supported, restarts from zero"} />
        <Fact label="Connections" value={d.resumable ? `Up to ${d.connections}` : "1"} />
        <Fact label="Added" value={formatAgo(d.createdAt)} />
        {d.completedAt && <Fact label="Finished" value={formatAgo(d.completedAt)} />}
      </dl>

      <section className="detail-section">
        <div className="path-field">
          <span className="path-label">Address</span>
          <div className="path-value">
            <span title={d.url}>{d.url}</span>
            <button className="icon-btn" onClick={() => copy(d.url, "address")} aria-label="Copy address" title="Copy address">
              <Copy size={14} />
            </button>
          </div>
        </div>
        <div className="path-field">
          <span className="path-label">Saved to</span>
          <div className="path-value">
            <span title={d.saveDir}>{d.saveDir}{sep}{d.fileName}</span>
            <button className="icon-btn" onClick={() => onReveal(d)} aria-label="Show in folder" title="Show in folder">
              <FolderOpen size={14} />
            </button>
          </div>
        </div>
      </section>

      <footer className="detail-actions">
        {d.status === "completed" ? (
          <button className="btn btn-primary" onClick={() => onOpen(d)}>
            <ExternalLink size={15} /> Open file
          </button>
        ) : (
          <button className="btn btn-primary" onClick={() => onToggle(d)}>
            {running ? <Pause size={15} /> : d.status === "failed" ? <RotateCw size={15} /> : <Play size={15} />}
            {running ? "Pause" : d.status === "failed" ? "Retry" : "Resume"}
          </button>
        )}
        <button className="btn" onClick={() => onRemove(d)}>
          <Trash size={15} /> Remove
        </button>
      </footer>
    </aside>
  );
}

function Fact({ label, value }: { label: string; value: string }) {
  return (
    <div className="fact">
      <dt>{label}</dt>
      <dd>{value}</dd>
    </div>
  );
}
