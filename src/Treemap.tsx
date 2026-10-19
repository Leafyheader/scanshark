import { useEffect, useMemo, useRef, useState } from "react";
import { api, FREE_ID, KIND_DIR, KIND_FILE, KIND_FREE, NodeInfo, Rect, Summary } from "./api";
import { formatBytes, formatCount, formatDate, formatSize, percent } from "./format";

const HUES = [50, 185, 125, 290, 20, 215];
const HEADER_H = 14;

interface Props {
  nodeId: number;
  showFree: boolean;
  summary: Summary;
  selectedId: number | null;
  /** Bumped whenever the underlying tree changes (rescan, delete). */
  version: number;
  dark: boolean;
  onSelect: (id: number | null, isDir: boolean) => void;
  onZoomIn: (id: number) => void;
  onZoomOut: () => void;
}

function fill(kind: number, depth: number, dark: boolean): string {
  if (kind === KIND_FREE) return dark ? "#2a2f38" : "#eef0f4";
  const hue = HUES[depth % HUES.length];
  if (kind === KIND_DIR) return dark ? `hsl(${hue} 40% 24%)` : `hsl(${hue} 85% 72%)`;
  return dark ? `hsl(${hue} 45% 34%)` : `hsl(${hue} 85% 82%)`;
}

function fit(ctx: CanvasRenderingContext2D, text: string, maxW: number): string {
  if (ctx.measureText(text).width <= maxW) return text;
  let lo = 0;
  let hi = text.length;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (ctx.measureText(text.slice(0, mid) + "…").width <= maxW) lo = mid;
    else hi = mid - 1;
  }
  return lo > 0 ? text.slice(0, lo) + "…" : "";
}

export function Treemap({ nodeId, showFree, summary, selectedId, version, dark, onSelect, onZoomIn, onZoomOut }: Props) {
  const lastWheel = useRef(0);
  const wrapRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const baseRef = useRef<HTMLCanvasElement | null>(null);
  const [size, setSize] = useState({ w: 0, h: 0 });
  const [rects, setRects] = useState<Rect[]>([]);
  const [hover, setHover] = useState<{ idx: number; x: number; y: number } | null>(null);
  const [tip, setTip] = useState<NodeInfo | null>(null);
  const infoCache = useRef(new Map<number, NodeInfo>());

  useEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    let timer: number | undefined;
    const ro = new ResizeObserver(() => {
      window.clearTimeout(timer);
      timer = window.setTimeout(() => setSize({ w: Math.floor(el.clientWidth), h: Math.floor(el.clientHeight) }), 80);
    });
    ro.observe(el);
    setSize({ w: Math.floor(el.clientWidth), h: Math.floor(el.clientHeight) });
    return () => {
      ro.disconnect();
      window.clearTimeout(timer);
    };
  }, []);

  useEffect(() => {
    infoCache.current.clear();
  }, [version]);

  useEffect(() => {
    if (size.w < 1 || size.h < 1) return;
    let stale = false;
    api.getLayout(nodeId, size.w, size.h, showFree).then((r) => {
      if (!stale) setRects(r);
    });
    return () => {
      stale = true;
    };
  }, [nodeId, showFree, size, version]);

  // Paint the (expensive) rectangles once per layout; hover and selection are drawn on top.
  useEffect(() => {
    const dpr = window.devicePixelRatio || 1;
    const base = (baseRef.current ??= document.createElement("canvas"));
    base.width = Math.max(1, Math.floor(size.w * dpr));
    base.height = Math.max(1, Math.floor(size.h * dpr));
    const ctx = base.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, size.w, size.h);
    ctx.textBaseline = "middle";

    const line = dark ? "rgba(0,0,0,0.65)" : "rgba(0,0,0,0.55)";
    const ink = dark ? "#e8eaee" : "#111";
    const dim = dark ? "#aab0bb" : "#333";

    for (const r of rects) {
      ctx.fillStyle = fill(r.kind, r.depth, dark);
      ctx.fillRect(r.x, r.y, r.w, r.h);
      ctx.strokeStyle = line;
      ctx.lineWidth = 1;
      ctx.strokeRect(Math.floor(r.x) + 0.5, Math.floor(r.y) + 0.5, Math.max(0, r.w - 1), Math.max(0, r.h - 1));

      if (r.kind === KIND_DIR && r.header && r.name) {
        ctx.font = "11px system-ui, sans-serif";
        ctx.fillStyle = ink;
        ctx.textAlign = "left";
        ctx.fillText(fit(ctx, r.name, r.w - 8), r.x + 4, r.y + HEADER_H / 2 + 0.5);
      } else if (r.kind === KIND_FILE && r.name && r.w >= 40 && r.h >= 14) {
        ctx.font = "11px system-ui, sans-serif";
        ctx.textAlign = "center";
        const lines = [r.name];
        if (r.h >= 30) lines.push(formatBytes(r.size));
        if (r.h >= 44) lines.push(formatDate(r.mtime));
        const lh = 14;
        let y = r.y + r.h / 2 - ((lines.length - 1) * lh) / 2;
        lines.forEach((t, i) => {
          ctx.fillStyle = i === 0 ? ink : dim;
          const s = fit(ctx, t, r.w - 6);
          if (s) ctx.fillText(s, r.x + r.w / 2, y);
          y += lh;
        });
      } else if (r.kind === KIND_FREE && r.w >= 120 && r.h >= 80) {
        ctx.font = "12px system-ui, sans-serif";
        ctx.textAlign = "center";
        ctx.fillStyle = ink;
        const lines = [
          `Free Space: ${percent(summary.disk_free, summary.disk_total)}`,
          `${formatSize(summary.disk_free)} free`,
          `Files: ${formatCount(summary.files)}`,
          `Folders: ${formatCount(summary.dirs)}`,
        ];
        lines.forEach((t, i) => ctx.fillText(t, r.x + r.w / 2, r.y + r.h / 2 + (i - 1.5) * 16));
      }
    }
    draw();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rects, dark, size, summary]);

  const selectedIdx = useMemo(
    () => (selectedId === null ? -1 : rects.findIndex((r) => r.id === selectedId)),
    [rects, selectedId],
  );

  function draw() {
    const canvas = canvasRef.current;
    const base = baseRef.current;
    if (!canvas || !base) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = base.width;
    canvas.height = base.height;
    const ctx = canvas.getContext("2d")!;
    ctx.drawImage(base, 0, 0);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    const outline = (r: Rect, color: string, width: number) => {
      ctx.strokeStyle = color;
      ctx.lineWidth = width;
      ctx.strokeRect(r.x + width / 2, r.y + width / 2, r.w - width, r.h - width);
    };
    if (hover && rects[hover.idx]) {
      ctx.fillStyle = "rgba(255,255,255,0.22)";
      const r = rects[hover.idx];
      ctx.fillRect(r.x, r.y, r.w, r.h);
    }
    if (selectedIdx >= 0) outline(rects[selectedIdx], dark ? "#7cc4ff" : "#0050d0", 3);
  }

  useEffect(draw, [hover, selectedIdx, rects]);

  const hitTest = (e: React.MouseEvent): number => {
    const box = canvasRef.current!.getBoundingClientRect();
    const px = e.clientX - box.left;
    const py = e.clientY - box.top;
    for (let i = rects.length - 1; i >= 0; i--) {
      const r = rects[i];
      if (px >= r.x && px < r.x + r.w && py >= r.y && py < r.y + r.h) return i;
    }
    return -1;
  };

  const hoverId = hover ? rects[hover.idx]?.id : undefined;
  useEffect(() => {
    setTip(null);
    if (hoverId === undefined || hoverId === FREE_ID) return;
    const cached = infoCache.current.get(hoverId);
    if (cached) {
      setTip(cached);
      return;
    }
    let stale = false;
    const t = window.setTimeout(() => {
      api.nodeInfo(hoverId).then((info) => {
        infoCache.current.set(hoverId, info);
        if (!stale) setTip(info);
      }).catch(() => {});
    }, 120);
    return () => {
      stale = true;
      window.clearTimeout(t);
    };
  }, [hoverId]);

  const hovered = hover ? rects[hover.idx] : undefined;

  return (
    <div ref={wrapRef} className="treemap">
      <canvas
        ref={canvasRef}
        style={{ width: size.w, height: size.h }}
        onMouseMove={(e) => {
          const idx = hitTest(e);
          setHover(idx < 0 ? null : { idx, x: e.clientX, y: e.clientY });
        }}
        onMouseLeave={() => setHover(null)}
        onWheel={(e) => {
          const now = performance.now();
          if (now - lastWheel.current < 250) return;
          if (e.deltaY > 0) {
            lastWheel.current = now;
            onZoomOut();
          } else if (e.deltaY < 0 && rects.length) {
            // Zoom into the top-level folder under the cursor.
            const box = canvasRef.current!.getBoundingClientRect();
            const px = e.clientX - box.left;
            const py = e.clientY - box.top;
            const top = rects[0].depth + 1;
            const r = rects.find(
              (q) => q.depth === top && q.kind === KIND_DIR && px >= q.x && px < q.x + q.w && py >= q.y && py < q.y + q.h,
            );
            if (r) {
              lastWheel.current = now;
              onZoomIn(r.id);
            }
          }
        }}
        onClick={(e) => {
          const idx = hitTest(e);
          const r = rects[idx];
          if (!r || r.kind === KIND_FREE) onSelect(null, false);
          else onSelect(r.id, r.kind === KIND_DIR);
        }}
        onDoubleClick={(e) => {
          const r = rects[hitTest(e)];
          if (r?.kind === KIND_DIR && r.id !== nodeId) onZoomIn(r.id);
        }}
      />
      {hover && hovered && (
        <div
          className="tooltip"
          style={{
            left: Math.min(hover.x + 14, window.innerWidth - 340),
            top: Math.min(hover.y + 18, window.innerHeight - 120),
          }}
        >
          {hovered.kind === KIND_FREE ? (
            <>
              <strong>Free space</strong>
              <span>{formatSize(summary.disk_free)} of {formatSize(summary.disk_total)}</span>
            </>
          ) : tip ? (
            <>
              <strong>{tip.name}</strong>
              <span className="path">{tip.path}</span>
              <span>
                {formatSize(tip.size)} · {percent(tip.size, tip.parent_size)} of parent
              </span>
              {tip.is_dir && (
                <span>{formatCount(tip.files)} files, {formatCount(tip.dirs)} folders</span>
              )}
              {tip.mtime > 0 && <span>{formatDate(tip.mtime)}</span>}
            </>
          ) : (
            <strong>{hovered.name || "…"}</strong>
          )}
        </div>
      )}
    </div>
  );
}
