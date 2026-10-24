<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="src/assets/logo-dark.png">
    <img src="src/assets/logo-light.png" alt="ScanShark" width="300">
  </picture>
</p>

# ScanShark

A fast, lightweight disk-space treemap in the spirit of SpaceMonger. Point it at a drive or folder and see at a glance where the space went.

Built with Rust, [Tauri 2](https://tauri.app) and React. The release binary is under 5 MB because it uses the system web view instead of bundling a browser.

## Features

- Parallel scan with live progress; hard links are counted once and symlinks are skipped
- Squarified treemap on a canvas, colored by folder depth, with name, size and date labels
- Hover tooltip with path, size, share of the parent folder and file counts
- Selected Item panel with size, share, file counts, modified date and actions
- Top Files list of the largest files under the current folder (10, 25 or 50); click one to jump to it
- Summary cards for drive usage, file and folder counts and the largest file, and a drives list to switch drives in one click
- List view of the current folder, sortable by name, size, file count or date, with the same selection, zooming and keyboard navigation
- Search by name across the whole scan (Ctrl+F), with `*` and `?` wildcards; results jump to the item
- Export Report of the current folder as a self-contained HTML page or as CSV (its items, or everything below it)
- View menu to show or hide the drives list, summary cards, Selected Item panel and Top Files
- Zoom in, out and full; mouse wheel, double-click and breadcrumb navigation
- Free-space block, reload, "stay on one filesystem" option
- Scans are saved compactly, so a drive you scanned before shows at once while a quicker rescan skips unchanged folders; **Full Rescan** checks everything
- Move to trash, open item, show in file manager
- Light (warm cream) and dark themes, 1024- or 1000-based size units, and an option to reopen the last folder at startup

## Install

Download from the [latest release](https://github.com/Leafyheader/scanshark/releases/latest):

- **Ubuntu / Debian:** `sudo apt install ./scanshark_*_amd64.deb`
- **Fedora:** `sudo dnf install ./scanshark-*.x86_64.rpm`

## Controls

| Action | How |
| --- | --- |
| Select | Click |
| Zoom into folder | Double-click, mouse wheel up, or the zoom-in button above the treemap |
| Zoom out | Mouse wheel down, Backspace, Esc, or the up-arrow button above the treemap |
| Search | Ctrl+F, then arrow keys and Enter |
| Jump to a level | Click a breadcrumb above the treemap |
| Delete (to trash) | Delete key, or **Delete** in the Selected Item panel |

## Build from source

Requirements: [Rust](https://rustup.rs), Node.js 20+, and the [Tauri system dependencies](https://tauri.app/start/prerequisites/) for your OS.

```sh
npm install
npm run tauri dev                    # run in development
npm run tauri build -- --no-bundle   # release binary in src-tauri/target/release
```

Release builds use several GB of disk for the Rust build cache in `src-tauri/target`; run `cargo clean` in `src-tauri` to reclaim it.

## License

[MIT](LICENSE) © 2026 Leafyhead Technologies
