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
  header: boolean;
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
}

export interface NodeInfo {
  path: string;
  name: string;
  size: number;
  mtime: number;
  is_dir: boolean;
  files: number;
  dirs: number;
  parent_size: number;
}

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
  startScan: (path: string, oneFilesystem: boolean) =>
    invoke<Summary | null>("start_scan", { path, oneFilesystem }),
  cancelScan: () => invoke<void>("cancel_scan"),
  getLayout: (id: number, width: number, height: number, showFree: boolean) =>
    invoke<Rect[]>("get_layout", { id, width, height, showFree }),
  nodeInfo: (id: number) => invoke<NodeInfo>("node_info", { id }),
  openNode: (id: number) => invoke<void>("open_node", { id }),
  revealNode: (id: number) => invoke<void>("reveal_node", { id }),
  deleteNode: (id: number) => invoke<Summary>("delete_node", { id }),
};
