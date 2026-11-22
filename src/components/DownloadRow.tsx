import { FolderOpen, Pause, Play, RotateCw, Trash } from "lucide-react";
import { memo } from "react";
import type { Download, ProgressItem } from "../api";
import { formatAgo, formatBytes, formatEta, formatSpeed, hostOf, percent } from "../format";
import { progressOf } from "../views";
import { FileIcon } from "./FileIcon";
import { SegmentBar } from "./SegmentBar";

interface Props {
  download: Download;
  live?: ProgressItem;
  selected: boolean;
  queueIndex: number | null;
  onSelect: (id: string, e: React.MouseEvent) => void;
  onToggle: (d: Download) => void;
  onOpen: (d: Download) => void;
  onReveal: (d: Download) => void;
  onRemove: (d: Download) => void;
}

export const DownloadRow = memo(function DownloadRow(p: Props) {
  const d = p.download;
  const prog = progressOf(d, p.live);
  const pct = percent(prog.downloaded, prog.total);
  const sizeText = prog.total ? `${formatBytes(prog.downloaded)} of ${formatBytes(prog.total)}` : formatBytes(prog.downloaded);

  let detail: React.ReactNode;
  let trailing: React.ReactNode = null;
  switch (d.status) {
    case "downloading":
      detail = sizeText;
      trailing = (
        <>
          <span className="row-speed">{formatSpeed(prog.speed)}</span>
          <span className="row-eta">{formatEta(prog.eta)}</span>
        </>
      );
      break;
    case "connecting":
      detail = `Connecting to ${hostOf(d.url)}…`;
      break;
    case "queued":
      detail = p.queueIndex != null ? `Waiting in queue, position ${p.queueIndex}` : "Waiting in queue";
      if (d.downloaded > 0) trailing = <span className="row-eta">{sizeText}</span>;
      break;
    case "paused":
      detail = pct != null ? `Paused at ${Math.floor(pct)}%, ${sizeText}` : `Paused, ${sizeText}`;
      if (!d.resumable && d.downloaded > 0) trailing = <span className="row-warn">Restarts on resume</span>;
      break;
    case "failed":
      detail = <span className="row-error">{d.error ?? "Download failed"}</span>;
      break;
    case "completed":
      detail = `${formatBytes(d.totalSize ?? d.downloaded)} from ${hostOf(d.finalUrl ?? d.url)}`;
      trailing = <span className="row-eta">{d.completedAt ? formatAgo(d.completedAt) : ""}</span>;
      break;
  }

  const running = d.status === "downloading" || d.status === "connecting" || d.status === "queued";

  return (
    <li
      className={`row${p.selected ? " is-selected" : ""} status-${d.status}`}
      data-row={d.id}
      aria-selected={p.selected}
      role="option"
      onMouseDown={(e) => p.onSelect(d.id, e)}
      onDoubleClick={() => (d.status === "completed" ? p.onOpen(d) : p.onToggle(d))}
    >
      <FileIcon category={d.category} />
      <div className="row-main">
        <div className="row-top">
          <span className="row-name" title={d.fileName}>{d.fileName}</span>
          <span className="row-trailing">{trailing}</span>
        </div>
        <div className="row-detail">{detail}</div>
        {d.status !== "completed" && !(d.status === "failed" && prog.downloaded === 0) && (
          <SegmentBar status={d.status} downloaded={prog.downloaded} total={prog.total} segments={prog.segments} />
        )}
      </div>
      <div className="row-actions" onMouseDown={(e) => e.stopPropagation()}>
        {d.status !== "completed" && !(d.status === "failed" && prog.downloaded === 0) && (
          <button
            className="icon-btn"
            title={running ? "Pause" : d.status === "failed" ? "Retry" : "Resume"}
            aria-label={running ? `Pause ${d.fileName}` : `Resume ${d.fileName}`}
            onClick={() => p.onToggle(d)}
          >
            {running ? <Pause size={16} /> : d.status === "failed" ? <RotateCw size={16} /> : <Play size={16} />}
          </button>
        )}
        <button className="icon-btn" title="Show in folder" aria-label={`Show ${d.fileName} in folder`} onClick={() => p.onReveal(d)}>
          <FolderOpen size={16} />
        </button>
        <button className="icon-btn" title="Remove" aria-label={`Remove ${d.fileName}`} onClick={() => p.onRemove(d)}>
          <Trash size={16} />
        </button>
      </div>
    </li>
  );
});
