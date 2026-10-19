use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use rayon::prelude::*;

use crate::tree::{Node, NodeId, Tree};

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

struct Entry {
    name: String,
    size: u64,
    mtime: i64,
    is_dir: bool,
    files: u64,
    dirs: u64,
    children: Vec<Entry>,
}

struct Ctx<'a> {
    progress: &'a Progress,
    cancel: &'a AtomicBool,
    opts: &'a Options,
    root_dev: u64,
    seen_links: Mutex<HashSet<(u64, u64)>>,
}

/// Scans `root` in parallel. Returns `None` if cancelled.
pub fn scan(
    root: &Path,
    opts: &Options,
    progress: &Progress,
    cancel: &AtomicBool,
) -> Result<Option<Vec<Node>>, String> {
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
    };

    let threads = std::thread::available_parallelism().map_or(8, |n| n.get() * 2).max(8);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .stack_size(16 * 1024 * 1024)
        .build()
        .map_err(|e| e.to_string())?;

    let mut entry = pool.install(|| scan_dir(root, root.to_string_lossy().into_owned(), mtime_of(&meta), &ctx));
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }

    entry.name = root.to_string_lossy().into_owned();
    let mut nodes = Vec::with_capacity((progress.files.load(Ordering::Relaxed) + progress.dirs.load(Ordering::Relaxed) + 1) as usize);
    flatten(entry, None, &mut nodes);
    Ok(Some(nodes))
}

fn scan_dir(path: &Path, name: String, mtime: i64, ctx: &Ctx) -> Entry {
    let mut dir = Entry { name, size: 0, mtime, is_dir: true, files: 0, dirs: 0, children: Vec::new() };
    if ctx.cancel.load(Ordering::Relaxed) {
        return dir;
    }
    ctx.progress.dirs.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut cur) = ctx.progress.current.try_lock() {
        *cur = path.to_string_lossy().into_owned();
    }

    let Ok(read) = fs::read_dir(path) else { return dir };
    let mut subdirs: Vec<(PathBuf, String, i64)> = Vec::new();
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
            subdirs.push((child, name, mtime_of(&meta)));
        } else if meta.is_file() {
            let size = file_size(&meta, ctx);
            ctx.progress.files.fetch_add(1, Ordering::Relaxed);
            ctx.progress.bytes.fetch_add(size, Ordering::Relaxed);
            dir.children.push(Entry { name, size, mtime: mtime_of(&meta), is_dir: false, files: 0, dirs: 0, children: Vec::new() });
        }
    }

    let sub: Vec<Entry> = subdirs
        .into_par_iter()
        .map(|(p, name, mtime)| scan_dir(&p, name, mtime, ctx))
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

/// Moves the entry tree into the arena, returning the id of `entry`.
fn flatten(entry: Entry, parent: Option<NodeId>, nodes: &mut Vec<Node>) -> NodeId {
    let id = nodes.len() as NodeId;
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
        let cid = flatten(child, Some(id), nodes);
        nodes[id as usize].children.push(cid);
    }
    id
}

pub fn build_tree(nodes: Vec<Node>, root: &Path, disk_total: u64, disk_free: u64) -> Tree {
    Tree { nodes, root_path: root.to_path_buf(), disk_total, disk_free }
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

#[cfg(unix)]
fn file_size(meta: &fs::Metadata, ctx: &Ctx) -> u64 {
    use std::os::unix::fs::MetadataExt;
    if meta.nlink() > 1 {
        let mut seen = ctx.seen_links.lock().unwrap();
        if !seen.insert((meta.dev(), meta.ino())) {
            return 0; // hard link already counted
        }
    }
    meta.blocks() * 512
}
#[cfg(not(unix))]
fn file_size(meta: &fs::Metadata, _: &Ctx) -> u64 {
    meta.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_a_directory() {
        let dir = std::env::temp_dir().join(format!("scanshark-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("a/b")).unwrap();
        fs::write(dir.join("a/b/big.bin"), vec![1u8; 100_000]).unwrap();
        fs::write(dir.join("a/small.txt"), b"hi").unwrap();
        fs::write(dir.join("top.txt"), b"x").unwrap();

        let progress = Progress::default();
        let cancel = AtomicBool::new(false);
        let nodes = scan(&dir, &Options { one_filesystem: false }, &progress, &cancel).unwrap().unwrap();
        let tree = build_tree(nodes, &dir, 0, 0);

        let root = tree.node(0).unwrap();
        assert_eq!(root.files, 3);
        assert_eq!(root.dirs, 2);
        assert!(root.size >= 100_000);
        // biggest child first
        assert_eq!(&*tree.node(root.children[0]).unwrap().name, "a");
        let _ = fs::remove_dir_all(&dir);
    }
}
