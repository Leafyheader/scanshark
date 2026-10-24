import { KeyboardEvent, useEffect, useMemo, useRef, useState } from "react";
import { api, Child } from "./api";
import { formatCount, formatShortDate, formatSize, percent } from "./format";
import { Icon } from "./Overview";

export type SortKey = "name" | "size" | "files" | "mtime";
export interface Sort {
  key: SortKey;
  desc: boolean;
}

/** Rows drawn at first; a folder with more offers to show the rest. */
const PAGE = 1000;

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

function compare(key: SortKey, a: Child, b: Child): number {
  switch (key) {
    case "name":
      return collator.compare(a.name, b.name);
    case "size":
      return a.size - b.size;
    case "files":
      // Files sort below every folder.
      return (a.is_dir ? a.files : -1) - (b.is_dir ? b.files : -1);
    case "mtime":
      return a.mtime - b.mtime;
  }
}

interface Props {
  nodeId: number;
  version: number;
  selectedId: number | null;
  sort: Sort;
  onSort: (s: Sort) => void;
  onSelect: (id: number | null, isDir: boolean) => void;
  onZoomIn: (id: number) => void;
}

export function ListView({ nodeId, version, selectedId, sort, onSort, onSelect, onZoomIn }: Props) {
  const [items, setItems] = useState<Child[] | null>(null);
  const [showAll, setShowAll] = useState(false);
  const body = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let stale = false;
    setShowAll(false);
    api.children(nodeId).then((c) => !stale && setItems(c)).catch(() => !stale && setItems([]));
    return () => {
      stale = true;
    };
  }, [nodeId, version]);

  const rows = useMemo(() => {
    if (!items) return [];
    const dir = sort.desc ? -1 : 1;
    // Ties fall back to size, biggest first, which is the order the items arrive in.
    return [...items].sort((a, b) => dir * compare(sort.key, a, b) || b.size - a.size);
  }, [items, sort]);
  const total = useMemo(() => rows.reduce((s, r) => s + r.size, 0), [rows]);
  const biggest = useMemo(() => rows.reduce((m, r) => Math.max(m, r.size), 0), [rows]);
  const selIndex = rows.findIndex((r) => r.id === selectedId);
  const shown = showAll || selIndex >= PAGE ? rows : rows.slice(0, PAGE);

  // Keep the selection in sight, for example the folder just zoomed out of.
  useEffect(() => {
    body.current?.querySelector(".picked")?.scrollIntoView({ block: "nearest" });
  }, [selectedId, rows]);

  const head = (key: SortKey, label: string, className = "") => {
    const on = sort.key === key;
    return (
      <th className={className} aria-sort={on ? (sort.desc ? "descending" : "ascending") : undefined}>
        <button onClick={() => onSort(on ? { key, desc: !sort.desc } : { key, desc: key !== "name" })}>
          {label}
          <span className="arrow">{on ? (sort.desc ? "▼" : "▲") : ""}</span>
        </button>
      </th>
    );
  };

  const onKey = (e: KeyboardEvent) => {
    if (rows.length === 0) return;
    const go = (i: number) => {
      const r = rows[Math.max(0, Math.min(rows.length - 1, i))];
      onSelect(r.id, r.is_dir);
    };
    if (e.key === "ArrowDown") go(selIndex + 1);
    else if (e.key === "ArrowUp") go(selIndex < 0 ? rows.length - 1 : selIndex - 1);
    else if (e.key === "Home") go(0);
    else if (e.key === "End") go(rows.length - 1);
    else if (e.key === "Enter" && selIndex >= 0 && rows[selIndex].is_dir) onZoomIn(rows[selIndex].id);
    else return;
    e.preventDefault();
  };

  return (
    <div className="list" ref={body} tabIndex={0} onKeyDown={onKey}>
      <table>
        <thead>
          <tr>
            {head("name", "Name", "name")}
            {head("size", "Size", "num")}
            <th className="share">Share</th>
            {head("files", "Files", "num")}
            {head("mtime", "Modified", "date")}
          </tr>
        </thead>
        <tbody>
          {shown.map((r) => (
            <tr
              key={r.id}
              className={r.id === selectedId ? "picked" : undefined}
              onClick={() => onSelect(r.id, r.is_dir)}
              onDoubleClick={() => r.is_dir && onZoomIn(r.id)}
            >
              <td className="name" title={r.name}>
                <span className="cell">
                  <Icon name={r.is_dir ? "folder" : "file"} />
                  <span className="text">{r.name}</span>
                </span>
              </td>
              <td className="num">{formatSize(r.size)}</td>
              <td className="share">
                <span className="cell">
                  <span className="bar">
                    <span style={{ width: percent(r.size, biggest) }} />
                  </span>
                  {percent(r.size, total)}
                </span>
              </td>
              <td className="num">{r.is_dir ? formatCount(r.files) : ""}</td>
              <td className="date">{formatShortDate(r.mtime)}</td>
            </tr>
          ))}
        </tbody>
      </table>
      {items && rows.length === 0 && <div className="empty">This folder is empty.</div>}
      {shown.length < rows.length && (
        <div className="more">
          <button onClick={() => setShowAll(true)}>Show all {formatCount(rows.length)} items</button>
        </div>
      )}
    </div>
  );
}
