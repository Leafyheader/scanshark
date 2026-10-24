//! Export Report: the folder being viewed, written out as CSV or as a self-contained HTML page.

use std::io::{self, Write};

use crate::tree::{NodeId, Tree};

/// How many of the largest files the HTML report lists.
const HTML_TOP_FILES: usize = 50;

/// "1.5 GB", counting 1 KB as 1000 bytes when `decimal`, else as 1024, like the app does.
pub fn human_size(bytes: u64, decimal: bool) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let step = if decimal { 1000.0 } else { 1024.0 };
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= step && i < UNITS.len() - 1 {
        v /= step;
        i += 1;
    }
    match i {
        0 => format!("{bytes} B"),
        _ if v >= 100.0 => format!("{v:.0} {}", UNITS[i]),
        _ if v >= 10.0 => format!("{v:.1} {}", UNITS[i]),
        _ => format!("{v:.2} {}", UNITS[i]),
    }
}

/// Unix seconds as "2026-10-24 13:05 UTC", or "" when unknown.
pub fn utc_date(secs: i64) -> String {
    if secs <= 0 {
        return String::new();
    }
    // Days to civil date, after Howard Hinnant's `civil_from_days`.
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", rem / 3600, rem % 3600 / 60)
}

fn percent(part: u64, whole: u64) -> String {
    if whole == 0 { "0%".into() } else { format!("{:.1}%", part as f64 * 100.0 / whole as f64) }
}

/// A CSV field, quoted when needed. A leading `=`, `+`, `-` or `@` is defused with a `'`, so a
/// spreadsheet doesn't run a file name as a formula.
fn csv_field(s: &str) -> String {
    let s = if s.starts_with(['=', '+', '-', '@']) { format!("'{s}") } else { s.to_string() };
    if s.contains([',', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s }
}

fn html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Writes folder `id` as CSV: its own items, or with `recursive` everything below it, each
/// folder followed by its contents. Returns the number of rows.
pub fn write_csv(tree: &Tree, id: NodeId, recursive: bool, decimal: bool, w: &mut impl Write) -> io::Result<u64> {
    let total = tree.nodes[id as usize].size;
    writeln!(w, "Path,Type,Size (bytes),Size,Share of folder,Files,Folders,Modified")?;
    let mut rows = 0;
    let mut stack: Vec<NodeId> = tree.nodes[id as usize].children.iter().rev().copied().collect();
    while let Some(c) = stack.pop() {
        let n = &tree.nodes[c as usize];
        let (files, dirs) = if n.is_dir { (n.files.to_string(), n.dirs.to_string()) } else { (String::new(), String::new()) };
        writeln!(
            w,
            "{},{},{},{},{},{},{},{}",
            csv_field(&tree.path_of(c).to_string_lossy()),
            if n.is_dir { "Folder" } else { "File" },
            n.size,
            human_size(n.size, decimal),
            percent(n.size, total),
            files,
            dirs,
            utc_date(n.mtime),
        )?;
        rows += 1;
        if recursive {
            stack.extend(n.children.iter().rev());
        }
    }
    Ok(rows)
}

/// Writes folder `id` as one HTML page: totals, the items in it, and its largest files.
pub fn write_html(tree: &Tree, id: NodeId, decimal: bool, now: i64, w: &mut impl Write) -> io::Result<()> {
    let n = &tree.nodes[id as usize];
    let path = tree.path_of(id).to_string_lossy().into_owned();
    let size = |b| html(&human_size(b, decimal));
    write!(
        w,
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} - ScanShark report</title>
<style>
:root {{ --bg: #ece6da; --panel: #f8f4ec; --text: #2b2620; --muted: #73695b; --border: #ddd4c4; --accent: #2563c9; color-scheme: light dark; }}
@media (prefers-color-scheme: dark) {{
  :root {{ --bg: #0b1120; --panel: #111a2e; --text: #e6ebf5; --muted: #8b97b0; --border: #22304d; --accent: #5ea0ff; }}
}}
body {{ margin: 0; padding: 24px 16px; background: var(--bg); color: var(--text); font: 14px/1.4 system-ui, -apple-system, "Segoe UI", sans-serif; }}
main {{ max-width: 1000px; margin: 0 auto; display: flex; flex-direction: column; gap: 16px; }}
h1 {{ margin: 0; font-size: 22px; }}
h2 {{ margin: 0 0 10px; font-size: 13px; text-transform: uppercase; letter-spacing: 0.05em; color: var(--muted); }}
.muted {{ color: var(--muted); }}
.path {{ color: var(--muted); word-break: break-all; }}
section {{ background: var(--panel); border: 1px solid var(--border); border-radius: 12px; padding: 14px 16px; overflow-x: auto; }}
.stats {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(160px, 1fr)); gap: 12px; }}
.stats div {{ background: var(--panel); border: 1px solid var(--border); border-radius: 12px; padding: 12px 14px; }}
.stats b {{ display: block; font-size: 20px; font-variant-numeric: tabular-nums; }}
table {{ width: 100%; border-collapse: collapse; }}
th, td {{ padding: 6px 8px; text-align: left; border-bottom: 1px solid var(--border); white-space: nowrap; }}
th {{ font-size: 12px; color: var(--muted); font-weight: 600; }}
td.name {{ white-space: normal; word-break: break-all; }}
.num {{ text-align: right; font-variant-numeric: tabular-nums; }}
.bar {{ display: inline-block; width: 90px; height: 6px; border-radius: 3px; background: var(--border); overflow: hidden; vertical-align: middle; margin-right: 8px; }}
.bar span {{ display: block; height: 100%; background: var(--accent); }}
footer {{ font-size: 12px; color: var(--muted); text-align: center; }}
</style>
</head>
<body>
<main>
<header>
<h1>{title}</h1>
<div class="path">{path}</div>
</header>
<div class="stats">
<div><span class="muted">Size</span><b>{size}</b></div>
<div><span class="muted">Files</span><b>{files}</b></div>
<div><span class="muted">Folders</span><b>{dirs}</b></div>
<div><span class="muted">Share of disk</span><b>{disk}</b></div>
</div>
<section>
<h2>Items in this folder</h2>
<table>
<tr><th>Name</th><th class="num">Size</th><th>Share</th><th class="num">Files</th><th>Modified</th></tr>
"#,
        title = html(&n.name),
        path = html(&path),
        size = size(n.size),
        files = n.files,
        dirs = n.dirs,
        disk = if tree.disk_total > 0 { percent(n.size, tree.disk_total) } else { "-".into() },
    )?;
    for &c in &n.children {
        let k = &tree.nodes[c as usize];
        let share = percent(k.size, n.size);
        writeln!(
            w,
            r#"<tr><td class="name">{icon} {name}</td><td class="num">{size}</td><td><span class="bar"><span style="width:{share}"></span></span>{share}</td><td class="num">{files}</td><td class="muted">{date}</td></tr>"#,
            icon = if k.is_dir { "📁" } else { "📄" },
            name = html(&k.name),
            size = size(k.size),
            files = if k.is_dir { k.files.to_string() } else { String::new() },
            date = utc_date(k.mtime),
        )?;
    }
    if n.children.is_empty() {
        writeln!(w, r#"<tr><td colspan="5" class="muted">This folder is empty.</td></tr>"#)?;
    }
    write!(
        w,
        r#"</table>
</section>
<section>
<h2>Largest files</h2>
<table>
<tr><th>Name</th><th>Folder</th><th class="num">Size</th><th>Modified</th></tr>
"#
    )?;
    for f in tree.largest_files(id, HTML_TOP_FILES) {
        let k = &tree.nodes[f as usize];
        let dir: Vec<&str> = tree.trail(id, f).iter().map(|&d| &*tree.nodes[d as usize].name).collect();
        writeln!(
            w,
            r#"<tr><td class="name">{name}</td><td class="muted">{dir}</td><td class="num">{size}</td><td class="muted">{date}</td></tr>"#,
            name = html(&k.name),
            dir = html(&dir.join(std::path::MAIN_SEPARATOR_STR)),
            size = size(k.size),
            date = utc_date(k.mtime),
        )?;
    }
    write!(
        w,
        r#"</table>
</section>
<footer>Made by ScanShark on {now}, from a scan taken {scanned}.</footer>
</main>
</body>
</html>
"#,
        now = utc_date(now),
        scanned = utc_date(tree.scanned_at),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatting() {
        assert_eq!(human_size(512, false), "512 B");
        assert_eq!(human_size(1536, false), "1.50 KB");
        assert_eq!(human_size(1_500_000_000, true), "1.50 GB");
        assert_eq!(utc_date(0), "");
        assert_eq!(utc_date(1_793_000_000), "2026-10-26 07:33 UTC");
        assert_eq!(utc_date(951_782_400), "2000-02-29 00:00 UTC");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("=SUM(A1)"), "'=SUM(A1)");
        assert_eq!(csv_field("say \"hi\", ok"), "\"say \"\"hi\"\", ok\"");
        assert_eq!(html("<a & b>"), "&lt;a &amp; b&gt;");
    }
}
