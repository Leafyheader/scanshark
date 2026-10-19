mod layout;
mod scan;
mod tree;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use sysinfo::Disks;
use tauri::{AppHandle, Emitter, State};

use layout::Rect;
use tree::{NodeId, Tree, ROOT};

#[derive(Default)]
struct AppState {
    tree: Mutex<Option<Tree>>,
    cancel: Arc<AtomicBool>,
    scanning: AtomicBool,
}

#[derive(Serialize, Clone)]
struct Summary {
    root_path: String,
    total_size: u64,
    files: u64,
    dirs: u64,
    disk_total: u64,
    disk_free: u64,
    scan_ms: u64,
}

#[derive(Serialize)]
struct NodeInfo {
    path: String,
    name: String,
    size: u64,
    mtime: i64,
    is_dir: bool,
    files: u64,
    dirs: u64,
    parent_size: u64,
}

#[derive(Serialize)]
struct Root {
    path: String,
    label: String,
    total: u64,
    free: u64,
}

#[derive(Serialize, Clone)]
struct ScanProgress {
    files: u64,
    dirs: u64,
    bytes: u64,
    current: String,
}

fn summarize(tree: &Tree, scan_ms: u64) -> Summary {
    let root = &tree.nodes[ROOT as usize];
    Summary {
        root_path: tree.root_path.to_string_lossy().into_owned(),
        total_size: root.size,
        files: root.files,
        dirs: root.dirs,
        disk_total: tree.disk_total,
        disk_free: tree.disk_free,
        scan_ms,
    }
}

/// Total and free bytes of the filesystem holding `path`.
fn disk_space(path: &Path) -> (u64, u64) {
    let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    Disks::new_with_refreshed_list()
        .list()
        .iter()
        .filter(|d| canon.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map_or((0, 0), |d| (d.total_space(), d.available_space()))
}

#[tauri::command]
fn list_roots() -> Vec<Root> {
    let mut roots: Vec<Root> = Vec::new();
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let home = PathBuf::from(home);
        let (total, free) = disk_space(&home);
        roots.push(Root { path: home.to_string_lossy().into_owned(), label: "Home folder".into(), total, free });
    }
    for d in Disks::new_with_refreshed_list().list() {
        let mount = d.mount_point().to_string_lossy().into_owned();
        let fs = d.file_system().to_string_lossy().to_lowercase();
        let pseudo = matches!(fs.as_str(), "tmpfs" | "devtmpfs" | "squashfs" | "overlay" | "efivarfs")
            || mount.starts_with("/run")
            || mount.starts_with("/snap")
            || mount.starts_with("/boot");
        if d.total_space() == 0 || pseudo || roots.iter().any(|r| r.path == mount) {
            continue;
        }
        roots.push(Root { path: mount.clone(), label: mount, total: d.total_space(), free: d.available_space() });
    }
    roots
}

#[tauri::command]
async fn start_scan(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    one_filesystem: bool,
) -> Result<Option<Summary>, String> {
    if state.scanning.swap(true, Ordering::SeqCst) {
        return Err("a scan is already running".into());
    }
    state.cancel.store(false, Ordering::SeqCst);

    let cancel = state.cancel.clone();
    let progress = Arc::new(scan::Progress::default());
    let finished = Arc::new(AtomicBool::new(false));

    let reporter = {
        let (progress, finished, app) = (progress.clone(), finished.clone(), app.clone());
        std::thread::spawn(move || {
            while !finished.load(Ordering::Relaxed) {
                let current = progress.current.lock().map(|c| c.clone()).unwrap_or_default();
                let _ = app.emit(
                    "scan-progress",
                    ScanProgress {
                        files: progress.files.load(Ordering::Relaxed),
                        dirs: progress.dirs.load(Ordering::Relaxed),
                        bytes: progress.bytes.load(Ordering::Relaxed),
                        current,
                    },
                );
                std::thread::sleep(Duration::from_millis(100));
            }
        })
    };

    let root = PathBuf::from(&path);
    let started = Instant::now();
    let result = {
        let (root, progress) = (root.clone(), progress.clone());
        tauri::async_runtime::spawn_blocking(move || {
            scan::scan(&root, &scan::Options { one_filesystem }, &progress, &cancel)
        })
        .await
        .map_err(|e| e.to_string())
    };

    finished.store(true, Ordering::Relaxed);
    let _ = reporter.join();
    state.scanning.store(false, Ordering::SeqCst);

    let Some(nodes) = result?? else { return Ok(None) };
    let (disk_total, disk_free) = disk_space(&root);
    let tree = scan::build_tree(nodes, &root, disk_total, disk_free);
    let summary = summarize(&tree, started.elapsed().as_millis() as u64);
    *state.tree.lock().unwrap() = Some(tree);
    Ok(Some(summary))
}

#[tauri::command]
fn cancel_scan(state: State<'_, AppState>) {
    state.cancel.store(true, Ordering::SeqCst);
}

#[tauri::command]
async fn get_layout(
    state: State<'_, AppState>,
    id: NodeId,
    width: f64,
    height: f64,
    show_free: bool,
) -> Result<Vec<Rect>, String> {
    let guard = state.tree.lock().unwrap();
    let tree = guard.as_ref().ok_or("nothing scanned yet")?;
    Ok(layout::layout(tree, id, width, height, show_free))
}

#[tauri::command]
fn node_info(state: State<'_, AppState>, id: NodeId) -> Result<NodeInfo, String> {
    let guard = state.tree.lock().unwrap();
    let tree = guard.as_ref().ok_or("nothing scanned yet")?;
    let n = tree.node(id).ok_or("unknown node")?;
    let parent_size = n.parent.and_then(|p| tree.node(p)).map_or(n.size, |p| p.size);
    Ok(NodeInfo {
        path: tree.path_of(id).to_string_lossy().into_owned(),
        name: n.name.to_string(),
        size: n.size,
        mtime: n.mtime,
        is_dir: n.is_dir,
        files: n.files,
        dirs: n.dirs,
        parent_size,
    })
}

/// Opens a file with its default application, or a folder in the file manager.
#[tauri::command]
fn open_node(state: State<'_, AppState>, id: NodeId) -> Result<(), String> {
    let path = {
        let guard = state.tree.lock().unwrap();
        guard.as_ref().ok_or("nothing scanned yet")?.path_of(id)
    };
    tauri_plugin_opener::open_path(path, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
fn reveal_node(state: State<'_, AppState>, id: NodeId) -> Result<(), String> {
    let path = {
        let guard = state.tree.lock().unwrap();
        guard.as_ref().ok_or("nothing scanned yet")?.path_of(id)
    };
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(|e| e.to_string())
}

/// Moves a node to the trash and updates the in-memory tree.
#[tauri::command]
async fn delete_node(state: State<'_, AppState>, id: NodeId) -> Result<Summary, String> {
    let path = {
        let guard = state.tree.lock().unwrap();
        let tree = guard.as_ref().ok_or("nothing scanned yet")?;
        if id == ROOT {
            return Err("cannot delete the scanned root".into());
        }
        tree.node(id).ok_or("unknown node")?;
        tree.path_of(id)
    };
    trash::delete(&path).map_err(|e| format!("{}: {e}", path.display()))?;

    let mut guard = state.tree.lock().unwrap();
    let tree = guard.as_mut().ok_or("nothing scanned yet")?;
    tree.remove(id)?;
    Ok(summarize(tree, 0))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            list_roots,
            start_scan,
            cancel_scan,
            get_layout,
            node_info,
            open_node,
            reveal_node,
            delete_node,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
