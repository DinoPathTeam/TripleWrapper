# TripleWrapper

> **Smart archive manager with intelligent storage decisions**  
> *Combining GNOME Disks simplicity with Steam's real-time performance graphs*

![TripleWrapper](https://img.shields.io/badge/version-0.1.0--alpha-orange)
![License](https://img.shields.io/badge/license-MIT-blue)
![Rust](https://img.shields.io/badge/rust-1.75+-orange)
![Python](https://img.shields.io/badge/python-3.11+-blue)
![GTK4](https://img.shields.io/badge/GTK-4.10+-green)

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
flatpak install flathub com.triplewrapper.TripleWrapper

# Or build locally
flatpak-builder --user --install --force-clean build-dir flatpak/com.triplewrapper.TripleWrapper.json
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

## Usage

### GUI
Launch `triplewrapper-gui` and:
1. Click **"Abrir archivo"** to select an archive
2. Click **"Analizar"** to check storage requirements
3. If external drive needed, confirm the suggested workspace
4. Click **"Iniciar"** to begin operation
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
├── src/
│   ├── core/           # Rust core library + CLI
│   │   ├── src/
│   │   │   ├── engine/       # Storage decision engine
│   │   │   ├── archive/      # 7z/tar/pixz wrapper
│   │   │   ├── disk/         # Disk scanning (sysinfo + lsblk)
│   │   │   ├── checksum/     # BLAKE3/SHA256/XXH3 streaming
│   │   │   ├── progress/     # Real-time telemetry
│   │   │   ├── ipc/          # DBus service
│   │   │   └── main.rs       # CLI entry point
│   │   └── Cargo.toml
│   │
│   ├── gui/            # Python/GTK4 GUI
│   │   ├── triplewrapper_gui/
│   │   │   ├── widgets/      # SteamGraph, DonutChart, DiskPanel
│   │   │   ├── core/         # DBus client
│   │   │   └── utils/        # Formatting, settings
│   │   ├── data/             # Desktop, icons, schemas
│   │   ├── meson.build
│   │   └── pyproject.toml
│   │
│   └── ipc/            # Shared IPC definitions
│
├── flatpak/            # Flatpak manifest
├── tests/              # Integration tests
└── .github/workflows/  # CI/CD
```

### Running Tests
```bash
# Rust tests
cd src/core && cargo test

# Python tests
cd src/gui && pytest tests/ -v

# Integration
cargo test --all && pytest tests/
```

### Building Flatpak
```bash
flatpak-builder --force-clean --user --install-deps-from=flathub \
  build-dir flatpak/com.triplewrapper.TripleWrapper.json
flatpak run com.triplewrapper.TripleWrapper
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

- **Flatpak sandbox** with minimal permissions
- **No setuid/setgid** - uses polkit/udisks2 for mount operations
- **Checksum verification** before any file replacement
- **Atomic rename** (renameat2) for in-place safety
- **No arbitrary code execution** - only calls system tools (7z, tar)

## Roadmap

### ✅ v0.1 - Foundation (COMPLETED)
- [x] **Core Architecture**: Rust core + Python/GTK4 GUI with DBus IPC
- [x] **Storage Decision Engine**: Internal/External/Critical verdict logic
- [x] **Disk Scanner**: sysinfo + lsblk + statvfs for accurate space detection
- [x] **Archive Operator**: 7z/tar/pixz wrapper with progress parsing
- [x] **Checksum Verification**: Streaming BLAKE3/SHA256/XXH3
- [x] **Progress Monitor**: Real-time telemetry + /proc/pid/io monitoring
- [x] **Steam-style Graph Widget**: Read/Write/Compress MB/s + ETA (Cairo)
- [x] **Donut Chart Widget**: Animated keep/remove/add breakdown
- [x] **Disk Panel Widget**: GNOME Disks style with selection
- [x] **Flatpak Manifest**: Sandboxed with host fs + udisks2 permissions
- [x] **CI/CD Pipeline**: GitHub Actions (rust-core, python-gui, integration, flatpak)
- [x] **MIT License**

### 🔄 v0.2 - Batch Operations (PLANNED)
- [ ] Batch operations queue management
- [ ] Pause/resume operations
- [ ] Multiple archive simultaneous processing

### 🔄 v0.3 - Cloud & Advanced (PLANNED)
- [ ] Cloud storage backends (rclone integration)
- [ ] Network mount auto-detection
- [ ] Encrypted archive support

### 🔄 v0.4 - Extensibility (PLANNED)
- [ ] Plugin system for custom formats
- [ ] Scripting API (Lua/Python)

### 🔄 v1.0 - Stable Release (PLANNED)
- [ ] Stable API
- [ ] Flathub publication
- [ ] AUR package
- [ ] Comprehensive documentation

## License

MIT - See [LICENSE](LICENSE) for details.

## Acknowledgments

- **7-Zip** by Igor Pavlov - Core compression engine
- **Steam** - Inspiration for the download graph UI
- **GNOME Disks** - Inspiration for the storage panel UI
- **Libadwaita** - Modern GTK4 widgets
- **BLAKE3** - Fast cryptographic hashing

---

*Built with ❤️ for the Linux gaming and archiving community*