# TripleWrapper

<img width="2112" height="1152" alt="Gemini_Generated_Image_hkzd9whkzd9whkzd" src="https://github.com/user-attachments/assets/a1b182d9-2b16-4224-a47f-781cdfeba883" />

> **Smart archive manager with intelligent storage decisions**  
> *Combining GNOME Disks simplicity with Steam's real-time performance graphs*

![TripleWrapper](https://img.shields.io/badge/version-1.0.0-blue)
![License](https://img.shields.io/badge/license-MIT-blue)
![Rust](https://img.shields.io/badge/rust-1.75+-orange)
![Python](https://img.shields.io/badge/python-3.11+-blue)
![GTK4](https://img.shields.io/badge/GTK-4.10+-green)

> **Status:** Stable.

## Overview

TripleWrapper is a modern archive manager for Linux that solves the classic problem: **"Not enough space to extract/modify this archive"**.

Instead of failing with a generic error, TripleWrapper:
1. **Calculates** the exact space needed for safe in-place rewriting (original + new file simultaneously)
2. **Scans** all mounted drives for available space
3. **Automatically redirects** workspace to an external drive if needed (`7z -w /mnt/external/.cache`)
4. **Shows real-time telemetry** with a Steam-style graph (read/write/compress MB/s)
5. **Verifies integrity** with BLAKE3 checksums before replacing the original

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                     TripleWrapper GUI (Python/GTK4)             │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐  │
│  │ Disk Panel  │  │ Donut Chart │  │    Steam Graph Widget   │  │
│  │ (GNOME)     │  │ (Storage)   │  │  (Read/Write/Compress)  │  │
│  └─────────────┘  └─────────────┘  └─────────────────────────┘  │
└──────────────────────────┬──────────────────────────────────────┘
                           │ DBus / IPC
┌──────────────────────────▼──────────────────────────────────────┐
│                  TripleWrapper Core (Rust)                      │
│  ┌────────────┐ ┌────────────┐ ┌──────────┐ ┌────────────────┐  │
│  │  Storage   │ │  Archive   │ │ Checksum │ │   Progress     │  │
│  │  Engine    │ │  Operator  │ │  (BLAKE3)│ │  Monitor       │  │
│  └────────────┘ └────────────┘ └──────────┘ └────────────────┘  │
└──────────────────────────┬──────────────────────────────────────┘
                           │ subprocess
┌──────────────────────────▼──────────────────────────────────────┐
│              System Tools: 7z • tar • pixz • lsblk • statvfs    │
└─────────────────────────────────────────────────────────────────┘
```

## Features

| Feature | Description |
|---------|-------------|
| **Smart Workspace Selection** | Auto-detects space, uses external drives via `7z -w` |
| **In-Place Safe** | Never modifies original until verified copy exists |
| **Steam-Style Graph** | Real-time read/write/compress speeds + ETA |
| **Storage Donut Chart** | Visual breakdown: keep / remove / add |
| **Multi-Format Support** | 7z, ZIP, TAR, TAR.GZ, TAR.XZ, TAR.ZST, TAR.BZ2, Pixz |
| **Checksum Verification** | BLAKE3 (default), SHA-256, XXH3 |
| **Flatpak Sandboxed** | Secure by default with explicit permissions |
| **Native GTK4/Libadwaita** | Perfect GNOME integration, works on KDE too |

## Installation

### Flatpak (Recommended)
```bash
# From Flathub (once published)
flatpak install flathub io.github.dinopathtream.TripleWrapper

# Or build locally
flatpak-builder --user --install --force-clean build-dir flatpak/io.github.dinopathtream.TripleWrapper.json
```

### From Source
```bash
# Requirements: Rust 1.75+, Python 3.11+, GTK4, Libadwaita, Graphene, Cairo
git clone https://github.com/triplewrapper/triplewrapper
cd triplewrapper

# Build Rust core
cd src/core && cargo build --release

# Build GUI
cd ../gui && pip install -e .

# Run
./triplewrapper-gui
```

### Arch Linux (AUR)
```bash
yay -S triplewrapper-git
```
> This option is unavailable until further notice. When is available, this notification will be deleted.


## Usage

### GUI
Launch `triplewrapper-gui` and:
1. Click **"Open file"** to select an archive
2. Click **"Analyze"** to check storage requirements
3. If external drive needed, confirm the suggested workspace
4. Click **"Start"** to begin operation
5. Watch the **Steam-style graph** for real-time speeds

### CLI (Core)
```bash
# List disks with free space
triplewrapper disks

# Analyze storage needs for an archive
triplewrapper analyze -a game.pak --remove 500M --add 1.5G --ratio 0.45

# List archive contents
triplewrapper list -a archive.7z

# Extract
triplewrapper extract -a archive.7z -o /output/dir

# Test integrity
triplewrapper test -a archive.7z

# Run DBus service (for GUI)
triplewrapper serve
```

## Storage Decision Logic

```
┌────────────────────────────────────────────────────────────┐
│  SPACE_NEEDED = current_size + estimated_final_size        │
│                     (In-Place Safe)                        │
└────────────────────────────────────────────────────────────┘
                              │
              ┌───────────────┼───────────────┐
              ▼               ▼               ▼
         SOURCE OK      EXTERNAL NEEDED   CRITICAL ERROR
         (Proceed)      (Redirect -w)     (Abort)
              │               │               │
         Free ≥ need    Free < need      Free < need
                        AND ext. OK      AND no ext.
```

## Development

### Project Structure
```
triplewrapper/
├── src/core/             # Rust core library + CLI
│   └── src/              # engine, archive, disk, checksum,
│                         # progress, queue, ipc, types, utils
├── triplewrapper_gui/    # Python/GTK4 GUI (canonical)
│   ├── views/            # welcome, analysis, progress
│   ├── widgets/          # steam_graph, storage_donut, queue_panel
│   ├── core/             # bridge, models, protocol
│   └── data/             # Desktop, icons, schemas, metainfo
├── tests/unit/           # Headless protocol tests (no display needed)
├── flatpak/              # Flatpak manifest (local-only, no network share)
├── meson.build           # GUI build
├── pyproject.toml        # GUI packaging + ruff/mypy config
└── .github/workflows/    # CI/CD
```

> Canonical GUI lives at `triplewrapper_gui/` (repo root). No other GUI copy exists.

### Running Tests
```bash
# Rust tests
cd src/core && cargo test

# Python tests (headless, no GTK display needed)
pytest tests/unit -v

# Integration (real CLI protocol)
./src/core/target/release/triplewrapper-core analyze -a file.zip --json
```

### Building Flatpak
```bash
flatpak-builder --force-clean --user --install-deps-from=flathub \
  build-dir flatpak/io.github.dinopathtream.TripleWrapper.json
flatpak run --command=triplewrapper-gui io.github.dinopathtream.TripleWrapper
```

## Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit changes (`git commit -m 'feat: add amazing feature'`)
4. Push to branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

### Code Style
- **Rust**: `cargo fmt` + `cargo clippy -D warnings`
- **Python**: `ruff check .` + `mypy triplewrapper_gui`
- **Commits**: Conventional Commits (`feat:`, `fix:`, `docs:`, etc.)

## Security

- **Local-only, offline-first**: todo el proceso corre en tu máquina, sin cloud ni cuentas
- **Flatpak sandbox** with minimal permissions (sin acceso a red)
- **No setuid/setgid** - uses polkit/udisks2 for mount operations
- **Checksum verification** before any file replacement
- **Atomic rename** (renameat2) for in-place safety
- **No arbitrary code execution** - only calls system tools (7z, tar)
- **Passwords**: prefer `TRIPLEWRAPPER_PASSWORD` over `--password` (visible in
  process list); never logged (`-p***` redaction), never persisted in queue
  files, wiped from memory on drop. Note: p7zip only accepts passwords via
  argv (it ignores piped stdin), so the 7z child briefly exposes it to local
  users — inherent to the 7z CLI, contained by the measures above

## Roadmap

- [x] **v0.1**: Foundation (storage engine, archive operator, GUI base)
- [x] **v0.2**: Batch operations, queue management
- [x] **v0.2.1**: Real core integration (JSON protocol, queue wiring)
- [x] **v0.3**: Local avanzado (archivos cifrados 7z AES, reanudar operaciones grandes, base local de integridad)
- [x] **v0.4**: Plugin system local (ejecutables `triplewrapper-*`, contrato en `docs/API.md`)
- [x] **v1.0**: Stable API, Flathub publication
- [X] **v1.1**: Full i18n (gettext, English source + `es.po`, system locale; Spanish UI ships under Flathub's new-submission exception until then)

> **Local-only principle**: no cloud backends (except rclone), no external telemetry,
> no network dependencies for operation. Network mounts (`/mnt`, NFS/SMB) are treated
> only as optional local paths, never as services.

## License

MIT - See [LICENSE](LICENSE) for details.

## Acknowledgments

- **7-Zip** by Igor Pavlov - Core compression engine
- **Steam** - Inspiration for the download graph UI
- **GNOME Disks** - Inspiration for the storage panel UI
- **Libadwaita** - Modern GTK4 widgets
- **BLAKE3** - Fast cryptographic hashing


---

**If you're having any issues from the app, please report it through the appropriate channels and we will acknowledge receipt within 5 business days.**

---


*Built with ❤️ for the Linux gaming and archiving community*

---
