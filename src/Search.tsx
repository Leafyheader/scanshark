import { useEffect, useRef, useState } from "react";
import { api, Hit } from "./api";
import { formatCount, formatSize } from "./format";
import { Icon } from "./Overview";

/** How many results to fetch; the rest are only counted. */
const LIMIT = 200;

interface Props {
  /** Bumped whenever the tree is replaced, so open results are fetched again. */
  version: number;
  onPick: (hit: Hit) => void;
}

/** The toolbar search box: matches names across the whole scan, biggest first. */
export function Search({ version, onPick }: Props) {
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<Hit[]>([]);
  const [total, setTotal] = useState(0);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLOListElement>(null);

  useEffect(() => {
    if (!query.trim()) {
      setHits([]);
      setTotal(0);
      return;
    }
    let stale = false;
    const t = setTimeout(() => {
      api
        .search(query, LIMIT)
        .then((r) => {
          if (stale) return;
          setHits(r.hits);
          setTotal(r.total);
          setActive(0);
        })
        .catch(() => !stale && setHits([]));
    }, 180);
    return () => {
      stale = true;
      clearTimeout(t);
    };
  }, [query, version]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") {
        e.preventDefault();
        input.current?.focus();
        input.current?.select();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    list.current?.children[active]?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const pick = (h: Hit) => {
    onPick(h);
    setOpen(false);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") setActive((a) => Math.min(a + 1, hits.length - 1));
    else if (e.key === "ArrowUp") setActive((a) => Math.max(a - 1, 0));
    else if (e.key === "Enter" && hits[active]) pick(hits[active]);
    else if (e.key === "Escape") {
      if (open && query) setOpen(false);
      else {
        setQuery("");
        input.current?.blur();
      }
    } else return;
    e.preventDefault();
    e.stopPropagation();
  };

  const showList = open && query.trim() !== "";
  return (
    <div className="search">
      <input
        ref={input}
        type="search"
        placeholder="Search names… (Ctrl+F)"
        value={query}
        spellCheck={false}
        onChange={(e) => {
          setQuery(e.target.value);
          setOpen(true);
        }}
        onFocus={() => setOpen(true)}
        onKeyDown={onKeyDown}
        aria-label="Search the scan by name"
      />
      {showList && (
        <>
          <div className="menu-backdrop" onClick={() => setOpen(false)} />
          <div className="menu results">
            <div className="count">
              {total === 0
                ? "No matches"
                : total > hits.length
                  ? `Biggest ${formatCount(hits.length)} of ${formatCount(total)} matches`
                  : `${formatCount(total)} ${total === 1 ? "match" : "matches"}`}
              <span className="hint">Use * and ? as wildcards, e.g. *.iso</span>
            </div>
            <ol ref={list}>
              {hits.map((h, i) => (
                <li key={h.id}>
                  <button
                    className={i === active ? "row picked" : "row"}
                    onMouseEnter={() => setActive(i)}
                    onClick={() => pick(h)}
                    title={h.dir ? `${h.name} in ${h.dir}` : h.name}
                  >
                    <span className="name">
                      <Icon name={h.is_dir ? "folder" : "file"} />
                      {h.name}
                    </span>
                    <span className="size">{formatSize(h.size)}</span>
                    <span className="dir">{h.dir || "top level"}</span>
                  </button>
                </li>
              ))}
            </ol>
          </div>
        </>
      )}
    </div>
  );
}
