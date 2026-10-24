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

    /// Items under the root whose name matches `query`, biggest first, at most `limit` of them,
    /// with the number of matches in all. A query with `*` or `?` is a wildcard pattern for the
    /// whole name; otherwise any name containing it matches. Case is ignored either way.
    pub fn search(&self, query: &str, limit: usize) -> (Vec<NodeId>, usize) {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return (Vec::new(), 0);
        }
        let pattern: Vec<char> = query.chars().collect();
        let wildcard = query.contains(['*', '?']);
        let mut hits: Vec<(u64, NodeId)> = Vec::new();
        // Walk from the root rather than over `nodes`, which still holds deleted items.
        let mut stack: Vec<NodeId> = self.nodes[ROOT as usize].children.clone();
        while let Some(id) = stack.pop() {
            let n = &self.nodes[id as usize];
            let name = n.name.to_lowercase();
            let found = if wildcard { glob(&pattern, &name.chars().collect::<Vec<_>>()) } else { name.contains(&query) };
            if found {
                hits.push((n.size, id));
            }
            stack.extend_from_slice(&n.children);
        }
        let total = hits.len();
        hits.sort_unstable_by(|a, b| b.cmp(a));
        hits.truncate(limit);
        (hits.into_iter().map(|(_, id)| id).collect(), total)
    }

    /// Hard-linked files (device, inode) whose size is counted by a link outside folder `id`.
    pub fn links_counted_outside(&self, id: NodeId) -> std::collections::HashSet<(u64, u64)> {
        let mut out = std::collections::HashSet::new();
        if self.links.is_empty() {
            return out;
        }
        // Walk from the root rather than over `links`, which still holds deleted items.
        let mut stack = vec![ROOT];
        while let Some(cur) = stack.pop() {
            let n = &self.nodes[cur as usize];
            if cur == id {
                continue;
            }
            // A link that counted 0 left its size to another one, which may be inside `id`.
            if let Some(l) = self.links.get(&cur).filter(|_| n.size > 0) {
                out.insert((l.dev, l.ino));
            }
            stack.extend_from_slice(&n.children);
        }
        out
    }

    /// Puts `nodes`, a fresh scan of folder `id` with its own root at 0, in place of that folder's
    /// contents and corrects every ancestor's totals. `id` keeps its number; the old contents stay
    /// in `nodes`, unreachable, like deleted items.
    pub fn graft(&mut self, id: NodeId, nodes: Vec<Node>, links: HashMap<NodeId, HardLink>) {
        let base = self.nodes.len() as NodeId - 1;
        let map = |k: NodeId| if k == ROOT { id } else { base + k };
        let mut nodes = nodes.into_iter();
        let new_root = nodes.next().expect("a scan always holds its root");
        for mut n in nodes {
            n.parent = n.parent.map(map);
            n.children.iter_mut().for_each(|c| *c = map(*c));
            self.nodes.push(n);
        }
        self.links.extend(links.into_iter().map(|(k, l)| (map(k), l)));

        let old = &mut self.nodes[id as usize];
        let (old_size, old_files, old_dirs) = (old.size, old.files, old.dirs);
        old.children = new_root.children.iter().map(|&c| map(c)).collect();
        old.size = new_root.size;
        old.mtime = new_root.mtime;
        old.files = new_root.files;
        old.dirs = new_root.dirs;

        let mut cur = self.nodes[id as usize].parent;
        while let Some(p) = cur {
            let n = &mut self.nodes[p as usize];
            n.size = n.size - old_size + new_root.size;
            n.files = n.files - old_files + new_root.files;
            n.dirs = n.dirs - old_dirs + new_root.dirs;
            // The folder on the way up changed size, so its place among its siblings may have too.
            let mut children = std::mem::take(&mut n.children);
            children.sort_by_key(|&c| std::cmp::Reverse(self.nodes[c as usize].size));
            self.nodes[p as usize].children = children;
            cur = self.nodes[p as usize].parent;
        }
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

/// Whether `name` matches `pattern`, where `*` stands for any run of characters and `?` for one.
fn glob(pattern: &[char], name: &[char]) -> bool {
    let (mut p, mut n) = (0, 0);
    // Where the last `*` was, and how much of the name it has swallowed so far.
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == name[n]) {
            p += 1;
            n += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some((p, n));
            p += 1;
        } else if let Some((sp, sn)) = star {
            p = sp + 1;
            n = sn + 1;
            star = Some((sp, sn + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

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

    #[test]
    fn graft_replaces_a_folder() {
        // 0 root/ ─┬─ 1 a/ ── 3 old (60)
        //          └─ 2 b (40)
        let mut tree = Tree {
            nodes: vec![
                Node { files: 2, dirs: 1, ..node("root/", 100, None, vec![1, 2]) },
                Node { files: 1, ..node("a", 60, Some(0), vec![3]) },
                node("b", 40, Some(0), vec![]),
                node("old", 60, Some(1), vec![]),
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
        // a/ now holds x (10) and y (5).
        let fresh = vec![
            Node { files: 2, ..node("/r/a", 15, None, vec![1, 2]) },
            node("x", 10, Some(0), vec![]),
            node("y", 5, Some(0), vec![]),
        ];
        tree.graft(1, fresh, HashMap::from([(2, HardLink { dev: 1, ino: 2, size: 5 })]));
        let root = tree.node(0).unwrap();
        assert_eq!((root.size, root.files, root.dirs), (55, 3, 1));
        assert_eq!(root.children, vec![2, 1], "b is now bigger than a");
        assert_eq!(&*tree.node(1).unwrap().name, "a");
        assert_eq!(tree.find(Path::new("/r/a/y")), Some(5));
        assert_eq!(tree.node(5).unwrap().parent, Some(1));
        assert!(tree.links.contains_key(&5));
        assert_eq!(tree.find(Path::new("/r/a/old")), None);
    }

    #[test]
    fn links_counted_outside_a_folder() {
        // 0 root/ ─┬─ 1 a/ ── 3 f (counted, 50)
        //          ├─ 2 b/ ── 4 g (same file, 0)
        //          └─ 5 h (another file, counted, 7)   6 gone (deleted, still in `links`)
        let link = |ino| HardLink { dev: 1, ino, size: 50 };
        let tree = Tree {
            nodes: vec![
                node("root/", 57, None, vec![1, 2, 5]),
                node("a", 50, Some(0), vec![3]),
                node("b/", 0, Some(0), vec![4]),
                node("f", 50, Some(1), vec![]),
                node("g", 0, Some(2), vec![]),
                node("h", 7, Some(0), vec![]),
                node("gone", 9, Some(0), vec![]),
            ],
            links: HashMap::from([(3, link(1)), (4, link(1)), (5, link(2)), (6, link(3))]),
            root_path: PathBuf::from("/r"),
            disk_total: 0,
            disk_free: 0,
            scan_ms: 0,
            scanned_at: 0,
            one_filesystem: false,
            cached: false,
        };
        // Rescanning b/, f already holds the size, so g stays 0.
        assert_eq!(tree.links_counted_outside(2), HashSet::from([(1, 1), (1, 2)]));
        // Rescanning a/, only g (which counted 0) is outside, so f counts again.
        assert_eq!(tree.links_counted_outside(1), HashSet::from([(1, 2)]));
    }

    #[test]
    fn search_by_name() {
        let tree = Tree {
            nodes: vec![
                node("root/", 100, None, vec![1, 2]),
                node("Photos/", 70, Some(0), vec![3, 4]),
                node("notes.txt", 25, Some(0), vec![]),
                node("beach.JPG", 50, Some(1), vec![]),
                node("photo.jpg.txt", 20, Some(1), vec![]),
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
        assert_eq!(tree.search("photo", 10), (vec![1, 4], 2));
        assert_eq!(tree.search("*.jpg", 10), (vec![3], 1));
        assert_eq!(tree.search("*.txt", 1), (vec![2], 2));
        assert_eq!(tree.search("n?tes*", 10), (vec![2], 1));
        assert_eq!(tree.search("  ", 10), (vec![], 0));
        assert!(glob(&['*'], &[]));
        assert!(!glob(&['a', '*', 'c'], &['a', 'b']));
    }
}
