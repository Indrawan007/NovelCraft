<div align="center">

# 📚 NovelCraft

**Aplikasi Baca & Tulis Novel — Native, Ringan, Wayland-First**

*Dibangun dengan Rust untuk Arch Linux + Hyprland*

[![Rust](https://img.shields.io/badge/Rust-1.75+-orange?logo=rust)](https://rust-lang.org)
[![Iced](https://img.shields.io/badge/GUI-Iced%200.13-blue)](https://iced.rs)
[![Wayland](https://img.shields.io/badge/Display-Wayland-green)](https://wayland.freedesktop.org)
[![License](https://img.shields.io/badge/License-MIT-yellow)](LICENSE)
[![Platform](https://img.shields.io/badge/Platform-Arch%20Linux-blueviolet)](https://archlinux.org)

</div>

---

## 🎯 Tujuan

NovelCraft adalah aplikasi **all-in-one** untuk para pembaca dan penulis novel
yang menginginkan pengalaman **native, cepat, dan bebas gangguan** di lingkungan
desktop modern berbasis Wayland.

### Mengapa aplikasi ini ada?

| Masalah yang Dihadapi | Solusi NovelCraft |
|---|---|
| Aplikasi novel berbasis Electron terlalu berat (200MB+ RAM) | Binary Rust ~20MB, RAM <100MB |
| Tidak ada app novel yang native Wayland | Wayland-first, tanpa XWayland |
| Perlu buka 2-3 app berbeda untuk baca & tulis | Satu aplikasi, dua mode |
| App proprietary, data terkunci di cloud | 100% offline, data milikmu |
| UI tidak cocok dengan tiling WM (Hyprland) | Integrasi langsung dengan Hyprland IPC |

### Filosofi

> *"Satu aplikasi, dua dunia. Baca dengan nyaman, tulis dengan fokus.
> Tanpa bloat, tanpa telemetry, tanpa internet."*

---

## ✨ Fitur Saat Ini (v0.1.0 — MVP)

### 📖 Mode Baca (Reader)
- [x] Buka file EPUB, TXT, Markdown
- [x] Atur ukuran font (A+/A-)
- [x] Scroll reading dengan progress
- [x] Dark theme (Tokyo Night)

### ✍️ Mode Tulis (Writer)
- [x] Text editor dengan syntax highlighting dasar
- [x] Word counter real-time
- [x] Buka & simpan file (Markdown, TXT)
- [x] Native file dialog (Wayland)

### 🏠 Library
- [x] Daftar novel dengan progress baca
- [x] Pencarian novel
- [x] Tampilan card-based

### ⚙️ Umum
- [x] Sidebar navigation
- [x] Dark theme bawaan
- [x] Single binary, tanpa dependency runtime

---

## 🛠️ Tech Stack

### Core

| Komponen | Teknologi | Alasan |
|---|---|---|
| **Bahasa** | Rust (2021 edition) | Memory-safe, performa tinggi, single binary |
| **GUI Framework** | [Iced 0.13](https://iced.rs) | Elm architecture, native Wayland via winit, ringan |
| **Async Runtime** | [Tokio](https://tokio.rs) | Async I/O untuk parsing file besar |
| **Database** | [SQLite](https://sqlite.org) via `rusqlite` | Embedded, tanpa server, cocok untuk data lokal |

### Parsing & Format

| Fungsi | Crate | Keterangan |
|---|---|---|
| **EPUB Reader** | [`epub`](https://crates.io/crates/epub) | Parse EPUB 2 & 3 |
| **Markdown** | [`pulldown-cmark`](https://crates.io/crates/pulldown-cmark) | CommonMark parser |
| **Export PDF** | [`printpdf`](https://crates.io/crates/printpdf) *(roadmap)* | Generate PDF dari Markdown |
| **Export EPUB** | [`epub-builder`](https://crates.io/crates/epub-builder) *(roadmap)* | Build EPUB dari chapter |

### Integrasi Sistem

| Fungsi | Teknologi | Keterangan |
|---|---|---|
| **File Dialog** | [`rfd`](https://crates.io/crates/rfd) | Native dialog, support Wayland portal |
| **Hyprland IPC** | `hyprctl` via `std::process` | Window rules, workspace, idle inhibit |
| **Desktop Entry** | `.desktop` file | Terintegrasi dengan app launcher |
| **Packaging** | `PKGBUILD` (AUR) | Mudah install di Arch |

### Arsitektur

```
┌─────────────────────────────────────────────┐
│                 NovelCraft                   │
├──────────┬──────────┬───────────┬───────────┤
│  Reader  │  Writer  │  Library  │ Settings  │
│  Module  │  Module  │  Module   │  Module   │
├──────────┴──────────┴───────────┴───────────┤
│              Core Engine                     │
│  ┌─────────┐ ┌──────────┐ ┌──────────────┐  │
│  │ Parser  │ │ Storage  │ │   Exporter   │  │
│  │ (EPUB/  │ │ (SQLite) │ │ (PDF/EPUB)   │  │
│  │  MD/TXT)│ │          │ │              │  │
│  └─────────┘ └──────────┘ └──────────────┘  │
├─────────────────────────────────────────────┤
│         Hyprland IPC Integration            │
│    (workspace, float, idle-inhibit)         │
├─────────────────────────────────────────────┤
│     Iced GUI  →  winit  →  Wayland          │
└─────────────────────────────────────────────┘
```

---

## 🚀 Instalasi & Build

### Prasyarat (Arch Linux)

```bash
# System dependencies
sudo pacman -S rustup gcc pkg-config wayland-protocols \
    libxkbcommon fontconfig

# Setup Rust toolchain
rustup default stable
```

### Build dari Source

```bash
# Clone repository
git clone https://github.com/username/novelcraft.git
cd novelcraft

# Development build
cargo run

# Release build (optimized)
cargo build --release

# Binary ada di:
./target/release/novelcraft
```

### Install via AUR *(coming soon)*

```bash
yay -S novelcraft
```

### Desktop Entry

```bash
# Copy desktop file
sudo cp novelcraft.desktop /usr/share/applications/

# Hyprland window rules (opsional, tambahkan ke hyprland.conf)
windowrulev2 = float, class:^(novelcraft)$
windowrulev2 = size 1100 750, class:^(novelcraft)$
windowrulev2 = center, class:^(novelcraft)$
```

---

## 🗺️ Roadmap

### Phase 1 — MVP ✅ *(sekarang)*
> Fondasi aplikasi, bisa baca & tulis dasar.
- [x] Project structure & Iced setup
- [x] Sidebar navigation
- [x] Text editor (Writer)
- [x] EPUB/TXT reader (Reader)
- [x] File open/save dialog
- [x] Library view dengan dummy data
- [x] Dark theme

### Phase 2 — Novel Writing Suite *(target: v0.3)*
> Fitur khusus untuk menulis novel secara serius.
- [ ] Chapter management (tree view, drag & drop)
- [ ] Character database (nama, deskripsi, relasi)
- [ ] World-building notes (tempat, waktu, lore)
- [ ] Word count target harian + progress bar
- [ ] Writing streak tracker (kalender)
- [ ] Distraction-free / Zen mode (fullscreen, hide sidebar)
- [ ] Auto-save setiap 30 detik
- [ ] SQLite storage untuk semua metadata

### Phase 3 — Advanced Reader *(target: v0.5)*
> Pengalaman membaca yang lebih baik.
- [ ] EPUB parsing lengkap (gambar, footnote, TOC)
- [ ] Bookmark & highlight dengan warna
- [ ] Reading progress sync (per chapter)
- [ ] Text-to-speech (via `piper` / `espeak-ng`)
- [ ] Custom CSS untuk rendering EPUB
- [ ] Support format CBZ/CBR (komik/manga)
- [ ] Night mode dengan warm filter

### Phase 4 — Export & Publishing *(target: v0.7)*
> Dari draft ke novel jadi.
- [ ] Export ke EPUB (valid EPUB 3)
- [ ] Export ke PDF (via `printpdf`)
- [ ] Export ke DOCX (via `docx-rs`)
- [ ] Custom cover image
- [ ] Metadata editor (judul, penulis, ISBN, sinopsis)
- [ ] Preview sebelum export

### Phase 5 — Hyprland Deep Integration *(target: v0.8)*
> Integrasi mendalam dengan ekosistem Hyprland.
- [ ] Auto workspace assignment (baca = ws5, tulis = ws6)
- [ ] Idle inhibit saat membaca (mencegah screen off)
- [ ] Hyprland IPC: toggle float/tiling dari dalam app
- [ ] Match theme dengan Hyprland/pywal colors
- [ ] Notification via `dunst`/`mako` (writing streak reminder)
- [ ] Global shortcut via Hyprland bind (quick capture ide)

### Phase 6 — Cloud & Sync *(target: v1.0)*
> Sinkronisasi antar device, tetap private.
- [ ] Sync via WebDAV (Nextcloud)
- [ ] Sync via Git (auto commit & push)
- [ ] End-to-end encryption untuk data novel
- [ ] Conflict resolution
- [ ] Export backup (ZIP dengan seluruh project)

### Phase 7 — AI-Assisted *(target: v1.x, eksperimental)*
> Bantuan AI lokal, tanpa cloud.
- [ ] Local LLM integration (via `llama.cpp` Rust bindings)
- [ ] Grammar & typo checker (via `tree-sitter`)
- [ ] Plot hole detector
- [ ] Character consistency checker
- [ ] Writing style analyzer

---

## 📁 Struktur Project

```
novelcraft/
├── Cargo.toml              # Dependencies & metadata
├── README.md               # File ini
├── LICENSE                 # MIT License
├── novelcraft.desktop      # Desktop entry untuk launcher
├── PKGBUILD                # AUR package (coming soon)
├── src/
│   ├── main.rs             # Entry point, Iced app setup
│   ├── app.rs              # App state & message handling
│   ├── ui/
│   │   ├── mod.rs
│   │   ├── reader.rs       # UI membaca novel
│   │   ├── writer.rs       # UI menulis novel
│   │   ├── library.rs      # UI manajemen library
│   │   └── settings.rs     # UI pengaturan
│   ├── core/
│   │   ├── mod.rs
│   │   ├── parser.rs       # Parse EPUB/TXT/Markdown
│   │   ├── storage.rs      # SQLite database layer
│   │   ├── exporter.rs     # Export ke EPUB/PDF/DOCX
│   │   └── hyprland.rs     # Hyprland IPC integration
│   └── theme/
│       ├── mod.rs
│       └── colors.rs       # Custom theme & colors
├── assets/
│   ├── fonts/              # Embedded fonts (opsional)
│   └── icons/              # App icons
└── data/
    └── novels.db           # SQLite database (runtime)
```

---

## 🤝 Kontribusi

Proyek ini bersifat **pribadi** untuk saat ini, tapi saran dan feedback
sangat diterima!

Jika ingin berkontribusi di masa depan:

1. Fork repository
2. Buat branch fitur (`git checkout -b fitur/chapter-tree`)
3. Commit perubahan (`git commit -m "feat: tambah chapter tree"`)
4. Push ke branch (`git push origin fitur/chapter-tree`)
5. Buat Pull Request

### Development Guidelines
- Gunakan `cargo clippy` sebelum commit
- Format kode dengan `cargo fmt`
- Test dengan `cargo test`
- Commit message mengikuti [Conventional Commits](https://conventionalcommits.org)

---

## 📜 Lisensi

Proyek ini dilisensikan di bawah [MIT License](LICENSE).

Bebas digunakan, dimodifikasi, dan didistribusikan.

---

<div align="center">

**Dibuat dengan 🦀 Rust + ☕ kopi di Arch Linux**

*NovelCraft — Karena menulis novel seharusnya menyenangkan, bukan berat.*

</div>
