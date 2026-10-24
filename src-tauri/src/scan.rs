use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use rayon::prelude::*;

use crate::tree::{HardLink, Node, NodeId, Tree, ROOT};

/// Live counters shared with the progress reporter.
#[derive(Default)]
pub struct Progress {
    pub files: AtomicU64,
    pub dirs: AtomicU64,
    pub bytes: AtomicU64,
    pub current: Mutex<String>,
}

pub struct Options {
    pub one_filesystem: bool,
}

pub struct Scanned {
    pub nodes: Vec<Node>,
    pub links: HashMap<NodeId, HardLink>,
}

struct Entry {
    name: String,
    size: u64,
    mtime: i64,
    is_dir: bool,
    files: u64,
    dirs: u64,
    link: Option<HardLink>,
    children: Vec<Entry>,
}

impl Entry {
    fn dir(name: String, mtime: i64) -> Self {
        Entry { name, size: 0, mtime, is_dir: true, files: 0, dirs: 0, link: None, children: Vec::new() }
    }
    fn file(name: String, size: u64, mtime: i64, link: Option<HardLink>) -> Self {
        Entry { name, size, mtime, is_dir: false, files: 0, dirs: 0, link, children: Vec::new() }
    }
}

/// A folder still to scan: path, name, mtime, and its node in the previous scan.
type Subdir = (PathBuf, String, i64, Option<NodeId>);

struct Ctx<'a> {
    progress: &'a Progress,
    cancel: &'a AtomicBool,
    opts: &'a Options,
    root_dev: u64,
    seen_links: Mutex<HashSet<(u64, u64)>>,
    prev: Option<&'a Tree>,
    /// Folders modified after this (unix seconds) may have changed again within the same second
    /// the previous scan read them, so they are listed again.
    prev_trusted_until: i64,
}

/// Scans `root` in parallel. Returns `None` if cancelled.
///
/// With `prev`, an earlier scan of the same root, folders whose modification time hasn't changed
/// since are rebuilt from it instead of being listed: adding, removing or renaming an entry
/// updates a folder's time. Their subfolders are still visited, and files that changed in place
/// keep their old size.
pub fn scan(
    root: &Path,
    opts: &Options,
    progress: &Progress,
    cancel: &AtomicBool,
    prev: Option<&Tree>,
) -> Result<Option<Scanned>, String> {
    let meta = fs::symlink_metadata(root).map_err(|e| format!("{}: {e}", root.display()))?;
    if !meta.is_dir() {
        return Err(format!("{} is not a directory", root.display()));
    }
    let ctx = Ctx {
        progress,
        cancel,
        opts,
        root_dev: device_of(&meta),
        seen_links: Mutex::new(HashSet::new()),
        prev,
        prev_trusted_until: prev.map_or(0, |t| t.scanned_at - 2),
    };

    let threads = std::thread::available_parallelism().map_or(8, |n| n.get() * 2).max(8);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .stack_size(16 * 1024 * 1024)
        .build()
        .map_err(|e| e.to_string())?;

    let mut entry = pool.install(|| scan_dir(root, root.to_string_lossy().into_owned(), mtime_of(&meta), prev.map(|_| ROOT), &ctx));
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }

    entry.name = root.to_string_lossy().into_owned();
    let mut out = Scanned {
        nodes: Vec::with_capacity((progress.files.load(Ordering::Relaxed) + progress.dirs.load(Ordering::Relaxed) + 1) as usize),
        links: HashMap::new(),
    };
    flatten(entry, None, &mut out);
    Ok(Some(out))
}

fn scan_dir(path: &Path, name: String, mtime: i64, cached: Option<NodeId>, ctx: &Ctx) -> Entry {
    let mut dir = Entry::dir(name, mtime);
    if ctx.cancel.load(Ordering::Relaxed) {
        return dir;
    }
    ctx.progress.dirs.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut cur) = ctx.progress.current.try_lock() {
        *cur = path.to_string_lossy().into_owned();
    }

    let subdirs = match cached.and_then(|c| reuse(path, mtime, c, &mut dir, ctx)) {
        Some(subdirs) => subdirs,
        None => list(path, cached, &mut dir, ctx),
    };

    let sub: Vec<Entry> = subdirs
        .into_par_iter()
        .map(|(p, name, mtime, cached)| scan_dir(&p, name, mtime, cached, ctx))
        .collect();
    dir.children.extend(sub);

    dir.children.sort_unstable_by(|a, b| b.size.cmp(&a.size));
    for c in &dir.children {
        dir.size += c.size;
        if c.is_dir {
            dir.files += c.files;
            dir.dirs += c.dirs + 1;
        } else {
            dir.files += 1;
        }
    }
    dir
}

/// Reads the folder at `path`, adding its files to `dir` and returning its subfolders.
fn list(path: &Path, cached: Option<NodeId>, dir: &mut Entry, ctx: &Ctx) -> Vec<Subdir> {
    let mut subdirs = Vec::new();
    let Ok(read) = fs::read_dir(path) else { return subdirs };
    // This folder's subfolders in the previous scan, so unchanged ones further down can be reused.
    let old: HashMap<&str, NodeId> = match (ctx.prev, cached) {
        (Some(prev), Some(c)) => prev.nodes[c as usize]
            .children
            .iter()
            .filter(|&&k| prev.nodes[k as usize].is_dir)
            .map(|&k| (&*prev.nodes[k as usize].name, k))
            .collect(),
        _ => HashMap::new(),
    };
    for item in read.flatten() {
        let Ok(meta) = item.metadata() else { continue }; // does not follow symlinks
        let name = item.file_name().to_string_lossy().into_owned();
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            let child = item.path();
            if skip_dir(&child, &meta, ctx) {
                continue;
            }
            let cached = old.get(name.as_str()).copied();
            subdirs.push((child, name, mtime_of(&meta), cached));
        } else if meta.is_file() {
            let (size, link) = file_size(&meta, ctx);
            add_file(dir, Entry::file(name, size, mtime_of(&meta), link), ctx);
        }
    }
    subdirs
}

/// Rebuilds an unchanged folder from the previous scan without listing it, adding its files to
/// `dir` and returning its subfolders. Returns `None`, leaving `dir` alone, if it may have changed.
fn reuse(path: &Path, mtime: i64, cached: NodeId, dir: &mut Entry, ctx: &Ctx) -> Option<Vec<Subdir>> {
    let prev = ctx.prev?;
    let old = &prev.nodes[cached as usize];
    if mtime == 0 || mtime != old.mtime || mtime > ctx.prev_trusted_until {
        return None;
    }
    let mut subdirs = Vec::new();
    for &c in &old.children {
        let n = &prev.nodes[c as usize];
        if n.is_dir {
            let p = path.join(&*n.name);
            let meta = fs::symlink_metadata(&p).ok()?;
            if !meta.is_dir() {
                return None;
            }
            if !skip_dir(&p, &meta, ctx) {
                subdirs.push((p, n.name.to_string(), mtime_of(&meta), Some(c)));
            }
        }
    }
    for &c in &old.children {
        let n = &prev.nodes[c as usize];
        if !n.is_dir {
            let link = prev.links.get(&c).copied();
            let size = link.map_or(n.size, |l| counted(&l, ctx));
            add_file(dir, Entry::file(n.name.to_string(), size, n.mtime, link), ctx);
        }
    }
    Some(subdirs)
}

fn add_file(dir: &mut Entry, file: Entry, ctx: &Ctx) {
    ctx.progress.files.fetch_add(1, Ordering::Relaxed);
    ctx.progress.bytes.fetch_add(file.size, Ordering::Relaxed);
    dir.children.push(file);
}

/// Moves the entry tree into the arena, returning the id of `entry`.
fn flatten(entry: Entry, parent: Option<NodeId>, out: &mut Scanned) -> NodeId {
    let nodes = &mut out.nodes;
    let id = nodes.len() as NodeId;
    if let Some(link) = entry.link {
        out.links.insert(id, link);
    }
    nodes.push(Node {
        name: entry.name.into_boxed_str(),
        size: entry.size,
        mtime: entry.mtime,
        parent,
        children: Vec::with_capacity(entry.children.len()),
        is_dir: entry.is_dir,
        files: entry.files,
        dirs: entry.dirs,
    });
    for child in entry.children {
        let cid = flatten(child, Some(id), out);
        out.nodes[id as usize].children.push(cid);
    }
    id
}

fn mtime_of(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64)
}

fn skip_dir(path: &Path, meta: &fs::Metadata, ctx: &Ctx) -> bool {
    if ctx.opts.one_filesystem && device_of(meta) != ctx.root_dev {
        return true;
    }
    #[cfg(target_os = "linux")]
    if matches!(path.to_str(), Some("/proc" | "/sys" | "/dev")) {
        return true;
    }
    let _ = path;
    false
}

#[cfg(unix)]
fn device_of(meta: &fs::Metadata) -> u64 {
    std::os::unix::fs::MetadataExt::dev(meta)
}
#[cfg(not(unix))]
fn device_of(_: &fs::Metadata) -> u64 {
    0
}

/// The size a hard-linked file counts for: its own the first time it is met, 0 after that.
fn counted(link: &HardLink, ctx: &Ctx) -> u64 {
    if ctx.seen_links.lock().unwrap().insert((link.dev, link.ino)) { link.size } else { 0 }
}

#[cfg(unix)]
fn file_size(meta: &fs::Metadata, ctx: &Ctx) -> (u64, Option<HardLink>) {
    use std::os::unix::fs::MetadataExt;
    let size = meta.blocks() * 512;
    if meta.nlink() > 1 {
        let link = HardLink { dev: meta.dev(), ino: meta.ino(), size };
        return (counted(&link, ctx), Some(link));
    }
    (size, None)
}
#[cfg(windows)]
fn file_size(meta: &fs::Metadata, _: &Ctx) -> (u64, Option<HardLink>) {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;
    const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;
    // Cloud-only placeholders (OneDrive "files on demand") report their full size but take no local space.
    if meta.file_attributes() & (FILE_ATTRIBUTE_OFFLINE | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS) != 0 {
        return (0, None);
    }
    (meta.len(), None)
}
#[cfg(not(any(unix, windows)))]
fn file_size(meta: &fs::Metadata, _: &Ctx) -> (u64, Option<HardLink>) {
    (meta.len(), None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    fn scan_tree(dir: &Path, prev: Option<&Tree>, scanned_at: i64) -> Tree {
        let progress = Progress::default();
        let cancel = AtomicBool::new(false);
        let s = scan(dir, &Options { one_filesystem: false }, &progress, &cancel, prev).unwrap().unwrap();
        Tree {
            nodes: s.nodes,
            links: s.links,
            root_path: dir.to_path_buf(),
            disk_total: 0,
            disk_free: 0,
            scan_ms: 0,
            scanned_at,
            one_filesystem: false,
            cached: false,
        }
    }

    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("scanshark-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("a/b")).unwrap();
        fs::write(dir.join("a/b/big.bin"), vec![1u8; 100_000]).unwrap();
        fs::write(dir.join("a/small.txt"), b"hi").unwrap();
        fs::write(dir.join("top.txt"), b"x").unwrap();
        dir
    }

    #[test]
    fn scans_a_directory() {
        let dir = test_dir("scan");
        let tree = scan_tree(&dir, None, 0);

        let root = tree.node(0).unwrap();
        assert_eq!(root.files, 3);
        assert_eq!(root.dirs, 2);
        assert!(root.size >= 100_000);
        // biggest child first
        assert_eq!(&*tree.node(root.children[0]).unwrap().name, "a");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rescans_only_changed_folders() {
        let dir = test_dir("rescan");
        // Pretend the first scan ran well after everything was written, so its folders are trusted.
        let first = scan_tree(&dir, None, i64::MAX / 2);
        let size_of = |t: &Tree, p: &str| t.node(t.find(&dir.join(p)).unwrap()).unwrap().size;
        let top_before = size_of(&first, "top.txt");

        // Growing a file in place leaves its folder's time alone, so the root is reused as is.
        fs::write(dir.join("top.txt"), vec![1u8; 200_000]).unwrap();
        // Adding a file changes a/b's time; back-date it so it differs within the same second too.
        fs::write(dir.join("a/b/new.bin"), vec![1u8; 50_000]).unwrap();
        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        File::open(dir.join("a/b")).unwrap().set_modified(old).unwrap();

        let second = scan_tree(&dir, Some(&first), 0);
        assert_eq!(size_of(&second, "top.txt"), top_before);
        assert!(second.find(&dir.join("a/b/new.bin")).is_some());
        assert_eq!(second.node(0).unwrap().files, 4);

        // Without a previous scan everything is measured again.
        let full = scan_tree(&dir, None, 0);
        assert!(size_of(&full, "top.txt") >= 200_000);
        let _ = fs::remove_dir_all(&dir);
    }
}
