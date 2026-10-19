use std::path::PathBuf;

pub type NodeId = u32;

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

pub struct Tree {
    pub nodes: Vec<Node>,
    pub root_path: PathBuf,
    pub disk_total: u64,
    pub disk_free: u64,
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
