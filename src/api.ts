import { invoke } from "@tauri-apps/api/core";

export const KIND_FILE = 0;
export const KIND_DIR = 1;
export const KIND_FREE = 2;
export const FREE_ID = 4294967295;

export interface Rect {
  id: number;
  x: number;
  y: number;
  w: number;
  h: number;
  depth: number;
  kind: number;
  /** Height of the directory name strip, 0 when there is none. */
  header: number;
  name: string;
  size: number;
  mtime: number;
}

export interface Summary {
  root_path: string;
  total_size: number;
  files: number;
  dirs: number;
  disk_total: number;
  disk_free: number;
  scan_ms: number;
  /** Unix seconds when the scan started. */
  scanned_at: number;
  /** Loaded from a snapshot; a fresh scan hasn't replaced it yet. */
  cached: boolean;
  largest_file: TopFile | null;
  /** The biggest folder directly under the root. */
  largest_folder: Folder | null;
}

export interface Folder {
  id: number;
  name: string;
  size: number;
}

export interface NodeInfo {
  id: number;
  path: string;
  name: string;
  size: number;
  mtime: number;
  is_dir: boolean;
  files: number;
  dirs: number;
  parent_size: number;
}

export interface TopFile {
  id: number;
  name: string;
  /** Folder holding the file, relative to the folder asked about. */
  dir: string;
  size: number;
  mtime: number;
  /** Folders to zoom through to reach the file's folder, top first. */
  trail: number[];
}

export interface Child {
  id: number;
  name: string;
  size: number;
  mtime: number;
  is_dir: boolean;
  files: number;
  dirs: number;
}

export interface Hit {
  id: number;
  name: string;
  /** Folder holding the item, relative to the root. */
  dir: string;
  size: number;
  is_dir: boolean;
  /** Folders to zoom through from the root to reach the item's folder, top first. */
  trail: number[];
}

/** "csv" lists the folder's own items, "csv-all" everything below it. */
export type ReportFormat = "html" | "csv" | "csv-all";

export interface Root {
  path: string;
  label: string;
  total: number;
  free: number;
}

export interface ScanProgress {
  files: number;
  dirs: number;
  bytes: number;
  current: string;
}

export const api = {
  listRoots: () => invoke<Root[]>("list_roots"),
  openCached: (path: string) => invoke<Summary | null>("open_cached", { path }),
  startScan: (path: string, oneFilesystem: boolean, full: boolean) =>
    invoke<Summary | null>("start_scan", { path, oneFilesystem, full }),
  cancelScan: () => invoke<void>("cancel_scan"),
  getLayout: (id: number, width: number, height: number, showFree: boolean) =>
    invoke<Rect[]>("get_layout", { id, width, height, showFree }),
  nodeInfo: (id: number) => invoke<NodeInfo>("node_info", { id }),
  topFiles: (id: number, limit: number) => invoke<TopFile[]>("top_files", { id, limit }),
  children: (id: number) => invoke<Child[]>("children", { id }),
  openNode: (id: number) => invoke<void>("open_node", { id }),
  revealNode: (id: number) => invoke<void>("reveal_node", { id }),
  deleteNode: (id: number) => invoke<Summary>("delete_node", { id }),
  locate: (paths: string[]) => invoke<(number | null)[]>("locate", { paths }),
  search: (query: string, limit: number) => invoke<{ hits: Hit[]; total: number }>("search", { query, limit }),
  exportReport: (id: number, path: string, format: ReportFormat, decimal: boolean) =>
    invoke<void>("export_report", { id, path, format, decimal }),
  openExport: () => invoke<void>("open_export"),
  cacheSize: () => invoke<number>("cache_size"),
  clearCache: () => invoke<void>("clear_cache"),
};
