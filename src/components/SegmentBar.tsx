import type { Status } from "../api";

interface Props {
  status: Status;
  downloaded: number;
  total: number | null;
  segments: [number, number, number][];
  large?: boolean;
}

/**
 * Draws each connection's progress at its real byte offset, so the bar shows
 * where in the file data is landing, not just how much has arrived.
 */
export function SegmentBar({ status, downloaded, total, segments, large }: Props) {
  const cls = `segbar segbar-${status}${large ? " segbar-large" : ""}`;
  const label = total ? `${Math.floor((downloaded / total) * 100)}% downloaded` : "Downloading, size unknown";

  if (status === "completed") {
    return <div className={cls} role="img" aria-label="Complete"><div className="segbar-fill" style={{ left: 0, width: "100%" }} /></div>;
  }
  if (!total) {
    return (
      <div className={cls} role="img" aria-label={label}>
        {status === "downloading" && <div className="segbar-indeterminate" />}
      </div>
    );
  }
  const fills = segments.length
    ? segments
    : ([[0, total, Math.min(downloaded, total)]] as [number, number, number][]);
  return (
    <div className={cls} role="img" aria-label={label}>
      {fills.map(([start, end, done]) => (
        <div key={start} className="segbar-lane" style={{ left: `${(start / total) * 100}%`, width: `${((end - start) / total) * 100}%` }}>
          <div className="segbar-fill" style={{ width: `${end > start ? (done / (end - start)) * 100 : 100}%` }} />
        </div>
      ))}
    </div>
  );
}
