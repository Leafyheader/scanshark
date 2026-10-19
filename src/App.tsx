import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import { api, NodeInfo, Root, ScanProgress, Summary } from "./api";
import { formatCount, formatSize, percent } from "./format";
import logo from "./assets/logo.png";
import { Treemap } from "./Treemap";
import "./App.css";

type Theme = "system" | "light" | "dark";

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
  const [error, setError] = useState<string | null>(null);

  const [zoomStack, setZoomStack] = useState<number[]>([0]);
  const [selected, setSelected] = useState<{ id: number; isDir: boolean } | null>(null);
  const [version, setVersion] = useState(0);
  const [showFree, setShowFree] = useState(() => load("showFree", true));
  const [oneFs, setOneFs] = useState(() => load("oneFs", false));
  const [theme, setTheme] = useState<Theme>(() => load("theme", "system"));
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

  const scan = useCallback(
    async (path: string) => {
      setError(null);
      setScanning(true);
      setProgress(null);
      try {
        const s = await api.startScan(path, oneFs);
        if (s) {
          setSummary(s);
          setZoomStack([0]);
          setSelected(null);
          setVersion((v) => v + 1);
        }
      } catch (e) {
        setError(String(e));
      } finally {
        setScanning(false);
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

  const requestDelete = async () => {
    if (!selected) return;
    try {
      setConfirm(await api.nodeInfo(selected.id));
    } catch (e) {
      setError(String(e));
    }
  };

  const doDelete = async () => {
    if (!selected || !confirm) return;
    const id = selected.id;
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
      if (confirm || setupOpen || scanning) return;
      if (e.key === "Delete" && selected && selected.id !== 0) requestDelete();
      else if (e.key === "Backspace" || e.key === "Escape") zoomOut();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const [crumbNames, setCrumbNames] = useState<Record<number, string>>({});
  useEffect(() => {
    zoomStack.forEach((id) => {
      if (id !== 0 && !(id in crumbNames)) {
        api.nodeInfo(id).then((n) => setCrumbNames((c) => ({ ...c, [id]: n.name }))).catch(() => {});
      }
    });
  }, [zoomStack, crumbNames]);
  useEffect(() => setCrumbNames({}), [version]);

  const hasTree = summary !== null;
  const canZoomIn = !!selected?.isDir && selected.id !== current;

  return (
    <div className="app">
      <div className="toolbar">
        <button onClick={pickFolder} disabled={scanning}>Open…</button>
        <button onClick={() => summary && scan(summary.root_path)} disabled={!hasTree || scanning}>Reload</button>
        <span className="sep" />
        <button onClick={zoomFull} disabled={!hasTree || zoomStack.length === 1}>Zoom Full</button>
        <button onClick={() => selected && zoomIn(selected.id)} disabled={!canZoomIn}>Zoom In</button>
        <button onClick={zoomOut} disabled={!hasTree || zoomStack.length === 1}>Zoom Out</button>
        <span className="sep" />
        <button className={showFree ? "on" : ""} onClick={() => setShowFree((v) => !v)} disabled={!hasTree}>
          Free Space
        </button>
        <span className="sep" />
        <button onClick={() => selected && api.openNode(selected.id).catch((e) => setError(String(e)))} disabled={!selected}>
          Open Item
        </button>
        <button onClick={() => selected && api.revealNode(selected.id).catch((e) => setError(String(e)))} disabled={!selected}>
          Show in Folder
        </button>
        <button className="danger" onClick={requestDelete} disabled={!selected || selected.id === 0}>
          Delete
        </button>
        <span className="grow" />
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

        {scanning && (
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
                Built with Rust, Tauri and React.
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
            <span className="crumbs">
              {zoomStack.length > 1 &&
                zoomStack.map((id, i) => (
                  <span key={`${id}-${i}`}>
                    <a onClick={() => zoomTo(i)}>{i === 0 ? "Top" : crumbNames[id] ?? "…"}</a>
                    {i < zoomStack.length - 1 && " › "}
                  </span>
                ))}
              {zoomStack.length > 1 && " · "}
              {formatCount(summary.files)} files · {formatCount(summary.dirs)} folders · {formatSize(summary.total_size)} used
            </span>
            <span>scanned in {(summary.scan_ms / 1000).toFixed(1)}s</span>
          </>
        ) : (
          <span>Ready</span>
        )}
      </div>
    </div>
  );
}
