//! Compressed copies of finished scans in the app's cache folder, so reopening a root can show
//! it straight away and the next scan can skip folders that haven't changed.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};

use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use flate2::Compression;

use crate::tree::{HardLink, Node, NodeId, Tree, ROOT};

const MAGIC: &[u8; 4] = b"SSNP";
const VERSION: u8 = 1;
/// Snapshots kept, most recent first; older ones are deleted.
const KEEP: usize = 5;
/// Most the snapshot folder may hold; the oldest snapshots go first.
const MAX_BYTES: u64 = 150 * 1024 * 1024;

const FLAG_DIR: u8 = 1;
const FLAG_LINK: u8 = 2;

fn file_for(dir: &Path, root: &Path) -> PathBuf {
    // FNV-1a, which unlike DefaultHasher stays the same across Rust releases.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in root.to_string_lossy().bytes() {
        h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
    }
    dir.join(format!("{h:016x}.snap"))
}

/// Writes the snapshot of `tree`, then trims the folder back to its limits.
pub fn save(dir: &Path, tree: &Tree) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let path = file_for(dir, &tree.root_path);
    let tmp = path.with_extension("tmp");
    // Buffered before the encoder too: it is slow at the many tiny writes a tree makes.
    let mut w = Writer(BufWriter::new(DeflateEncoder::new(BufWriter::new(File::create(&tmp)?), Compression::fast())));
    write_tree(&mut w, tree)?;
    w.0.into_inner().map_err(|e| e.into_error())?.finish()?.flush()?;
    fs::rename(&tmp, &path)?;
    prune(dir);
    Ok(())
}

/// The saved snapshot of `root`, if there is a readable one.
pub fn load(dir: &Path, root: &Path) -> Option<Tree> {
    let file = File::open(file_for(dir, root)).ok()?;
    let tree = read_tree(&mut Reader(BufReader::new(DeflateDecoder::new(BufReader::new(file))))).ok()?;
    (tree.root_path == root).then_some(tree)
}

/// Bytes used by the snapshot folder.
pub fn total_size(dir: &Path) -> u64 {
    fs::read_dir(dir).map_or(0, |read| read.flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum())
}

pub fn clear(dir: &Path) -> io::Result<()> {
    match fs::remove_dir_all(dir) {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        r => r,
    }
}

fn prune(dir: &Path) {
    let Ok(read) = fs::read_dir(dir) else { return };
    let mut snaps: Vec<_> = read
        .flatten()
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((m.modified().ok()?, m.len(), e.path()))
        })
        .collect();
    snaps.sort_by(|a, b| b.0.cmp(&a.0));
    let mut total = 0;
    for (i, (_, len, path)) in snaps.into_iter().enumerate() {
        total += len;
        if i >= KEEP || total > MAX_BYTES {
            let _ = fs::remove_file(path);
        }
    }
}

/// Nodes are written parents first, each followed by its children in order. Folder totals and
/// parent links aren't stored; they are rebuilt on load.
fn write_tree<W: Write>(w: &mut Writer<W>, tree: &Tree) -> io::Result<()> {
    w.0.write_all(MAGIC)?;
    w.0.write_all(&[VERSION])?;
    w.str(&tree.root_path.to_string_lossy())?;
    w.int(tree.scanned_at)?;
    w.uint(tree.disk_total)?;
    w.uint(tree.disk_free)?;
    w.uint(tree.scan_ms)?;
    w.0.write_all(&[tree.one_filesystem as u8])?;
    let root = &tree.nodes[ROOT as usize];
    w.uint(root.files + root.dirs + 1)?;

    let mut stack = vec![ROOT];
    while let Some(id) = stack.pop() {
        let n = &tree.nodes[id as usize];
        let link = tree.links.get(&id);
        let flags = if n.is_dir { FLAG_DIR } else { 0 } | if link.is_some() { FLAG_LINK } else { 0 };
        w.str(&n.name)?;
        w.0.write_all(&[flags])?;
        w.int(n.mtime)?;
        if n.is_dir {
            w.uint(n.children.len() as u64)?;
            stack.extend(n.children.iter().rev());
        } else {
            w.uint(n.size)?;
            if let Some(l) = link {
                w.uint(l.dev)?;
                w.uint(l.ino)?;
                w.uint(l.size)?;
            }
        }
    }
    Ok(())
}

fn read_tree<R: Read>(r: &mut Reader<R>) -> io::Result<Tree> {
    let mut magic = [0u8; 5];
    r.0.read_exact(&mut magic)?;
    if magic[..4] != *MAGIC || magic[4] != VERSION {
        return Err(io::Error::new(ErrorKind::InvalidData, "not a snapshot"));
    }
    let root_path = PathBuf::from(r.str()?);
    let scanned_at = r.int()?;
    let (disk_total, disk_free, scan_ms) = (r.uint()?, r.uint()?, r.uint()?);
    let one_filesystem = r.byte()? != 0;
    let count = r.uint()?;

    let mut nodes: Vec<Node> = Vec::with_capacity(count.min(1 << 24) as usize);
    let mut links = HashMap::new();
    // Folders still waiting for children, with how many are left.
    let mut open: Vec<(NodeId, u64)> = Vec::new();
    loop {
        while open.last().is_some_and(|&(_, left)| left == 0) {
            open.pop();
        }
        let parent = match open.last_mut() {
            Some((p, left)) => {
                *left -= 1;
                Some(*p)
            }
            None if nodes.is_empty() => None,
            None => break,
        };
        let id = nodes.len() as NodeId;
        let name = r.str()?.into_boxed_str();
        let flags = r.byte()?;
        let mtime = r.int()?;
        let is_dir = flags & FLAG_DIR != 0;
        let mut node = Node { name, size: 0, mtime, parent, children: Vec::new(), is_dir, files: 0, dirs: 0 };
        if is_dir {
            let n = r.uint()?;
            node.children.reserve(n.min(1 << 16) as usize);
            open.push((id, n));
        } else {
            node.size = r.uint()?;
            if flags & FLAG_LINK != 0 {
                links.insert(id, HardLink { dev: r.uint()?, ino: r.uint()?, size: r.uint()? });
            }
        }
        if let Some(p) = parent {
            nodes[p as usize].children.push(id);
        }
        nodes.push(node);
    }

    // Children always come after their parent, so walking backwards finishes each folder first.
    for id in (1..nodes.len()).rev() {
        let n = &nodes[id];
        let (size, files, dirs, is_dir, parent) = (n.size, n.files, n.dirs, n.is_dir, n.parent.unwrap());
        let p = &mut nodes[parent as usize];
        p.size += size;
        if is_dir {
            p.files += files;
            p.dirs += dirs + 1;
        } else {
            p.files += 1;
        }
    }

    Ok(Tree { nodes, links, root_path, disk_total, disk_free, scan_ms, scanned_at, one_filesystem, cached: true })
}

struct Writer<W>(W);

impl<W: Write> Writer<W> {
    /// LEB128 varint.
    fn uint(&mut self, mut v: u64) -> io::Result<()> {
        let mut buf = [0u8; 10];
        let mut i = 0;
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                buf[i] = b;
                return self.0.write_all(&buf[..=i]);
            }
            buf[i] = b | 0x80;
            i += 1;
        }
    }
    /// Zigzag varint.
    fn int(&mut self, v: i64) -> io::Result<()> {
        self.uint(((v << 1) ^ (v >> 63)) as u64)
    }
    fn str(&mut self, s: &str) -> io::Result<()> {
        self.uint(s.len() as u64)?;
        self.0.write_all(s.as_bytes())
    }
}

struct Reader<R>(R);

impl<R: Read> Reader<R> {
    fn byte(&mut self) -> io::Result<u8> {
        let mut b = [0u8; 1];
        self.0.read_exact(&mut b)?;
        Ok(b[0])
    }
    fn uint(&mut self) -> io::Result<u64> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let b = self.byte()?;
            v |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err(io::Error::new(ErrorKind::InvalidData, "varint too long"))
    }
    fn int(&mut self) -> io::Result<i64> {
        let v = self.uint()?;
        Ok((v >> 1) as i64 ^ -((v & 1) as i64))
    }
    fn str(&mut self) -> io::Result<String> {
        let len = self.uint()?;
        if len > 1 << 20 {
            return Err(io::Error::new(ErrorKind::InvalidData, "name too long"));
        }
        let mut buf = vec![0u8; len as usize];
        self.0.read_exact(&mut buf)?;
        String::from_utf8(buf).map_err(|e| io::Error::new(ErrorKind::InvalidData, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        // 0 /r ─┬─ 1 a ─┬─ 2 big (50, linked)
        //       │       └─ 3 mid (20)
        //       └─ 4 small (5)
        let node = |name: &str, size, parent, children: Vec<NodeId>, is_dir| Node {
            name: Box::from(name),
            size,
            mtime: -3,
            parent,
            children,
            is_dir,
            files: 0,
            dirs: 0,
        };
        let tree = Tree {
            nodes: vec![
                node("/r", 75, None, vec![1, 4], true),
                node("a", 70, Some(0), vec![2, 3], true),
                node("big", 50, Some(1), vec![], false),
                node("mid", 20, Some(1), vec![], false),
                node("small", 5, Some(0), vec![], false),
            ],
            links: HashMap::from([(2, HardLink { dev: 7, ino: u64::MAX, size: 50 })]),
            root_path: PathBuf::from("/r"),
            disk_total: 1000,
            disk_free: 300,
            scan_ms: 12,
            scanned_at: 1_700_000_000,
            one_filesystem: true,
            cached: false,
        };
        let mut buf = Vec::new();
        write_tree(&mut Writer(&mut buf), &tree).unwrap();
        let back = read_tree(&mut Reader(&buf[..])).unwrap();

        assert!(back.cached && back.one_filesystem);
        assert_eq!((back.scanned_at, back.disk_free, back.scan_ms), (1_700_000_000, 300, 12));
        assert_eq!(back.nodes.len(), 5);
        let root = &back.nodes[0];
        assert_eq!((root.size, root.files, root.dirs), (75, 3, 1));
        assert_eq!(back.find(Path::new("/r/a/mid")), Some(3));
        assert_eq!(back.nodes[3].mtime, -3);
        assert_eq!(back.links[&2].ino, u64::MAX);
        assert!(read_tree(&mut Reader(&buf[..buf.len() - 1])).is_err());
    }
}
