import { useStore } from "../store";

const W = 200;
const H = 40;

/** Last minute of total throughput, scaled to its own peak. */
export function SpeedGraph() {
  const history = useStore((s) => s.speedHistory);
  const peak = Math.max(...history, 1);
  const step = W / (history.length - 1);
  const points = history.map((v, i) => `${(i * step).toFixed(1)},${(H - (v / peak) * (H - 2)).toFixed(1)}`);
  const line = `M${points.join(" L")}`;
  return (
    <svg className="speed-graph" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" aria-hidden>
      <path className="speed-graph-area" d={`${line} L${W},${H} L0,${H} Z`} />
      <path className="speed-graph-line" d={line} vectorEffect="non-scaling-stroke" />
    </svg>
  );
}
