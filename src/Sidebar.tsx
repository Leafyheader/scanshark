import { NodeInfo, TopFile } from "./api";
import { formatCount, formatDate, formatSize, percent } from "./format";

interface DetailsProps {
  info: NodeInfo | null;
  /** True when `info` is the clicked item, false when it is the folder being viewed. */
  isSelection: boolean;
  isRoot: boolean;
  diskTotal: number;
  canZoomIn: boolean;
  canDelete: boolean;
  onOpen: () => void;
  onReveal: () => void;
  onZoomIn: () => void;
  onDelete: () => void;
}

export function Details({ info, isSelection, isRoot, diskTotal, canZoomIn, canDelete, onOpen, onReveal, onZoomIn, onDelete }: DetailsProps) {
  return (
    <section className="card details">
      <h3>{isSelection ? "Selected Item" : "This Folder"}</h3>
      {info ? (
        <>
          <div className="name" title={info.name}>{info.name}</div>
          <div className="path">{info.path}</div>
          <div className="big">{formatSize(info.size)}</div>
          <div className="muted">
            {!isRoot && `${percent(info.size, info.parent_size)} of parent`}
            {!isRoot && diskTotal > 0 && " · "}
            {diskTotal > 0 && `${percent(info.size, diskTotal)} of disk`}
          </div>
          <dl>
            <dt>Type</dt>
            <dd>{info.is_dir ? "Folder" : "File"}</dd>
            {info.is_dir && (
              <>
                <dt>Items</dt>
                <dd>
                  {formatCount(info.files)} files, {formatCount(info.dirs)} folders
                </dd>
              </>
            )}
            {info.mtime > 0 && (
              <>
                <dt>Modified</dt>
                <dd>{formatDate(info.mtime)}</dd>
              </>
            )}
          </dl>
          <div className="buttons">
            <button onClick={onOpen}>{info.is_dir ? "Open Folder" : "Open File"}</button>
            <button onClick={onReveal}>Show in Folder</button>
            <button onClick={onZoomIn} disabled={!canZoomIn}>Zoom In</button>
            <button className="danger" onClick={onDelete} disabled={isRoot || !canDelete}>Delete</button>
          </div>
        </>
      ) : (
        <div className="muted">…</div>
      )}
    </section>
  );
}

interface TopFilesProps {
  files: TopFile[];
  selectedId: number | null;
  onPick: (f: TopFile) => void;
}

export function TopFiles({ files, selectedId, onPick }: TopFilesProps) {
  const biggest = files[0]?.size ?? 0;
  return (
    <section className="card top-files">
      <h3>Top Files</h3>
      {files.length === 0 && <div className="muted">No files here.</div>}
      <ol>
        {files.map((f) => (
          <li key={f.id}>
            <button className={f.id === selectedId ? "row picked" : "row"} onClick={() => onPick(f)} title={f.dir ? `${f.name} in ${f.dir}` : f.name}>
              <span className="name">{f.name}</span>
              <span className="size">{formatSize(f.size)}</span>
              <span className="dir">{f.dir || "this folder"}</span>
              <span className="bar">
                <span style={{ width: percent(f.size, biggest) }} />
              </span>
            </button>
          </li>
        ))}
      </ol>
    </section>
  );
}
