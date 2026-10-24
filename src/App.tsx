import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import { api, Folder, NodeInfo, Root, ScanProgress, Summary, TopFile } from "./api";
import { formatAge, formatCount, formatSize, percent } from "./format";
import logo from "./assets/logo.png";
import { Drives, SummaryCards } from "./Overview";
import { ListView, Sort } from "./ListView";
import { Details, TopFiles } from "./Sidebar";
import { Treemap } from "./Treemap";
import "./App.css";

type Theme = "system" | "light" | "dark";
type View = "treemap" | "list";
/** The optional parts of the layout, switched on and off from the View menu. */
type Panels = { drives: boolean; stats: boolean; details: boolean; topFiles: boolean };
const PANELS: [keyof Panels, string][] = [
  ["drives", "Drives"],
  ["stats", "Summary Cards"],
  ["details", "Item Details"],
  ["topFiles", "Top Files"],
];

function load<T>(key: string, fallback: T): T {
  try {
    const v = localStorage.getItem(key);
    return v === null ? fallback : (JSON.parse(v) as T);
  } catch {
    return fallback;
  }
}
function save(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* storage unavailable */
  }
}

export default function App() {
  const [roots, setRoots] = useState<Root[]>([]);
  const [summary, setSummary] = useState<Summary | null>(null);
  const [progress, setProgress] = useState<ScanProgress | null>(null);
  const [scanning, setScanning] = useState(false);
  /** The scan runs behind a tree that is already on show, instead of under the progress dialog. */
  const [background, setBackground] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [zoomStack, setZoomStack] = useState<number[]>([0]);
  const [selected, setSelected] = useState<{ id: number; isDir: boolean } | null>(null);
  const [version, setVersion] = useState(0);
  const [showFree, setShowFree] = useState(() => load("showFree", true));
  const [oneFs, setOneFs] = useState(() => load("oneFs", false));
  const [theme, setTheme] = useState<Theme>(() => load("theme", "system"));
  const [view, setView] = useState<View>(() => load("view", "treemap"));
  const [panels, setPanels] = useState<Panels>(() => ({
    drives: true,
    stats: true,
    details: true,
    topFiles: true,
    ...load<Partial<Panels>>("panels", {}),
  }));
  const [viewMenu, setViewMenu] = useState(false);
  const [listSort, setListSort] = useState<Sort>(() => load("listSort", { key: "size", desc: true }));
  const [systemDark, setSystemDark] = useState(() => window.matchMedia("(prefers-color-scheme: dark)").matches);
  const [setupOpen, setSetupOpen] = useState(false);
  const [aboutOpen, setAboutOpen] = useState(false);
  const [appVersion, setAppVersion] = useState("");
  useEffect(() => {
    getVersion().then(setAppVersion).catch(() => {});
  }, []);
  const [confirm, setConfirm] = useState<NodeInfo | null>(null);

  const dark = theme === "dark" || (theme === "system" && systemDark);
  const current = zoomStack[zoomStack.length - 1];

  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const on = () => setSystemDark(mq.matches);
    mq.addEventListener("change", on);
    return () => mq.removeEventListener("change", on);
  }, []);
  useEffect(() => {
    document.documentElement.dataset.theme = dark ? "dark" : "light";
  }, [dark]);
  useEffect(() => save("showFree", showFree), [showFree]);
  useEffect(() => save("oneFs", oneFs), [oneFs]);
  useEffect(() => save("theme", theme), [theme]);
  useEffect(() => save("view", view), [view]);
  useEffect(() => save("listSort", listSort), [listSort]);
  useEffect(() => save("panels", panels), [panels]);

  useEffect(() => {
    api.listRoots().then(setRoots).catch(() => {});
    const un = listen<ScanProgress>("scan-progress", (e) => setProgress(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, []);

  useEffect(() => {
    const title = summary
      ? `${summary.root_path} - ${formatSize(summary.disk_total)} Total - ${formatSize(summary.disk_free)} Free - ScanShark`
      : "ScanShark";
    getCurrentWindow().setTitle(title).catch(() => {});
  }, [summary]);

  /** The latest view state, read once a scan finishes to keep the same place in the new tree. */
  const live = useRef<{
    summary: Summary | null;
    zoomStack: number[];
    selected: { id: number; isDir: boolean } | null;
    crumbs: Record<number, NodeInfo>;
    targetInfo: NodeInfo | null;
  } | null>(null);

  /** Finds the zoomed folders and the selection again, by path, after the tree was replaced. */
  const findPlace = async () => {
    const { zoomStack, selected, crumbs, targetInfo } = live.current!;
    const sel = selected && targetInfo?.id === selected.id ? targetInfo.path : "";
    const ids = await api.locate([...zoomStack.slice(1).map((id) => crumbs[id]?.path ?? ""), sel]);
    const stack = [0];
    const lost = ids.slice(0, -1).findIndex((id) => id === null);
    for (const id of lost < 0 ? ids.slice(0, -1) : ids.slice(0, lost)) stack.push(id!);
    const selId = ids[ids.length - 1];
    return { stack, selected: lost < 0 && selId !== null && selected ? { id: selId, isDir: selected.isDir } : null };
  };

  /** Scans `path`. A tree of the same root, on show or saved from last time, stays up meanwhile. */
  const scan = useCallback(
    async (path: string, full = false) => {
      setError(null);
      setScanning(true);
      setProgress(null);
      try {
        let shown = live.current?.summary?.root_path === path;
        if (!shown) {
          const cached = await api.openCached(path);
          if (cached) {
            setSummary(cached);
            setZoomStack([0]);
            setSelected(null);
            setVersion((v) => v + 1);
            shown = true;
          }
        }
        setBackground(shown);
        const s = await api.startScan(path, oneFs, full);
        if (s) {
          const place = shown ? await findPlace() : { stack: [0], selected: null };
          setSummary(s);
          setZoomStack(place.stack);
          setSelected(place.selected);
          setVersion((v) => v + 1);
        }
      } catch (e) {
        setError(String(e));
      } finally {
        setScanning(false);
        setBackground(false);
        api.listRoots().then(setRoots).catch(() => {});
      }
    },
    [oneFs],
  );

  const pickFolder = async () => {
    const dir = await open({ directory: true, multiple: false, defaultPath: summary?.root_path });
    if (typeof dir === "string") scan(dir);
  };

  const zoomIn = (id: number) => {
    if (id === current) return;
    setZoomStack((s) => [...s, id]);
    setSelected(null);
  };
  /** Zoom out one level and select the folder we just left, so you keep your place. */
  const zoomOut = () => {
    if (zoomStack.length < 2) return;
    setSelected({ id: current, isDir: true });
    setZoomStack((s) => s.slice(0, -1));
  };
  const zoomFull = () => {
    setZoomStack([0]);
    setSelected(null);
  };
  const zoomTo = (level: number) => {
    if (level >= zoomStack.length - 1) return;
    setSelected({ id: current, isDir: true });
    setZoomStack((s) => s.slice(0, level + 1));
  };

  /** Jump to one of the Top Files: zoom to its folder and select it. */
  const showFile = (f: TopFile) => {
    setZoomStack((s) => [...s, ...f.trail]);
    setSelected({ id: f.id, isDir: false });
  };
  /** The summary cards describe the whole scan, so their links start from the root. */
  const showRootFile = (f: TopFile) => {
    setZoomStack([0, ...f.trail]);
    setSelected({ id: f.id, isDir: false });
  };
  const showRootFolder = (f: Folder) => {
    setZoomStack([0, f.id]);
    setSelected(null);
  };

  /** What the details panel and its buttons act on: the selection, or else the folder being viewed. */
  const target = selected?.id ?? current;

  const requestDelete = async (id = target) => {
    if (id === 0) return;
    try {
      setConfirm(await api.nodeInfo(id));
    } catch (e) {
      setError(String(e));
    }
  };

  const doDelete = async () => {
    if (!confirm) return;
    const id = confirm.id;
    setConfirm(null);
    try {
      setSummary(await api.deleteNode(id));
      setSelected(null);
      setZoomStack((s) => (s.includes(id) ? s.slice(0, s.indexOf(id)) : s));
      setVersion((v) => v + 1);
    } catch (e) {
      setError(String(e));
    }
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && (confirm || setupOpen || aboutOpen || viewMenu)) {
        setViewMenu(false);
        setConfirm(null);
        setSetupOpen(false);
        setAboutOpen(false);
        return;
      }
      if (confirm || setupOpen || aboutOpen || (scanning && !background)) return;
      if (e.key === "Delete" && selected && !scanning) requestDelete(selected.id);
      else if (e.key === "Backspace" || e.key === "Escape") zoomOut();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const [crumbs, setCrumbs] = useState<Record<number, NodeInfo>>({});
  useEffect(() => {
    zoomStack.forEach((id) => {
      if (!(id in crumbs)) {
        api.nodeInfo(id).then((n) => setCrumbs((c) => ({ ...c, [id]: n }))).catch(() => {});
      }
    });
  }, [zoomStack, crumbs]);
  useEffect(() => setCrumbs({}), [version]);
  const viewSize = current === 0 ? summary?.total_size : crumbs[current]?.size;

  const [targetInfo, setTargetInfo] = useState<NodeInfo | null>(null);
  useEffect(() => {
    if (!summary) return;
    let stale = false;
    api.nodeInfo(target).then((n) => !stale && setTargetInfo(n)).catch(() => !stale && setTargetInfo(null));
    return () => {
      stale = true;
    };
  }, [target, version, summary]);

  const [topFiles, setTopFiles] = useState<TopFile[]>([]);
  useEffect(() => {
    if (!summary) return;
    let stale = false;
    api.topFiles(current, 10).then((f) => !stale && setTopFiles(f)).catch(() => !stale && setTopFiles([]));
    return () => {
      stale = true;
    };
  }, [current, version, summary]);

  const [cacheSize, setCacheSize] = useState<number | null>(null);
  useEffect(() => {
    if (setupOpen) api.cacheSize().then(setCacheSize).catch(() => setCacheSize(null));
  }, [setupOpen]);
  const clearCache = async () => {
    try {
      await api.clearCache();
      setCacheSize(await api.cacheSize());
    } catch (e) {
      setError(String(e));
    }
  };

  live.current = { summary, zoomStack, selected, crumbs, targetInfo };

  const hasTree = summary !== null;
  const canZoomIn = !!selected?.isDir && selected.id !== current;

  return (
    <div className="app">
      <div className="toolbar">
        <button onClick={pickFolder} disabled={scanning}>Open…</button>
        <button
          onClick={() => summary && scan(summary.root_path)}
          disabled={!hasTree || scanning}
          title="Scan again, skipping folders that haven't changed"
        >
          Reload
        </button>
        <button
          onClick={() => summary && scan(summary.root_path, true)}
          disabled={!hasTree || scanning}
          title="Scan everything again, including files that changed size in place"
        >
          Full Rescan
        </button>
        <span className="sep" />
        <button onClick={zoomFull} disabled={!hasTree || zoomStack.length === 1}>Zoom Full</button>
        <button onClick={() => selected && zoomIn(selected.id)} disabled={!canZoomIn}>Zoom In</button>
        <button onClick={zoomOut} disabled={!hasTree || zoomStack.length === 1}>Zoom Out</button>
        <span className="sep" />
        <button className={showFree ? "on" : ""} onClick={() => setShowFree((v) => !v)} disabled={!hasTree || view !== "treemap"}>
          Free Space
        </button>
        <span className="grow" />
        <div className="menu-wrap">
          <button className={viewMenu ? "open" : ""} onClick={() => setViewMenu((v) => !v)} aria-haspopup="menu" aria-expanded={viewMenu}>
            View ▾
          </button>
          {viewMenu && (
            <>
              <div className="menu-backdrop" onClick={() => setViewMenu(false)} />
              <div className="menu" role="menu">
                {PANELS.map(([key, label]) => (
                  <label key={key} role="menuitemcheckbox" aria-checked={panels[key]}>
                    <input type="checkbox" checked={panels[key]} onChange={() => setPanels((p) => ({ ...p, [key]: !p[key] }))} />
                    {label}
                  </label>
                ))}
              </div>
            </>
          )}
        </div>
        <button onClick={() => setSetupOpen(true)}>Setup</button>
        <button onClick={() => setAboutOpen(true)}>About</button>
      </div>

      {error && (
        <div className="error" onClick={() => setError(null)}>
          {error}
        </div>
      )}

      <div className="main">
        {hasTree ? (
          <div className="workspace">
          {panels.drives && <Drives roots={roots} current={summary} disabled={scanning} onPick={scan} />}
          <div className="content">
          {panels.stats && <SummaryCards
            summary={summary}
            root={roots.find((r) => r.path === summary.root_path)}
            onShowFile={showRootFile}
            onShowFolder={showRootFolder}
          />}
          <div className="work">
          <section className="panel">
            <header className="panel-head">
              <h2>Disk Usage</h2>
              <nav className="crumbs">
                {zoomStack.map((id, i) => (
                  <span key={`${id}-${i}`}>
                    {i > 0 && <span className="chev">›</span>}
                    {i < zoomStack.length - 1 ? (
                      <a onClick={() => zoomTo(i)}>{i === 0 ? summary.root_path : crumbs[id]?.name ?? "…"}</a>
                    ) : (
                      <span className="here">{i === 0 ? summary.root_path : crumbs[id]?.name ?? "…"}</span>
                    )}
                  </span>
                ))}
              </nav>
              <span className="grow" />
              <div className="tabs" role="tablist">
                <button role="tab" aria-selected={view === "treemap"} className={view === "treemap" ? "on" : ""} onClick={() => setView("treemap")}>
                  Treemap
                </button>
                <button role="tab" aria-selected={view === "list"} className={view === "list" ? "on" : ""} onClick={() => setView("list")}>
                  List
                </button>
              </div>
              {viewSize !== undefined && (
                <span className="muted">
                  {formatSize(viewSize)}
                  {summary.disk_total > 0 && ` · ${percent(viewSize, summary.disk_total)} of disk`}
                </span>
              )}
            </header>
            <div className="panel-body">
              {view === "treemap" ? (
                <Treemap
                  nodeId={current}
                  showFree={showFree}
                  summary={summary}
                  selectedId={selected?.id ?? null}
                  version={version}
                  dark={dark}
                  onSelect={(id, isDir) => setSelected(id === null ? null : { id, isDir })}
                  onZoomIn={zoomIn}
                  onZoomOut={zoomOut}
                />
              ) : (
                <ListView
                  nodeId={current}
                  version={version}
                  selectedId={selected?.id ?? null}
                  sort={listSort}
                  onSort={setListSort}
                  onSelect={(id, isDir) => setSelected(id === null ? null : { id, isDir })}
                  onZoomIn={zoomIn}
                />
              )}
            </div>
          </section>
          {(panels.details || panels.topFiles) && (
          <aside className="sidebar">
            {panels.details && <Details
              info={targetInfo?.id === target ? targetInfo : null}
              isSelection={selected !== null}
              isRoot={target === 0}
              diskTotal={summary.disk_total}
              canZoomIn={canZoomIn}
              canDelete={!scanning}
              onOpen={() => api.openNode(target).catch((e) => setError(String(e)))}
              onReveal={() => api.revealNode(target).catch((e) => setError(String(e)))}
              onZoomIn={() => zoomIn(target)}
              onDelete={() => requestDelete()}
            />}
            {panels.topFiles && <TopFiles files={topFiles} selectedId={selected?.id ?? null} onPick={showFile} />}
          </aside>
          )}
          </div>
          </div>
          </div>
        ) : (
          <div className="welcome">
            <img className="hero" src={logo} alt="ScanShark" width={220} height={220} />
            <p>Pick a drive or folder to see where the space went.</p>
            <div className="roots">
              {roots.map((r) => (
                <button key={r.path} className="root" onClick={() => scan(r.path)} disabled={scanning}>
                  <strong>{r.label}</strong>
                  <span className="path">{r.path}</span>
                  <div className="bar">
                    <div style={{ width: percent(r.total - r.free, r.total) }} />
                  </div>
                  <span>
                    {formatSize(r.free)} free of {formatSize(r.total)}
                  </span>
                </button>
              ))}
            </div>
            <button className="primary" onClick={pickFolder} disabled={scanning}>
              Choose folder…
            </button>
          </div>
        )}

        {scanning && !background && (
          <div className="overlay">
            <div className="dialog">
              <h2>Scanning…</h2>
              <p>
                {formatCount(progress?.files ?? 0)} files · {formatCount(progress?.dirs ?? 0)} folders ·{" "}
                {formatSize(progress?.bytes ?? 0)}
              </p>
              <p className="path">{progress?.current}</p>
              <button onClick={() => api.cancelScan()}>Cancel</button>
            </div>
          </div>
        )}

        {confirm && (
          <div className="overlay">
            <div className="dialog">
              <h2>Move to trash?</h2>
              <p className="path">{confirm.path}</p>
              <p>
                {formatSize(confirm.size)}
                {confirm.is_dir && ` · ${formatCount(confirm.files)} files`}
              </p>
              <div className="actions">
                <button onClick={() => setConfirm(null)}>Cancel</button>
                <button className="danger" autoFocus onClick={doDelete}>
                  Move to Trash
                </button>
              </div>
            </div>
          </div>
        )}

        {aboutOpen && (
          <div className="overlay" onClick={() => setAboutOpen(false)}>
            <div className="dialog about" onClick={(e) => e.stopPropagation()}>
              <img className="logo" src={logo} alt="ScanShark logo" width={180} height={180} />
              <p className="muted">Version {appVersion || "…"}</p>
              <p>A fast, lightweight disk-space treemap. See where the space went.</p>
              <p className="muted">
               
                <br />© 2026 Leafyhead Technologies
                <br />Released under the MIT License.
              </p>
              <div className="actions">
                <button onClick={() => setAboutOpen(false)}>Close</button>
              </div>
            </div>
          </div>
        )}

        {setupOpen && (
          <div className="overlay" onClick={() => setSetupOpen(false)}>
            <div className="dialog" onClick={(e) => e.stopPropagation()}>
              <h2>Setup</h2>
              <label>
                Theme{" "}
                <select value={theme} onChange={(e) => setTheme(e.target.value as Theme)}>
                  <option value="system">System</option>
                  <option value="light">Light</option>
                  <option value="dark">Dark</option>
                </select>
              </label>
              <label>
                <input type="checkbox" checked={oneFs} onChange={(e) => setOneFs(e.target.checked)} /> Stay on one
                filesystem when scanning
              </label>
              <div className="setting">
                <span>
                  Saved scans{cacheSize !== null && `: ${formatSize(cacheSize)}`}
                  <br />
                  <span className="muted">Used to show a drive straight away and rescan it faster.</span>
                </span>
                <button onClick={clearCache} disabled={scanning || !cacheSize}>
                  Clear
                </button>
              </div>
              <div className="actions">
                <button onClick={() => setSetupOpen(false)}>Close</button>
              </div>
            </div>
          </div>
        )}
      </div>

      <div className="status">
        {summary ? (
          <>
            {scanning && background ? (
              <span className="refresh">
                <span className="spinner" />
                Refreshing… {formatCount(progress?.files ?? 0)} files · {formatCount(progress?.dirs ?? 0)} folders
                <span className="path">{progress?.current}</span>
                <a onClick={() => api.cancelScan()}>Cancel</a>
              </span>
            ) : (
              <span>
                {summary.cached && <span className="badge">Cached {formatAge(summary.scanned_at)}</span>}
                {formatCount(summary.files)} files · {formatCount(summary.dirs)} folders · {formatSize(summary.total_size)} scanned
              </span>
            )}
            <span className="disk">
              {summary.disk_total > 0 && (
                <>
                  <span className="bar">
                    <span style={{ width: percent(summary.disk_total - summary.disk_free, summary.disk_total) }} />
                  </span>
                  {formatSize(summary.disk_total - summary.disk_free)} used of {formatSize(summary.disk_total)} ·{" "}
                </>
              )}
              {summary.cached ? "last scan took" : "scanned in"} {(summary.scan_ms / 1000).toFixed(1)}s
            </span>
          </>
        ) : (
          <span>Ready</span>
        )}
      </div>
    </div>
  );
}
