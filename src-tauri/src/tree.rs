use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub type NodeId = u32;

#[derive(Clone)]
pub struct Node {
    pub name: Box<str>,
    /// Bytes (allocated size on unix, apparent size elsewhere); directories hold the sum of their children.
    pub size: u64,
    /// Unix seconds, 0 when unknown.
    pub mtime: i64,
    pub parent: Option<NodeId>,
    /// Sorted by size, descending.
    pub children: Vec<NodeId>,
    pub is_dir: bool,
    /// Descendant file count (directories only).
    pub files: u64,
    /// Descendant directory count, excluding the node itself.
    pub dirs: u64,
}

/// A file with more than one hard link. Only the first link met during a scan counts its size.
#[derive(Clone, Copy)]
pub struct HardLink {
    pub dev: u64,
    pub ino: u64,
    /// The file's own size, even when this link counted as 0.
    pub size: u64,
}

#[derive(Clone)]
pub struct Tree {
    pub nodes: Vec<Node>,
    /// Hard-linked files, so an incremental rescan can count each one once.
    pub links: HashMap<NodeId, HardLink>,
    pub root_path: PathBuf,
    pub disk_total: u64,
    pub disk_free: u64,
    pub scan_ms: u64,
    /// Unix seconds when the scan started.
    pub scanned_at: i64,
    pub one_filesystem: bool,
    /// Loaded from a snapshot rather than scanned in this session.
    pub cached: bool,
}

pub const ROOT: NodeId = 0;

impl Tree {
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id as usize)
    }

    pub fn path_of(&self, id: NodeId) -> PathBuf {
        let mut parts = Vec::new();
        let mut cur = id;
        while let Some(parent) = self.nodes[cur as usize].parent {
            parts.push(&*self.nodes[cur as usize].name);
            cur = parent;
        }
        let mut path = self.root_path.clone();
        for part in parts.into_iter().rev() {
            path.push(part);
        }
        path
    }

    /// The node at `path`, if it is under the root and was scanned.
    pub fn find(&self, path: &Path) -> Option<NodeId> {
        let rel = path.strip_prefix(&self.root_path).ok()?;
        let mut cur = ROOT;
        for part in rel.components() {
            let name = part.as_os_str().to_string_lossy();
            cur = *self.nodes[cur as usize].children.iter().find(|&&c| *self.nodes[c as usize].name == *name)?;
        }
        Some(cur)
    }

    /// The `limit` largest files under `id`, biggest first.
    pub fn largest_files(&self, id: NodeId, limit: usize) -> Vec<NodeId> {
        use std::cmp::Reverse;
        use std::collections::BinaryHeap;
        // Min-heap of the best files so far; children are sorted by size, so a folder no bigger
        // than the smallest kept file (and every sibling after it) can be skipped.
        let mut best: BinaryHeap<Reverse<(u64, NodeId)>> = BinaryHeap::with_capacity(limit + 1);
        let mut stack = vec![id];
        if limit == 0 {
            return Vec::new();
        }
        while let Some(cur) = stack.pop() {
            let n = &self.nodes[cur as usize];
            let full = best.len() == limit;
            let floor = best.peek().map_or(0, |b| b.0 .0);
            if full && n.size <= floor {
                continue;
            }
            if !n.is_dir {
                best.push(Reverse((n.size, cur)));
                if best.len() > limit {
                    best.pop();
                }
                continue;
            }
            let keep: Vec<NodeId> =
                n.children.iter().copied().take_while(|&c| !full || self.nodes[c as usize].size > floor).collect();
            stack.extend(keep.into_iter().rev());
        }
        let mut out: Vec<_> = best.into_iter().map(|Reverse(b)| b).collect();
        out.sort_unstable_by(|a, b| b.cmp(a));
        out.into_iter().map(|(_, id)| id).collect()
    }

    /// Folders from just below `ancestor` down to the parent of `id`, top first.
    pub fn trail(&self, ancestor: NodeId, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut cur = self.nodes[id as usize].parent;
        while let Some(p) = cur {
            if p == ancestor {
                break;
            }
            out.push(p);
            cur = self.nodes[p as usize].parent;
        }
        out.reverse();
        out
    }

    /// Detaches `id` from the tree and subtracts its totals from every ancestor.
    pub fn remove(&mut self, id: NodeId) -> Result<(), String> {
        let (size, files, dirs, is_dir, parent) = {
            let n = self.node(id).ok_or("unknown node")?;
            (n.size, n.files, n.dirs, n.is_dir, n.parent)
        };
        let parent = parent.ok_or("cannot remove the scan root")?;
        self.nodes[parent as usize].children.retain(|&c| c != id);

        let (file_delta, dir_delta) = if is_dir { (files, dirs + 1) } else { (1, 0) };
        let mut cur = Some(parent);
        while let Some(p) = cur {
            let n = &mut self.nodes[p as usize];
            n.size = n.size.saturating_sub(size);
            n.files = n.files.saturating_sub(file_delta);
            n.dirs = n.dirs.saturating_sub(dir_delta);
            cur = n.parent;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str, size: u64, parent: Option<NodeId>, children: Vec<NodeId>) -> Node {
        let is_dir = !children.is_empty() || name.ends_with('/');
        Node { name: name.into(), size, mtime: 0, parent, children, is_dir, files: 0, dirs: 0 }
    }

    #[test]
    fn largest_files_and_trail() {
        // 0 root/ ─┬─ 1 a/ ─┬─ 3 big (50)
        //          │        └─ 4 mid (20)
        //          ├─ 2 small (25)
        //          └─ 5 b/ ── 6 tiny (5)
        let tree = Tree {
            nodes: vec![
                node("root/", 100, None, vec![1, 2, 5]),
                node("a/", 70, Some(0), vec![3, 4]),
                node("small", 25, Some(0), vec![]),
                node("big", 50, Some(1), vec![]),
                node("mid", 20, Some(1), vec![]),
                node("b/", 5, Some(0), vec![6]),
                node("tiny", 5, Some(5), vec![]),
            ],
            links: HashMap::new(),
            root_path: PathBuf::from("/r"),
            disk_total: 0,
            disk_free: 0,
            scan_ms: 0,
            scanned_at: 0,
            one_filesystem: false,
            cached: false,
        };
        assert_eq!(tree.largest_files(0, 2), vec![3, 2]);
        assert_eq!(tree.largest_files(0, 10), vec![3, 2, 4, 6]);
        assert_eq!(tree.largest_files(1, 1), vec![3]);
        assert!(tree.largest_files(0, 0).is_empty());
        assert_eq!(tree.trail(0, 3), vec![1]);
        assert!(tree.trail(1, 3).is_empty());
        assert_eq!(tree.find(Path::new("/r/small")), Some(2));
        assert_eq!(tree.find(Path::new("/r")), Some(0));
        assert_eq!(tree.find(Path::new("/r/nope")), None);
        assert_eq!(tree.find(Path::new("/elsewhere")), None);
    }
}
