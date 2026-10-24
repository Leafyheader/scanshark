import { ReactNode } from "react";
import { Folder, Root, Summary, TopFile } from "./api";
import { formatCount, formatSize, percent } from "./format";

const icons = {
  drive: (
    <>
      <rect x="3" y="13" width="18" height="7" rx="2" />
      <path d="M5 13 7.5 5h9L19 13" />
      <circle cx="17" cy="16.5" r="0.8" fill="currentColor" />
    </>
  ),
  home: <path d="M4 11 12 4l8 7v9h-5v-6H9v6H4z" />,
  files: (
    <>
      <path d="M8 3h7l4 4v11H8z" />
      <path d="M5 7v14h11" />
    </>
  ),
  folder: <path d="M3 6h6l2 2h10v11H3z" />,
  file: (
    <>
      <path d="M6 3h8l5 5v13H6z" />
      <path d="M14 3v5h5" />
    </>
  ),
  open: (
    <>
      <path d="M3 19V5h6l2 2h8v3" />
      <path d="M3 19l3-8h15l-3 8z" />
    </>
  ),
  reload: (
    <>
      <path d="M20 12a8 8 0 1 1-2.4-5.7" />
      <path d="M20 4v5h-5" />
    </>
  ),
  rescan: (
    <>
      <path d="M4.5 10A8 8 0 0 1 18 6.5M19.5 14A8 8 0 0 1 6 17.5" />
      <path d="M18 3v4h-4M6 21v-4h4" />
    </>
  ),
  export: (
    <>
      <path d="M12 3v12" />
      <path d="m7 10 5 5 5-5" />
      <path d="M4 20h16" />
    </>
  ),
  view: (
    <>
      <rect x="3" y="4" width="18" height="16" rx="2" />
      <path d="M15 4v16M3 10h12" />
    </>
  ),
  settings: (
    <>
      <path d="M4 7h9M17 7h3M4 17h3M11 17h9" />
      <circle cx="15" cy="7" r="2" />
      <circle cx="9" cy="17" r="2" />
    </>
  ),
  about: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 11v6M12 7.5v.5" />
    </>
  ),
  top: (
    <>
      <path d="M5 4h14M12 20V9" />
      <path d="m7 13 5-5 5 5" />
    </>
  ),
  up: (
    <>
      <path d="M12 19V5" />
      <path d="m6 11 6-6 6 6" />
    </>
  ),
  zoomIn: (
    <>
      <circle cx="11" cy="11" r="7" />
      <path d="m20 20-4-4M11 8v6M8 11h6" />
    </>
  ),
  free: (
    <>
      <path d="M11 4a8 8 0 1 0 9 9h-9z" />
      <path d="M14 3a7 7 0 0 1 7 7h-7z" />
    </>
  ),
  search: (
    <>
      <circle cx="11" cy="11" r="7" />
      <path d="m20 20-4-4" />
    </>
  ),
};

export type IconName = keyof typeof icons;

export function Icon({ name, size = 20 }: { name: IconName; size?: number }) {
  return (
    <svg
      className="icon"
      viewBox="0 0 24 24"
      width={size}
      height={size}
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      {icons[name]}
    </svg>
  );
}

function UsageBar({ used, total }: { used: number; total: number }) {
  return (
    <span className="usage">
      <span style={{ width: percent(used, total) }} />
    </span>
  );
}

/** Last part of a path, or the path itself for a drive root like "/" or "C:\". */
export function baseName(path: string): string {
  return path.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || path;
}

function Card(props: { icon: keyof typeof icons; label: string; onClick?: () => void; title?: string; children: ReactNode }) {
  const body = (
    <>
      <span className="tile">
        <Icon name={props.icon} />
      </span>
      <span className="body">
        <span className="label">{props.label}</span>
        {props.children}
      </span>
    </>
  );
  return props.onClick ? (
    <button className="stat" onClick={props.onClick} title={props.title}>
      {body}
    </button>
  ) : (
    <div className="stat" title={props.title}>
      {body}
    </div>
  );
}

interface SummaryCardsProps {
  summary: Summary;
  /** The drive or Home entry for the scanned root, when it is one. */
  root?: Root;
  onShowFile: (f: TopFile) => void;
  onShowFolder: (f: Folder) => void;
}

export function SummaryCards({ summary, root, onShowFile, onShowFolder }: SummaryCardsProps) {
  const used = summary.disk_total - summary.disk_free;
  const file = summary.largest_file;
  const folder = summary.largest_folder;
  return (
    <div className="stats">
      <Card icon={root?.label === "Home folder" ? "home" : "drive"} label={root?.label ?? baseName(summary.root_path)} title={summary.root_path}>
        {summary.disk_total > 0 ? (
          <>
            <span className="value">
              {formatSize(used)} <span className="of">of {formatSize(summary.disk_total)}</span>
            </span>
            <span className="meter">
              <UsageBar used={used} total={summary.disk_total} />
              {percent(used, summary.disk_total)}
            </span>
          </>
        ) : (
          <span className="value">{formatSize(summary.total_size)}</span>
        )}
      </Card>
      <Card icon="files" label="Total Files">
        <span className="value">{formatCount(summary.files)}</span>
        <span className="sub">{formatSize(summary.total_size)} scanned</span>
      </Card>
      <Card
        icon="folder"
        label="Folders"
        onClick={folder ? () => onShowFolder(folder) : undefined}
        title={folder ? `Zoom into ${folder.name}` : undefined}
      >
        <span className="value">{formatCount(summary.dirs)}</span>
        <span className="sub">{folder ? `Largest: ${folder.name} · ${formatSize(folder.size)}` : "No subfolders"}</span>
      </Card>
      <Card
        icon="file"
        label="Largest File"
        onClick={file ? () => onShowFile(file) : undefined}
        title={file ? `Show ${file.dir ? `${file.name} in ${file.dir}` : file.name}` : undefined}
      >
        <span className="value name">{file?.name ?? "None"}</span>
        <span className="sub">{file ? formatSize(file.size) : "No files scanned"}</span>
      </Card>
    </div>
  );
}

interface DrivesProps {
  roots: Root[];
  /** The scanned root, listed on its own when it isn't one of `roots`. */
  current: Summary;
  disabled: boolean;
  onPick: (path: string) => void;
}

export function Drives({ roots, current, disabled, onPick }: DrivesProps) {
  const isRoot = roots.some((r) => r.path === current.root_path);
  const entry = (r: Root, icon: keyof typeof icons) => {
    const on = r.path === current.root_path;
    return (
      <button key={r.path} className={on ? "drive on" : "drive"} onClick={() => !on && onPick(r.path)} disabled={disabled && !on} title={r.path}>
        <span className="head">
          <Icon name={icon} />
          <span className="name">{r.label}</span>
        </span>
        {r.label !== r.path && <span className="where">{r.path}</span>}
        {r.total > 0 && (
          <>
            <UsageBar used={r.total - r.free} total={r.total} />
            <span className="sub">
              <span>
                {formatSize(r.total - r.free)} / {formatSize(r.total)}
              </span>
              <span>{percent(r.total - r.free, r.total)}</span>
            </span>
          </>
        )}
      </button>
    );
  };
  return (
    <nav className="drives">
      {!isRoot && (
        <>
          <h3>Folder</h3>
          {entry(
            { path: current.root_path, label: baseName(current.root_path), total: current.disk_total, free: current.disk_free },
            "folder",
          )}
        </>
      )}
      <h3>Drives</h3>
      {roots.map((r) => entry(r, r.label === "Home folder" ? "home" : "drive"))}
    </nav>
  );
}
