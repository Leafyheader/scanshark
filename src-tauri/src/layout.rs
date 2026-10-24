use serde::Serialize;

use crate::tree::{NodeId, Tree};

/// Id used for the virtual "free space" block.
pub const FREE_ID: NodeId = u32::MAX;

pub const KIND_FILE: u8 = 0;
pub const KIND_DIR: u8 = 1;
pub const KIND_FREE: u8 = 2;

const MAX_RECTS: usize = 400_000;

/// Rectangles are emitted parents-first, so the last rect under a point is the deepest.
#[derive(Serialize)]
pub struct Rect {
    pub id: NodeId,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub depth: u16,
    pub kind: u8,
    /// Height of the strip a directory reserves at the top for its name; 0 when it has none.
    pub header: f32,
    pub name: String,
    pub size: u64,
    pub mtime: i64,
}

pub fn layout(tree: &Tree, id: NodeId, width: f64, height: f64, show_free: bool) -> Vec<Rect> {
    let mut out = Vec::new();
    if tree.node(id).is_none() || width < 1.0 || height < 1.0 {
        return out;
    }
    let free = if show_free && id == crate::tree::ROOT { tree.disk_free } else { 0 };
    lay_node(tree, id, 0.0, 0.0, width, height, 0, free, &mut out);
    out
}

#[allow(clippy::too_many_arguments)]
fn lay_node(tree: &Tree, id: NodeId, x: f64, y: f64, w: f64, h: f64, depth: u16, free: u64, out: &mut Vec<Rect>) {
    let node = &tree.nodes[id as usize];
    if !node.is_dir {
        push(out, id, x, y, w, h, depth, KIND_FILE, 0.0, &node.name, node.size, node.mtime);
        return;
    }

    // Big blocks get a taller header with room for the size; the padding leaves a gutter around children.
    // The view's own folder is the backdrop and is named by the UI, so it gets neither.
    let header = if depth == 0 {
        0.0
    } else if w >= 120.0 && h >= 64.0 {
        24.0
    } else if w >= 46.0 && h >= 30.0 {
        16.0
    } else {
        0.0
    };
    let (ix, iy, iw, ih) = if depth == 0 {
        (x, y, w, h)
    } else if header > 0.0 {
        (x + 3.0, y + header, w - 6.0, h - header - 3.0)
    } else if w >= 8.0 && h >= 8.0 {
        (x + 2.0, y + 2.0, w - 4.0, h - 4.0)
    } else {
        (x, y, 0.0, 0.0)
    };
    let size = node.size + free;
    push(out, id, x, y, w, h, depth, KIND_DIR, header, &node.name, size, node.mtime);
    if iw < 2.0 || ih < 2.0 {
        return;
    }

    let mut items: Vec<(NodeId, u64)> = node
        .children
        .iter()
        .map(|&c| (c, tree.nodes[c as usize].size))
        .filter(|&(_, s)| s > 0)
        .collect();
    if free > 0 {
        let pos = items.partition_point(|&(_, s)| s >= free);
        items.insert(pos, (FREE_ID, free));
    }

    squarify(&items, ix, iy, iw, ih, |cid, cx, cy, cw, ch| {
        if out.len() >= MAX_RECTS || cw < 2.0 || ch < 2.0 {
            return;
        }
        if cid == FREE_ID {
            push(out, FREE_ID, cx, cy, cw, ch, depth + 1, KIND_FREE, 0.0, "Free Space", free, 0);
        } else {
            lay_node(tree, cid, cx, cy, cw, ch, depth + 1, 0, out);
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn push(out: &mut Vec<Rect>, id: NodeId, x: f64, y: f64, w: f64, h: f64, depth: u16, kind: u8, header: f64, name: &str, size: u64, mtime: i64) {
    // Names are only useful where a label can actually be drawn.
    let labelled = header > 0.0 || (w >= 30.0 && h >= 12.0);
    out.push(Rect {
        id,
        x: x as f32,
        y: y as f32,
        w: w as f32,
        h: h as f32,
        depth,
        kind,
        header: header as f32,
        name: if labelled { name.to_owned() } else { String::new() },
        size,
        mtime,
    });
}

/// Squarified treemap (Bruls et al.). `items` must be sorted by size, descending.
fn squarify(items: &[(NodeId, u64)], x: f64, y: f64, w: f64, h: f64, mut emit: impl FnMut(NodeId, f64, f64, f64, f64)) {
    let total: f64 = items.iter().map(|&(_, s)| s as f64).sum();
    if total <= 0.0 || w <= 0.0 || h <= 0.0 {
        return;
    }
    let scale = w * h / total;
    let (mut x, mut y, mut w, mut h) = (x, y, w, h);
    let mut i = 0;
    while i < items.len() {
        let first = items[i].1 as f64 * scale;
        if first < 4.0 {
            break; // everything remaining is sub-pixel; leave it blank
        }
        let short = w.min(h);
        let mut j = i;
        let mut sum = 0.0;
        let mut best = f64::MAX;
        while j < items.len() {
            let a = items[j].1 as f64 * scale;
            let new_sum = sum + a;
            let worst = (short * short * first / (new_sum * new_sum)).max(new_sum * new_sum / (short * short * a));
            if j > i && worst > best {
                break;
            }
            best = worst;
            sum = new_sum;
            j += 1;
        }

        if w >= h {
            let cw = sum / h;
            let mut cy = y;
            for &(id, s) in &items[i..j] {
                let ih = s as f64 * scale / cw;
                emit(id, x, cy, cw, ih);
                cy += ih;
            }
            x += cw;
            w -= cw;
        } else {
            let rh = sum / w;
            let mut cx = x;
            for &(id, s) in &items[i..j] {
                let iw = s as f64 * scale / rh;
                emit(id, cx, y, iw, rh);
                cx += iw;
            }
            y += rh;
            h -= rh;
        }
        i = j;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn squarify_fills_area_proportionally() {
        let items = [(1, 6), (2, 6), (3, 4), (4, 3), (5, 2), (6, 2), (7, 1)];
        let mut rects = Vec::new();
        squarify(&items, 0.0, 0.0, 600.0, 400.0, |id, x, y, w, h| rects.push((id, x, y, w, h)));
        assert_eq!(rects.len(), items.len());
        let area: f64 = rects.iter().map(|r| r.3 * r.4).sum();
        assert!((area - 240_000.0).abs() < 1.0);
        let (_, _, _, w, h) = rects[0];
        assert!((w * h - 240_000.0 * 6.0 / 24.0).abs() < 1.0);
    }
}
