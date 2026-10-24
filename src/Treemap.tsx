import { useEffect, useMemo, useRef, useState } from "react";
import { api, FREE_ID, KIND_DIR, KIND_FREE, NodeInfo, Rect, Summary } from "./api";
import { formatCount, formatDate, formatSize, percent, SizeUnits } from "./format";

/** One hue per top-level block, biggest first; everything inside a block shares its hue. */
const HUES = [221, 152, 268, 28, 188, 352, 46, 318, 200, 96];
const FONT = 'system-ui, -apple-system, "Segoe UI", sans-serif';

interface Props {
  nodeId: number;
  showFree: boolean;
  summary: Summary;
  selectedId: number | null;
  /** Bumped whenever the underlying tree changes (rescan, delete). */
  version: number;
  dark: boolean;
  /** The size units in use; only here so the labels are drawn again when they change. */
  units: SizeUnits;
  onSelect: (id: number | null, isDir: boolean) => void;
  onZoomIn: (id: number) => void;
  onZoomOut: () => void;
}

/** Lightness of a block `level` steps below the top-level one; nested folders alternate so each level stands out. */
function lightness(kind: number, level: number, dark: boolean): number {
  // Light mode keeps every block light enough for dark text, whatever its hue.
  const dir = dark ? [30, 41, 35, 45] : [62, 71, 66, 74];
  const l = dir[Math.min(level, 3)];
  return kind === KIND_DIR ? l : l + (dark ? 9 : 8);
}

/** Rounded rect path, falling back to a plain one where `roundRect` is missing or the block is tiny. */
function shape(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  ctx.beginPath();
  if (r > 0 && w >= 10 && h >= 10 && ctx.roundRect) ctx.roundRect(x, y, w, h, Math.min(r, w / 2, h / 2));
  else ctx.rect(x, y, w, h);
}

/** Sibling blocks are drawn inset by this much, leaving a gutter between them. */
function inset(r: Rect): [number, number, number, number] {
  return r.w >= 4 && r.h >= 4 ? [r.x + 1, r.y + 1, r.w - 2, r.h - 2] : [r.x, r.y, r.w, r.h];
}

/** Folder name on the left of its header strip, size on the right when both fit. */
function header(ctx: CanvasRenderingContext2D, r: Rect, name: string, size: string, fg: string, dim: string) {
  const big = r.header >= 24;
  const pad = big ? 9 : 6;
  const y = r.y + 1 + r.header / 2;
  const maxW = r.w - pad * 2;
  ctx.font = `600 ${big ? 13 : 11}px ${FONT}`;
  const nameW = ctx.measureText(name).width;
  ctx.font = `${big ? 12 : 10.5}px ${FONT}`;
  const sizeW = ctx.measureText(size).width;
  const showSize = nameW + sizeW + 12 <= maxW;
  if (showSize) {
    ctx.fillStyle = dim;
    ctx.textAlign = "right";
    ctx.fillText(size, r.x + r.w - pad, y);
  }
  ctx.font = `600 ${big ? 13 : 11}px ${FONT}`;
  ctx.fillStyle = fg;
  ctx.textAlign = "left";
  ctx.fillText(fit(ctx, name, showSize ? maxW - sizeW - 12 : maxW), r.x + pad, y);
}

/** Name, size and share stacked in the top-left corner, as many lines as fit. */
function label(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, lines: string[], fg: string, dim: string) {
  const pad = w >= 80 && h >= 40 ? 8 : 5;
  const lh = 15;
  ctx.textAlign = "left";
  lines.forEach((t, i) => {
    if (!t || pad + (i + 1) * lh > h) return;
    ctx.font = i === 0 ? `600 11.5px ${FONT}` : `11px ${FONT}`;
    ctx.fillStyle = i === 0 ? fg : dim;
    const s = fit(ctx, t, w - pad * 2);
    if (s) ctx.fillText(s, x + pad, y + pad + i * lh + lh / 2 - 1);
  });
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

export function Treemap({ nodeId, showFree, summary, selectedId, version, dark, units, onSelect, onZoomIn, onZoomOut }: Props) {
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
      if (stale) return;
      setRects(r);
      setHover(null); // indexes into the old rects
    }).catch(() => {
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

    const ink = dark ? "#e6ebf5" : "#2b2620";
    const muted = dark ? "#8b97b0" : "#73695b";
    const total = rects[0]?.size ?? 0;
    let hue = 0;
    let topLevel = -1;

    rects.forEach((r, i) => {
      if (r.depth === 1) hue = HUES[++topLevel % HUES.length];
      const [x, y, w, h] = inset(r);
      const radius = r.depth <= 1 ? 7 : 4;
      const share = total > 0 && r.size < total ? ` (${percent(r.size, total)})` : "";

      if (r.depth === 0) return; // the view's own folder is the backdrop

      let fg = "#fff";
      let fgDim = "rgba(255,255,255,0.72)";
      if (r.kind === KIND_FREE) {
        ctx.fillStyle = dark ? "#1c263b" : "#eee7da";
        shape(ctx, x, y, w, h, radius);
        ctx.fill();
        ctx.setLineDash([4, 3]);
        ctx.strokeStyle = dark ? "#3a4763" : "#c9bfad";
        ctx.lineWidth = 1;
        ctx.stroke();
        ctx.setLineDash([]);
        fg = ink;
        fgDim = muted;
      } else {
        const l = lightness(r.kind, r.depth - 1, dark);
        ctx.fillStyle = `hsl(${hue} ${dark ? 55 : 50}% ${l}%)`;
        shape(ctx, x, y, w, h, radius);
        ctx.fill();
        if (r.kind === KIND_DIR && w >= 6 && h >= 6) {
          ctx.strokeStyle = r.depth === 1 ? `hsl(${hue} 80% ${dark ? 60 : 38}%)` : `hsl(${hue} 60% ${l + (dark ? 12 : -10)}%)`;
          ctx.lineWidth = r.depth === 1 ? 1.5 : 1;
          ctx.stroke();
        }
        if (!dark) {
          fg = ink;
          fgDim = "rgba(43,38,32,0.72)";
        }
      }

      if (r.header && r.name) {
        header(ctx, r, r.name, formatSize(r.size) + share, fg, fgDim);
      } else if (r.kind === KIND_FREE && w >= 90 && h >= 50) {
        label(ctx, x, y, w, h, ["Free Space", formatSize(summary.disk_free), `${percent(summary.disk_free, summary.disk_total)} of disk`], fg, fgDim);
      } else if (r.name && w >= 44 && h >= 18 && !(rects[i + 1]?.depth > r.depth)) {
        // Rects come parents-first, so a deeper next rect is a child that would cover the label.
        label(ctx, x, y, w, h, [r.name, formatSize(r.size), share.trim()], fg, fgDim);
      }
    });
    draw();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rects, dark, size, summary, units]);

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
    const hovered = hover ? rects[hover.idx] : undefined;
    if (hovered && hovered.depth > 0) {
      const [x, y, w, h] = inset(hovered);
      shape(ctx, x, y, w, h, hovered.depth <= 1 ? 7 : 4);
      ctx.fillStyle = dark ? "rgba(255,255,255,0.16)" : "rgba(255,255,255,0.35)";
      ctx.fill();
    }
    if (selectedIdx >= 0) {
      const r = rects[selectedIdx];
      const [x, y, w, h] = inset(r);
      shape(ctx, x + 1, y + 1, Math.max(0, w - 2), Math.max(0, h - 2), r.depth <= 1 ? 6 : 3);
      ctx.strokeStyle = dark ? "#ffffff" : "#2b2620";
      ctx.lineWidth = 2;
      ctx.stroke();
    }
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
