# ScanShark roadmap

Planned work, in the order we intend to do it. Tick items off as they land.

The development PC is short on disk space, so keep that in mind: the Rust build cache in `src-tauri/target` is the biggest consumer (`cargo clean` frees it), and anything the app writes to disk (caches, reports) must stay small and be easy to clear.

## Phase 1: Selected Item panel and Top Files

- [x] **Selected Item panel** on the right of the treemap: name, path, size, share of the parent folder and the disk, file and folder counts, last modified. It holds the Open, Show in Folder, Zoom In and Delete buttons, which leave the toolbar. With nothing selected it describes the folder being viewed.
- [x] **Top Files list** under that panel: the 10 largest files under the folder being viewed. Clicking one zooms to its folder and selects it.

## Phase 2: Faster reloads

After the first scan of a drive, scanning it again should be much quicker, on Windows and on Linux (Fedora, Ubuntu).

- [x] Save a compact snapshot of each scan to the app's cache folder. Compress it, keep only the last few roots, cap the total size, and add a Setup option to clear it.
- [x] Opening or reloading a root that has a snapshot shows it straight away, marked as cached, while a fresh scan runs in the background and replaces it.
- [x] Incremental rescan: a folder whose modification time hasn't changed keeps its cached file list, so its files aren't listed and measured again. We still descend into its subfolders, because a change deep down doesn't touch the parent's time. A file that grows in place doesn't change its folder's time either, so keep a "Full rescan" action for when that matters.
- [ ] Windows: look at reading the NTFS master file table or the USN change journal, as WizTree does. Both need administrator rights, so this is an optional fast path, not the default.

Notes from landing the first three: snapshots are kept for the last 5 roots, up to 150 MB, in the app cache folder's `snapshots` directory. A 240K-item home folder makes a 2.3 MB snapshot, and a warm rescan drops from about 370 ms to 105 ms. Incremental rescans are switched off on FAT and exFAT drives, which don't reliably update folder times. A folder that couldn't be read last time and has since become readable stays empty until a Full Rescan.

## Phase 3: Summary cards and drive sidebar

- [x] **Summary cards** along the top: drive usage bar, total files, total folders, largest file.
- [x] **Drives sidebar** listing the drives (and Home), so you can switch drives without going back to the start screen.

## Phase 4: List view

- [x] A **List** tab next to the treemap: a table of the folder being viewed, with name, size, share, files and modified columns, sortable, with the same selection and zooming as the treemap.

## Phase 5: Export and search

- [ ] **Export Report** of the folder being viewed to CSV or HTML.
- [ ] **Search** by name across the scanned tree, with results that jump to the item.

## Known issues

- [ ] Windows: hard links (for example in `C:\Windows\WinSxS`) are counted once per link, so the Windows folder looks bigger than it is.
- [ ] Windows: "Stay on one filesystem" has no effect; hide it there or make it skip other volumes and junctions.
- [ ] Linux: Windows partitions mounted by the desktop go through ntfs-3g (FUSE), which answers about 6,000 files a second against 30,000 or more on btrfs, so a full C: drive takes minutes. Mounting it with the kernel's `ntfs3` driver is much faster; consider detecting `fuseblk` and saying so in the app.
- [ ] Moving the window to a screen with a different scale factor doesn't repaint the treemap at the new sharpness.
