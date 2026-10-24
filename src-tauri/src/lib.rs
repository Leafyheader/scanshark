mod layout;
mod report;
mod scan;
mod snapshot;
mod tree;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use sysinfo::Disks;
use tauri::{AppHandle, Emitter, Manager, State};

use layout::Rect;
use tree::{NodeId, Tree, ROOT};

#[derive(Default)]
struct AppState {
    /// Shared with a running scan, which reuses it, and with the thread saving its snapshot.
    tree: Mutex<Option<Arc<Tree>>>,
    cancel: Arc<AtomicBool>,
    scanning: AtomicBool,
    /// The last report written, which the app can offer to open.
    last_export: Mutex<Option<PathBuf>>,
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
    scanned_at: i64,
    /// Loaded from a snapshot; a fresh scan hasn't replaced it yet.
    cached: bool,
    largest_file: Option<TopFile>,
    /// The biggest folder directly under the root.
    largest_folder: Option<Folder>,
}

#[derive(Serialize, Clone)]
struct Folder {
    id: NodeId,
    name: String,
    size: u64,
}

#[derive(Serialize)]
struct NodeInfo {
    id: NodeId,
    path: String,
    name: String,
    size: u64,
    mtime: i64,
    is_dir: bool,
    files: u64,
    dirs: u64,
    parent_size: u64,
}

#[derive(Serialize, Clone)]
struct TopFile {
    id: NodeId,
    name: String,
    /// Folder holding the file, relative to the folder that was asked about.
    dir: String,
    size: u64,
    mtime: i64,
    /// Folders to zoom through to reach the file's folder, top first.
    trail: Vec<NodeId>,
}

/// A Search result.
#[derive(Serialize)]
struct Hit {
    id: NodeId,
    name: String,
    /// Folder holding the item, relative to the root.
    dir: String,
    size: u64,
    is_dir: bool,
    /// Folders to zoom through from the root to reach the item's folder, top first.
    trail: Vec<NodeId>,
}

#[derive(Serialize)]
struct SearchResults {
    hits: Vec<Hit>,
    /// Matches in all, which can be more than `hits` holds.
    total: usize,
}

/// One row of the List view.
#[derive(Serialize)]
struct Child {
    id: NodeId,
    name: String,
    size: u64,
    mtime: i64,
    is_dir: bool,
    files: u64,
    dirs: u64,
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

/// File `f` as a Top Files row, relative to folder `id`.
fn top_file(tree: &Tree, id: NodeId, f: NodeId) -> TopFile {
    let n = &tree.nodes[f as usize];
    let trail = tree.trail(id, f);
    let dir: Vec<&str> = trail.iter().map(|&d| &*tree.nodes[d as usize].name).collect();
    TopFile {
        id: f,
        name: n.name.to_string(),
        dir: dir.join(std::path::MAIN_SEPARATOR_STR),
        size: n.size,
        mtime: n.mtime,
        trail,
    }
}

fn summarize(tree: &Tree) -> Summary {
    let root = &tree.nodes[ROOT as usize];
    // Children are sorted by size, so the first folder is the biggest.
    let largest_folder = root.children.iter().map(|&c| (c, &tree.nodes[c as usize])).find(|(_, n)| n.is_dir);
    Summary {
        root_path: tree.root_path.to_string_lossy().into_owned(),
        total_size: root.size,
        files: root.files,
        dirs: root.dirs,
        disk_total: tree.disk_total,
        disk_free: tree.disk_free,
        scan_ms: tree.scan_ms,
        scanned_at: tree.scanned_at,
        cached: tree.cached,
        largest_file: tree.largest_files(ROOT, 1).first().map(|&f| top_file(tree, ROOT, f)),
        largest_folder: largest_folder.map(|(id, n)| Folder { id, name: n.name.to_string(), size: n.size }),
    }
}

fn snapshot_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_cache_dir().map(|d| d.join("snapshots")).map_err(|e| e.to_string())
}

/// FAT and exFAT don't reliably update a folder's modified time when its contents change,
/// so incremental rescans can't be trusted there.
fn folder_times_reliable(path: &Path) -> bool {
    let path = if cfg!(windows) { path.to_path_buf() } else { path.canonicalize().unwrap_or_else(|_| path.to_path_buf()) };
    Disks::new_with_refreshed_list()
        .list()
        .iter()
        .filter(|d| path.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .is_none_or(|d| !d.file_system().to_string_lossy().to_lowercase().contains("fat"))
}

/// Total and free bytes of the filesystem holding `path`.
#[cfg(windows)]
fn disk_space(path: &Path) -> (u64, u64) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    // Asking Windows directly avoids matching `\\?\C:\...` (what canonicalize returns) against `C:\`.
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let (mut free, mut total) = (0u64, 0u64);
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, &mut total, std::ptr::null_mut()) };
    if ok == 0 { (0, 0) } else { (total, free) }
}

/// Total and free bytes of the filesystem holding `path`.
#[cfg(not(windows))]
fn disk_space(path: &Path) -> (u64, u64) {
    let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    Disks::new_with_refreshed_list()
        .list()
        .iter()
        .filter(|d| canon.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map_or((0, 0), |d| (d.total_space(), d.available_space()))
}

/// Mount points of other drives inside `root`, which a scan of `root` leaves to their own entry.
/// Btrfs subvolumes such as /home share the root's device and are kept.
#[cfg(not(windows))]
fn other_drives(root: &Path) -> std::collections::HashSet<PathBuf> {
    let canon = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let disks = Disks::new_with_refreshed_list();
    let Some(own) = disks
        .list()
        .iter()
        .filter(|d| canon.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
    else {
        return Default::default();
    };
    disks
        .list()
        .iter()
        .filter(|d| d.name() != own.name() && d.mount_point() != canon && d.mount_point().starts_with(&canon))
        // Scanned paths are built from `root` as given, which may differ from its canonical form.
        .map(|d| root.join(d.mount_point().strip_prefix(&canon).unwrap()))
        .collect()
}
#[cfg(windows)]
fn other_drives(_: &Path) -> std::collections::HashSet<PathBuf> {
    Default::default()
}

/// A readable name for a mounted drive: "System" for /, "Windows" for a Windows partition,
/// otherwise the mount folder's name (the volume label, or its ID when it has none).
#[cfg(not(windows))]
fn drive_label(mount: &Path) -> String {
    if mount == Path::new("/") {
        "System".into()
    } else if mount.join("Windows").join("System32").is_dir() {
        "Windows".into()
    } else {
        mount.file_name().map_or_else(|| mount.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned())
    }
}
#[cfg(windows)]
fn drive_label(mount: &Path) -> String {
    mount.to_string_lossy().into_owned()
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
        // Desktops mount other drives under /run/media/<user>; the rest of /run is runtime state.
        let pseudo = matches!(fs.as_str(), "tmpfs" | "devtmpfs" | "squashfs" | "overlay" | "efivarfs")
            || (mount.starts_with("/run") && !mount.starts_with("/run/media/"))
            || mount.starts_with("/snap")
            || mount.starts_with("/var/lib/snapd")
            || mount.starts_with("/boot");
        if d.total_space() == 0 || pseudo || roots.iter().any(|r| r.path == mount) {
            continue;
        }
        let label = drive_label(d.mount_point());
        roots.push(Root { path: mount, label, total: d.total_space(), free: d.available_space() });
    }
    roots
}

/// Shows the saved snapshot of `path`, if there is one, so there is something to look at while
/// it is scanned again.
#[tauri::command]
async fn open_cached(app: AppHandle, state: State<'_, AppState>, path: String) -> Result<Option<Summary>, String> {
    if state.scanning.load(Ordering::SeqCst) {
        return Err("a scan is already running".into());
    }
    let dir = snapshot_dir(&app)?;
    let tree = tauri::async_runtime::spawn_blocking(move || snapshot::load(&dir, Path::new(&path)))
        .await
        .map_err(|e| e.to_string())?;
    let Some(tree) = tree else { return Ok(None) };
    let summary = summarize(&tree);
    *state.tree.lock().unwrap() = Some(Arc::new(tree));
    Ok(Some(summary))
}

/// Scans `path`, reusing unchanged folders from the tree on show when it is of the same root,
/// unless `full` is set. The result replaces that tree and is saved as a snapshot.
#[tauri::command]
async fn start_scan(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    one_filesystem: bool,
    full: bool,
) -> Result<Option<Summary>, String> {
    if state.scanning.swap(true, Ordering::SeqCst) {
        return Err("a scan is already running".into());
    }
    state.cancel.store(false, Ordering::SeqCst);
    let root = PathBuf::from(&path);
    let prev = if full {
        None
    } else {
        let current = state.tree.lock().unwrap().clone();
        current.filter(|t| t.root_path == root && t.one_filesystem == one_filesystem && folder_times_reliable(&root))
    };

    let cancel = state.cancel.clone();
    let started = Instant::now();
    let scanned_at = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let result = {
        let root = root.clone();
        with_progress(&app, move |progress| {
            let opts = scan::Options { one_filesystem, skip: other_drives(&root), ..Default::default() };
            scan::scan(&root, &opts, progress, &cancel, prev.as_deref())
        })
        .await
    };
    state.scanning.store(false, Ordering::SeqCst);

    let Some(scanned) = result?? else { return Ok(None) };
    let (disk_total, disk_free) = disk_space(&root);
    let tree = Arc::new(Tree {
        nodes: scanned.nodes,
        links: scanned.links,
        root_path: root,
        disk_total,
        disk_free,
        scan_ms: started.elapsed().as_millis() as u64,
        scanned_at,
        one_filesystem,
        cached: false,
    });
    let summary = summarize(&tree);
    *state.tree.lock().unwrap() = Some(tree.clone());
    save_snapshot(&app, tree)?;
    Ok(Some(summary))
}

/// Scans folder `id` again in full and puts the result in place of its old contents, leaving
/// the rest of the tree as it was. Returns `None` if cancelled.
#[tauri::command]
async fn rescan_node(app: AppHandle, state: State<'_, AppState>, id: NodeId) -> Result<Option<Summary>, String> {
    let (path, root, one_filesystem, counted) = {
        let guard = state.tree.lock().unwrap();
        let tree = guard.as_ref().ok_or("nothing scanned yet")?;
        if !tree.node(id).ok_or("unknown node")?.is_dir {
            return Err("only folders can be rescanned".into());
        }
        (tree.path_of(id), tree.root_path.clone(), tree.one_filesystem, tree.links_counted_outside(id))
    };
    if state.scanning.swap(true, Ordering::SeqCst) {
        return Err("a scan is already running".into());
    }
    state.cancel.store(false, Ordering::SeqCst);

    let cancel = state.cancel.clone();
    let result = {
        let root = root.clone();
        with_progress(&app, move |progress| {
            let opts = scan::Options { one_filesystem, skip: other_drives(&root), counted };
            scan::scan(&path, &opts, progress, &cancel, None)
        })
        .await
    };
    state.scanning.store(false, Ordering::SeqCst);

    let Some(scanned) = result?? else { return Ok(None) };
    let (disk_total, disk_free) = disk_space(&root);
    let (summary, tree) = {
        let mut guard = state.tree.lock().unwrap();
        // Copies the tree if its snapshot is still being written.
        let tree = Arc::make_mut(guard.as_mut().ok_or("nothing scanned yet")?);
        tree.graft(id, scanned.nodes, scanned.links);
        tree.disk_total = disk_total;
        tree.disk_free = disk_free;
        (summarize(tree), guard.clone().unwrap())
    };
    save_snapshot(&app, tree)?;
    Ok(Some(summary))
}

/// Runs `work` on the blocking pool, sending its progress to the app every 100 ms meanwhile.
async fn with_progress<T: Send + 'static>(
    app: &AppHandle,
    work: impl FnOnce(&scan::Progress) -> T + Send + 'static,
) -> Result<T, String> {
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

    let result = {
        let progress = progress.clone();
        tauri::async_runtime::spawn_blocking(move || work(&progress)).await.map_err(|e| e.to_string())
    };
    finished.store(true, Ordering::Relaxed);
    let _ = reporter.join();
    result
}

/// Saves `tree` as the snapshot of its root, on a thread of its own.
fn save_snapshot(app: &AppHandle, tree: Arc<Tree>) -> Result<(), String> {
    let dir = snapshot_dir(app)?;
    std::thread::spawn(move || {
        if let Err(e) = snapshot::save(&dir, &tree) {
            eprintln!("saving snapshot of {}: {e}", tree.root_path.display());
        }
    });
    Ok(())
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
        id,
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

#[tauri::command]
fn top_files(state: State<'_, AppState>, id: NodeId, limit: usize) -> Result<Vec<TopFile>, String> {
    let guard = state.tree.lock().unwrap();
    let tree = guard.as_ref().ok_or("nothing scanned yet")?;
    tree.node(id).ok_or("unknown node")?;
    Ok(tree.largest_files(id, limit).into_iter().map(|f| top_file(tree, id, f)).collect())
}

/// The items directly inside folder `id`, biggest first.
#[tauri::command]
fn children(state: State<'_, AppState>, id: NodeId) -> Result<Vec<Child>, String> {
    let guard = state.tree.lock().unwrap();
    let tree = guard.as_ref().ok_or("nothing scanned yet")?;
    let n = tree.node(id).ok_or("unknown node")?;
    Ok(n.children
        .iter()
        .map(|&c| {
            let n = &tree.nodes[c as usize];
            Child {
                id: c,
                name: n.name.to_string(),
                size: n.size,
                mtime: n.mtime,
                is_dir: n.is_dir,
                files: n.files,
                dirs: n.dirs,
            }
        })
        .collect())
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
    if state.scanning.load(Ordering::SeqCst) {
        return Err("wait for the scan to finish before deleting".into());
    }
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
    // Copies the tree if its snapshot is still being written.
    let tree = Arc::make_mut(guard.as_mut().ok_or("nothing scanned yet")?);
    tree.remove(id)?;
    Ok(summarize(tree))
}

/// The nodes at `paths` in the current tree, so the view can keep its place after a rescan.
#[tauri::command]
fn locate(state: State<'_, AppState>, paths: Vec<String>) -> Result<Vec<Option<NodeId>>, String> {
    let guard = state.tree.lock().unwrap();
    let tree = guard.as_ref().ok_or("nothing scanned yet")?;
    Ok(paths.iter().map(|p| tree.find(Path::new(p))).collect())
}

/// Items whose name matches `query`, biggest first. Async so a big tree is searched off the UI thread.
#[tauri::command]
async fn search(state: State<'_, AppState>, query: String, limit: usize) -> Result<SearchResults, String> {
    let tree = state.tree.lock().unwrap().clone().ok_or("nothing scanned yet")?;
    let (ids, total) = tree.search(&query, limit);
    let hits = ids
        .into_iter()
        .map(|id| {
            let n = &tree.nodes[id as usize];
            let trail = tree.trail(ROOT, id);
            let dir: Vec<&str> = trail.iter().map(|&d| &*tree.nodes[d as usize].name).collect();
            Hit {
                id,
                name: n.name.to_string(),
                dir: dir.join(std::path::MAIN_SEPARATOR_STR),
                size: n.size,
                is_dir: n.is_dir,
                trail,
            }
        })
        .collect();
    Ok(SearchResults { hits, total })
}

/// Writes a report of folder `id` to `path`: "html", "csv" (its items) or "csv-all" (everything below it).
#[tauri::command]
async fn export_report(
    state: State<'_, AppState>,
    id: NodeId,
    path: String,
    format: String,
    decimal: bool,
) -> Result<(), String> {
    let tree = state.tree.lock().unwrap().clone().ok_or("nothing scanned yet")?;
    tree.node(id).ok_or("unknown node")?;
    let path = PathBuf::from(path);
    let file = std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut w = std::io::BufWriter::new(file);
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    match format.as_str() {
        "html" => report::write_html(&tree, id, decimal, now, &mut w),
        "csv" => report::write_csv(&tree, id, false, decimal, &mut w).map(drop),
        "csv-all" => report::write_csv(&tree, id, true, decimal, &mut w).map(drop),
        _ => return Err(format!("unknown report format {format}")),
    }
    .and_then(|()| std::io::Write::flush(&mut w))
    .map_err(|e| format!("{}: {e}", path.display()))?;
    *state.last_export.lock().unwrap() = Some(path);
    Ok(())
}

/// Opens the last report written, in the default app for its type.
#[tauri::command]
fn open_export(state: State<'_, AppState>) -> Result<(), String> {
    let path = state.last_export.lock().unwrap().clone().ok_or("no report written yet")?;
    tauri_plugin_opener::open_path(path, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
fn cache_size(app: AppHandle) -> Result<u64, String> {
    Ok(snapshot::total_size(&snapshot_dir(&app)?))
}

#[tauri::command]
fn clear_cache(app: AppHandle) -> Result<(), String> {
    snapshot::clear(&snapshot_dir(&app)?).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            list_roots,
            open_cached,
            start_scan,
            rescan_node,
            cancel_scan,
            get_layout,
            node_info,
            top_files,
            children,
            open_node,
            reveal_node,
            delete_node,
            locate,
            search,
            export_report,
            open_export,
            cache_size,
            clear_cache,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
