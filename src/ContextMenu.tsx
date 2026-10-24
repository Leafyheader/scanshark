import { useLayoutEffect, useRef, useState } from "react";

export type MenuItem =
  | { label: string; onClick: () => void; disabled?: boolean; danger?: boolean; checked?: boolean }
  | "sep";

interface Props {
  x: number;
  y: number;
  items: MenuItem[];
  onClose: () => void;
}

/** A right-click menu at (x, y), moved back inside the window if it would spill out. */
export function ContextMenu({ x, y, items, onClose }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x, y });
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    setPos({
      x: Math.max(4, Math.min(x, window.innerWidth - el.offsetWidth - 4)),
      y: Math.max(4, Math.min(y, window.innerHeight - el.offsetHeight - 4)),
    });
  }, [x, y]);

  return (
    <>
      <div
        className="menu-backdrop"
        onClick={onClose}
        onContextMenu={(e) => {
          e.preventDefault();
          onClose();
        }}
        onWheel={onClose}
      />
      <div ref={ref} className="menu context-menu" role="menu" style={{ left: pos.x, top: pos.y }}>
        {items.map((item, i) =>
          item === "sep" ? (
            <div key={i} className="menu-sep" role="separator" />
          ) : (
            <button
              key={i}
              role={item.checked === undefined ? "menuitem" : "menuitemcheckbox"}
              aria-checked={item.checked}
              className={item.danger ? "danger" : undefined}
              disabled={item.disabled}
              onClick={() => {
                onClose();
                item.onClick();
              }}
            >
              <span className="check">{item.checked ? "✓" : ""}</span>
              {item.label}
            </button>
          ),
        )}
      </div>
    </>
  );
}
