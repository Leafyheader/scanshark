<p align="center"><img src="src/assets/logo.png" alt="ScanShark" width="220"></p>

# ScanShark

A fast, lightweight disk-space treemap in the spirit of SpaceMonger. Point it at a drive or folder and see at a glance where the space went.

Built with Rust, [Tauri 2](https://tauri.app) and React. The release binary is under 5 MB because it uses the system web view instead of bundling a browser.

## Features

- Parallel scan with live progress; hard links are counted once and symlinks are skipped
- Squarified treemap on a canvas, colored by folder depth, with name, size and date labels
- Hover tooltip with path, size, share of the parent folder and file counts
- Zoom in, out and full; mouse wheel, double-click and breadcrumb navigation
- Free-space block, reload, "stay on one filesystem" option
- Move to trash, open item, show in file manager
- Light and dark themes

## Controls

| Action | How |
| --- | --- |
| Select | Click |
| Zoom into folder | Double-click, mouse wheel up, or **Zoom In** |
| Zoom out | Mouse wheel down, Backspace, Esc, or **Zoom Out** |
| Jump to a level | Click a breadcrumb in the status bar |
| Delete (to trash) | Delete key or **Delete** |

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
