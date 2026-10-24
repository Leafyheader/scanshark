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
};

export function Icon({ name }: { name: keyof typeof icons }) {
  return (
    <svg className="icon" viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinejoin="round" aria-hidden>
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
