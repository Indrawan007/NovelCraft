<div align="center">

# 📚 NovelCraft

**Aplikasi Baca & Tulis Novel — Native, Ringan, Wayland-First**

*Dibangun dengan Rust untuk Arch Linux + Hyprland*

[![Rust](https://img.shields.io/badge/Rust-stable-orange?logo=rust)](https://rust-lang.org)
[![Iced](https://img.shields.io/badge/GUI-Iced%200.13-blue)](https://iced.rs)
[![Wayland](https://img.shields.io/badge/Display-Wayland-green)](https://wayland.freedesktop.org)
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

## 🚧 Status Implementasi

Proyek masih dalam tahap awal. Aplikasi Rust saat ini memiliki jendela Iced,
tema gelap NovelCraft yang mengikuti palet mockup, dan navigasi antara Library, Reader, Writer, dan Settings. Writer
menyediakan editor teks, hitungan kata, draft baru, syntax highlighting Markdown
dasar, preview Markdown langsung, serta buka dan simpan file TXT/Markdown.
Highlighting aktif untuk draft baru dan file `.md`/`.markdown` (termasuk huruf
besar); file TXT ditampilkan sebagai teks biasa. Warna editor menggunakan tema
highlighter gelap bawaan Iced yang berbeda dari palet tema aplikasi. Preview memakai isi editor saat ini dan
tidak menyimpan file atau membuka tautan secara otomatis. Tombol **Mode fokus**
di Writer menyembunyikan sidebar; tekan **Keluar mode fokus** untuk
menampilkannya kembali. Mode fokus tidak mengaktifkan fullscreen dan otomatis
berakhir saat berpindah halaman. Shortcut navigasi tetap tersedia. Writer juga
menyediakan target jumlah kata per draft dan progress bar; target hanya berlaku
selama sesi dan direset ketika draft baru dibuat atau sebuah file berhasil dibuka.
Target menghitung seluruh isi draft berdasarkan pemisah whitespace, termasuk
teks yang sudah ada dan markup Markdown. Hitungan diperbarui saat draft dibuka
atau diedit, bukan setiap tampilan dibangun ulang. Perhitungan masih menelusuri
seluruh draft saat diedit; preview Markdown juga diparse ulang setelah edit, sehingga
draft besar tetap dapat terasa lambat. Ini bukan target harian; mengubah target
tidak mengubah atau menyimpan isi file. Kosongkan kolom untuk menyembunyikan target.
Tombol **Fullscreen (F11)** terpisah dari mode fokus dan tersedia di semua halaman;
tekan lagi untuk kembali ke mode berjendela. Fullscreen dapat digabungkan dengan
mode fokus, dan tidak otomatis berakhir saat berpindah halaman atau disimpan
antar-sesi. Permintaan fullscreen bergantung pada dukungan compositor/window
manager; tombol membaca mode jendela saat ditekan, tetapi perubahan eksternal atau
kompositor dapat memengaruhi hasilnya. Mengubah ukuran tampilan dapat menggeser
teks Reader dan membatalkan pengukuran gulir sebelumnya.
Reader dapat membuka file EPUB, TXT/Markdown UTF-8, menggulir teks, dan mengatur
ukuran font (12–32). Tombol A+/A− menyimpan ukuran secara lokal dan menerapkannya
setelah penyimpanan berhasil; ukuran dipulihkan saat aplikasi dibuka ulang.
Selama Settings dimuat/disimpan atau gagal dimuat, tombol font dinonaktifkan.
Jika Settings gagal dimuat, perbaiki file dan gunakan **Coba muat ulang Settings**;
jika penyimpanan gagal, ukuran tetap dan tombol dapat dicoba lagi. **Mode fokus** di Reader juga menyembunyikan sidebar tanpa fullscreen;
berpindah halaman mengakhirinya. Mengubah lebar area baca dapat menggeser posisi
teks karena pembungkusan ulang; indikator gulir menjadi `—` hingga terukur lagi. Posisi gulir ditampilkan untuk dokumen teks atau bagian EPUB saat
konten dapat digulir; tanda `—` berarti posisi belum tersedia. Angka tersebut
adalah posisi scrollbar, bukan persentase buku yang telah dibaca. Gunakan tombol
**Simpan posisi** dan **Pulihkan posisi** untuk menyimpan atau memuat penanda
secara manual. Saat berpindah halaman dalam sesi yang sama, posisi gulir yang
sudah terukur dipulihkan secara perkiraan ketika kembali ke Reader; ini tidak
menulis atau mengganti penanda manual. Jika posisi belum terukur, termasuk setelah
perubahan ukuran tampilan atau font, posisi dapat kembali ke awal. Pemulihan
sementara ini tidak bertahan setelah aplikasi ditutup.
EPUB ditampilkan sebagai teks per bagian dalam urutan spine (tidak selalu sama
dengan bab). Gunakan tombol sebelumnya/berikutnya atau kolom **Ke bagian** untuk
melompat langsung ke nomor bagian; lompatan dimulai dari atas bagian tersebut.
Pencarian teks EPUB menemukan bagian sebelumnya/berikutnya dengan mengubah teks
ke huruf kecil Unicode (bukan case folding penuh), memutar kembali saat mencapai
ujung, dan kembali ke awal bagian yang cocok. Pencarian tidak menyorot atau melompat ke posisi kata, hanya mencari teks
hasil ekstraksi, dan dapat terasa lambat pada buku besar. Untuk pencarian berulang
dengan kata yang sama pada dokumen yang sama, hasil tiap bagian yang sudah diperiksa
disimpan sementara; pencarian pertama tetap memproses bagian yang dilewati. Reader menampilkan daftar
isi EPUB 3 `nav` atau NCX jika tujuannya cocok dengan bagian spine; `nav` yang
valid didahulukan. Jika `nav` tidak tersedia atau tidak dapat digunakan, NCX
menjadi cadangan. Pilihan daftar isi membuka awal bagian, bukan posisi jangkar
spesifik; entri yang tidak cocok dilewati. Tautan eksternal, query URL, beberapa
path yang kompleks, serta `nav` yang bukan XHTML UTF-8 valid, memiliki DTD
selain `<!DOCTYPE html>`, atau lebih dari 2 MiB tidak didukung. Jika daftar isi tidak tersedia, gunakan nomor bagian. Gambar dan
gaya belum dirender. Markdown
memiliki tampilan dasar untuk judul, paragraf, daftar, penekanan, dan blok kode,
serta pilihan untuk melihat sumber mentah. Tabel, gambar, HTML, dan elemen lain
yang belum didukung dapat dilihat di mode sumber. Tautan tidak dibuka otomatis.
Untuk TXT dan Markdown, **Cari dalam file** mencari per baris pada sumber UTF-8
(Markdown termasuk markup), lalu menampilkan nomor baris dan cuplikan. Tombol
sebelumnya/berikutnya berputar antarbaris yang cocok, bukan antar-kemunculan
pada baris yang sama; cuplikan berpusat pada kecocokan pertama di baris itu.
Baris dipisahkan oleh LF atau CRLF. Pencarian mengabaikan kapitalisasi dengan lowercase per
karakter Unicode (bukan case folding atau normalisasi Unicode penuh), tidak
mencocokkan lintas baris, dan tidak menggeser atau menyorot teks dokumen.
Pencarian dapat lambat pada file besar; membuka/menutup panel dapat mengubah
layout dan menghapus pengukuran posisi gulir sementara.
Library menampilkan kartu berbaris yang membungkus sesuai ruang berisi nama dan lokasi file, menyediakan
pencarian nama file, menampilkan penanda manual yang tersimpan, dan dapat membuka
file di Reader. Settings menyediakan pilihan tema gelap yang disimpan lokal;
integrasi Hyprland belum dibuat.
`mockup.html` adalah target desain visual. Shell dan Library mulai disesuaikan;
Reader, Writer, dan Settings belum sepenuhnya mengikuti mockup. Data dan kontrol
contoh di mockup tidak otomatis menjadi fitur aplikasi native.

### Proyek dan chapter (tahap awal)

Di Writer, **Proyek baru** memilih folder yang sudah ada dan membuat `.novelcraft.json`
di dalamnya; file metadata yang sudah ada tidak ditimpa. **Buka proyek** memilih
folder yang berisi metadata itu. Masukkan judul dan pilih **Tambah chapter** untuk
membuat file kosong `chapter-N.md`; klik chapter pada daftar untuk membukanya di
editor, lalu simpan secara manual seperti draft biasa. Folder dapat berisi file lain:
nomor yang sudah dipakai dilewati, bukan ditimpa. Judul disimpan di metadata,
bukan otomatis ditulis ke isi Markdown. Proyek tidak dibuka otomatis pada sesi berikutnya.

Membuka chapter lain meminta konfirmasi jika draft saat ini belum disimpan;
menolak konfirmasi mempertahankan draft. Beralih proyek tidak mengganti isi editor.
File chapter tetap file Markdown biasa, dapat dibuka di editor lain, dan tetap
mengikuti pemeriksaan konflik serta batasan penyimpanan Writer di bawah. **Simpan
sebagai** menulis ke lokasi lain tetapi tidak mengubah daftar chapter proyek.
Tombol **Judul** mengubah judul di metadata tanpa mengganti nama file; **Atas**
dan **Bawah** mengubah urutan dalam daftar. **Hapus** meminta konfirmasi dan
hanya menghapus entri dari daftar, bukan file Markdown atau draft yang sedang
terbuka. File yang dilepas belum bisa ditambahkan kembali melalui daftar chapter;
file tetap dapat dibuka melalui tombol **Buka** biasa. Saat ini belum ada impor
file chapter yang sudah ada, tree view, drag & drop, atau autosave.

Metadata proyek divalidasi dan perubahan eksternal diperiksa sebelum penulisan;
metadata rusak, versi tidak didukung, dan symbolic link ditolak. Jika pembuatan
chapter berhasil tetapi penulisan metadata gagal atau berkonflik, file kosong
mungkin tertinggal di folder dan tidak masuk daftar; pesan kesalahan menyebutkan
nama file tersebut. Pembuatan metadata proyek yang gagal saat menulis juga dapat
meninggalkan metadata belum lengkap. Tidak ada penguncian lintas instance dan
pemeriksaan konflik tidak menutup semua race; gunakan satu instance dan buat backup.

Writer meminta konfirmasi sebelum membuat draft baru, membuka file lain, atau
menutup jendela jika draft memiliki perubahan yang belum disimpan. Saat operasi file Reader, Writer, Library, atau
Settings masih berlangsung, selesaikan dialog atau tunggu prosesnya sebelum
menutup aplikasi. Simpan draft secara manual; peringatan penutupan tidak dapat
melindungi dari penghentian paksa, crash, atau kehilangan daya. Dialog file
Wayland memerlukan portal desktop yang berfungsi.

Penyimpanan Writer menulis file sementara di direktori tujuan sebelum mengganti
file lama. Pada Linux, symbolic link yang valid diikuti dan izin file dipertahankan;
file dengan hard link atau yang pemilik/grupnya akan berubah ditolak. File baru
dibuat dengan izin privat (0600) pada Linux. Atribut lanjutan seperti ACL dan
extended attributes belum dipertahankan.

Writer memeriksa perubahan isi file di luar aplikasi sebelum menyimpan. Jika file
berubah atau terhapus, gunakan **Simpan sebagai** untuk menyimpan draft di lokasi
lain. **Simpan sebagai** meminta konfirmasi tambahan sebelum mengganti file lain
yang sudah ada (dialog sistem juga mungkin meminta konfirmasi). Setelah disetujui,
Writer memeriksa kembali isi dan tujuan path; jika berubah, simpan dibatalkan dan
draft tetap ada. Pemeriksaan ini tidak mengunci file dan tidak mendeteksi semua
penggantian file dengan isi identik: perubahan antara pemeriksaan terakhir dan
penggantian masih dapat terlewat. Tetap buat backup.
Menyimpan file yang belum diedit mempertahankan isi UTF-8 aslinya, termasuk
format newline. Setelah diedit, ekspor editor Iced menormalkan pemisah baris
menjadi LF, dapat menambahkan newline di akhir, dan dapat mengurangi baris kosong
akhir. Writer meminta konfirmasi sebelum setiap penyimpanan draft yang diedit,
termasuk Simpan sebagai; batal mempertahankan draft tanpa menulis file. Dialog
menampilkan jumlah pemisah baris pada versi tersimpan dan pada hasil yang akan
ditulis, termasuk jumlah pemisah di akhir. Perbedaannya dapat berasal dari edit
pengguna maupun normalisasi, sehingga bukan diff isi. Periksa hasil simpan dan
gunakan backup bila format baris penting.

Library menyimpan lokasi file di `$XDG_DATA_HOME/novelcraft/library.json`, atau
`~/.local/share/novelcraft/library.json` jika `XDG_DATA_HOME` tidak ditetapkan
sebagai path absolut. File asli tidak dipindahkan atau dihapus. Jika file asli
pindah, gunakan **Ubah lokasi** pada kartu untuk memilih file baru lalu
konfirmasikan perubahan. Hanya daftar lokasi yang berubah; file tidak dipindahkan,
dihapus, atau dibandingkan isinya. Penanda path lama tidak dipindahkan; jika path
baru sudah memiliki penanda tersimpan, penanda itu tetap dipakai. Lokasi baru harus
berupa file biasa dengan ekstensi yang didukung dan tidak boleh sama persis dengan
entri lain; alias path yang berbeda tidak dideteksi sebagai duplikat. Lokasi diperiksa
kembali sebelum menyimpan, tetapi tidak dikunci dan bisa berubah sesudahnya. Jika file
terhapus, hapus entrinya atau pilih lokasi lain.

Kartu menampilkan hasil pemeriksaan lokasi terakhir (file ditemukan, tidak ditemukan,
bukan file biasa, atau tidak dapat diperiksa). Pemeriksaan berjalan di latar belakang
saat Library dimuat, daftarnya berubah, atau halaman Library dibuka; **Periksa ulang
lokasi** juga tersedia. Status dapat menjadi usang setelah pemeriksaan, tidak memastikan
file bisa dibaca atau isinya valid, dan tidak mencegah pembukaan; symbolic link diikuti.
Filesystem yang lambat bisa menunda hasil seluruh pemeriksaan, tetapi tidak mengunci
kontrol Library lainnya. Pemeriksaan tidak menghapus entri atau mengubah file.

Library dapat diurutkan berdasarkan urutan daftar, nama A–Z, atau nama Z–A;
tombol urutan berputar di antara ketiganya. Urutan nama menggunakan lowercase
Unicode dan perbandingan leksikografis, bukan aturan bahasa lokal; nama yang sama
mempertahankan urutan daftar. Filter **Hanya tidak ditemukan** dapat digabungkan
dengan pencarian nama. Filter hanya menampilkan status “tidak ditemukan” dari
pemeriksaan terakhir, bukan lokasi yang belum diperiksa, tidak dapat diperiksa,
atau bukan file biasa. Saat pemeriksaan berjalan, hasil bisa kosong sampai selesai.
Urutan dan filter hanya berlaku selama sesi dan tidak mengubah data Library.

**Hapus dari daftar** meminta
konfirmasi dan hanya menghapus entri Library, bukan file maupun penanda. Jika
data Library memuat beberapa entri dengan path identik, semuanya dihapus setelah
peringatan dan konfirmasi. Library menolak menimpa daftar yang berubah sejak dimuat; gunakan **Muat ulang Library** sebelum
mencoba menambahkan atau menghapus entri lagi. File Library yang rusak, bukan
file biasa, atau berupa symbolic link ditolak tanpa diganti. Pada Linux, daftar
baru ditulis dengan izin privat (maksimal 0600, sesuai umask); ACL dan atribut
tambahan tidak dipertahankan.
Pemeriksaan konflik tidak mengunci file: perubahan setelah pemeriksaan terakhir
masih bisa terlewat. Hindari beberapa instance dan tetap buat backup.

Penanda Reader disimpan di `novelcraft/bookmarks.json` pada direktori data yang
sama. Satu posisi disimpan per path file; memindahkan file membuatnya dianggap
file lain. Pemulihan memakai bagian EPUB dan persentase gulir perkiraan, sehingga
perubahan isi, ukuran jendela, atau font dapat menggeser posisi. Jika posisi
gulir belum terukur (`—`), hanya bagian yang disimpan dan dipulihkan dari atas.
Penanda tidak disinkronkan antar-instance dan tidak disimpan secara otomatis.
Tombol **Hapus penanda** di Reader meminta konfirmasi dan hanya menghapus
penanda untuk file yang terbuka, bukan file bukunya; penanda file lain tetap ada.
Penyimpanan dan penghapusan menolak file penanda yang berupa symbolic link,
bukan file biasa, atau tidak valid. Sebelum mengganti file, aplikasi memeriksa
apakah isinya berubah sejak operasi dimulai dan membatalkan operasi jika berbeda.
Pemeriksaan ini tidak mengunci file: perubahan setelah pemeriksaan terakhir masih
bisa terlewat; hindari menjalankan beberapa instance dan tetap buat backup.
Kartu Library menampilkan penanda yang terakhir disimpan secara manual, bukan
posisi terkini. Perkiraan posisi keseluruhan TXT/Markdown memakai posisi gulir;
untuk EPUB dihitung dari nomor bagian, posisi gulir bagian, dan jumlah bagian
saat penanda disimpan. Setiap bagian EPUB berbobot sama meskipun panjangnya
berbeda, sehingga angka ini bukan ukuran banyaknya teks yang telah dibaca.
Perubahan isi file, ukuran jendela, atau font dapat membuat perkiraan meleset.
Jika posisi gulir tidak tersedia, perkiraan tidak ditampilkan. Penanda EPUB
lama yang belum menyimpan jumlah bagian tetap dapat dipulihkan; simpan ulang
penanda di Reader untuk mendapatkan perkiraan. Tombol **Lanjut baca** pada kartu
dengan penanda membuka file dan mencoba memulihkan penanda tersebut; tombol
**Baca** tetap membuka dari awal. Jika penanda hilang atau tidak valid, dokumen
yang berhasil dibuka dimulai dari awal dengan pemberitahuan. Menghapus kartu dari Library
tidak menghapus file atau penanda.

Daftar di bawah ini adalah **target MVP**, bukan fitur yang sudah selesai.

## ✨ Target Fitur MVP (v0.1.0)

### 📖 Mode Baca (Reader)
- [x] Buka file TXT dan Markdown UTF-8
- [x] Buka EPUB sebagai teks, lompat ke bagian spine, dan cari bagian berdasarkan teks
- [x] Daftar isi dasar EPUB 3 nav dan NCX dipetakan ke bagian spine (tanpa navigasi jangkar)
- [x] Render Markdown dasar dan lihat sumber
- [x] Atur ukuran font (A+/A-) dan pulihkan antar-sesi
- [x] Mode fokus Reader (sembunyikan sidebar; tanpa fullscreen)
- [x] Scroll reading dan indikator posisi gulir per dokumen/bagian
- [x] Simpan, pulihkan, dan hapus satu penanda posisi manual per file antar-sesi
- [x] Tema gelap NovelCraft dan tema bawaan Iced

### ✍️ Mode Tulis (Writer)
- [x] Proyek chapter dasar: buat/buka folder, tambah, buka, ubah judul, susun ulang dan hapus entri chapter Markdown
- [x] Draft baru dengan konfirmasi untuk perubahan yang belum disimpan
- [x] Preview Markdown dasar di samping editor
- [x] Mode fokus Writer (sembunyikan sidebar; tanpa fullscreen)
- [x] Text editor dengan syntax highlighting Markdown dasar
- [x] Word counter real-time dan target jumlah kata sementara per draft
- [x] Buka & simpan file (Markdown, TXT)
- [x] Native file dialog (Wayland, via portal)

### 🏠 Library
- [x] Tampilkan penanda baca manual per file pada kartu dan lanjut baca dari penanda
- [x] Perkiraan posisi keseluruhan dari penanda manual (EPUB: bobot bagian sama)
- [x] Pencarian berdasarkan nama file, pengurutan tampilan, dan filter lokasi hilang
- [x] Tampilan kartu yang membungkus sesuai lebar Library

### ⚙️ Umum
- [x] Sidebar navigation
- [x] Pilihan tema NovelCraft dan tema gelap bawaan Iced yang disimpan lokal
- [ ] Single binary, tanpa dependency runtime

---

## 🎨 Tema

Di Settings, pilih NovelCraft, Tokyo Night, Dark, Nord, atau Dracula. Tema berubah setelah
berhasil disimpan dan dipulihkan saat aplikasi dibuka ulang; sebelum pengaturan
dimuat, jika gagal dimuat, atau jika belum ada pilihan tema, digunakan NovelCraft.
Pilihan tema lama yang tersimpan tetap dipertahankan. Tema berlaku pada antarmuka
dan Markdown Reader/preview Writer. Syntax highlighting editor tetap menggunakan
palet gelap bawaan Iced yang terpisah.

Tema dan ukuran font Reader tersimpan di `$XDG_CONFIG_HOME/novelcraft/settings.json`, atau
`~/.config/novelcraft/settings.json` jika `XDG_CONFIG_HOME` tidak absolut. File
rusak, tema/ukuran font tidak valid, dan symbolic link ditolak tanpa ditimpa; perbaiki file
lalu muat ulang di Settings. Jika penyimpanan gagal, tema aktif tidak berubah.
Penulisan memakai file sementara dan rename; di Linux file hasil simpan privat
(0600), dan ACL/atribut tambahan tidak dipertahankan. Tidak ada sinkronisasi atau
penguncian antar-instance; perubahan antara pemeriksaan terakhir dan rename
masih dapat terlewat. Hindari menjalankan beberapa instance bersamaan.

---

## ⌨️ Shortcut Keyboard (Linux)

| Shortcut | Aksi |
|---|---|
| `F11` | Minta masuk/keluar fullscreen |
| `Ctrl+1`, `Ctrl+2`, `Ctrl+3` | Buka Library, Reader, atau Writer |
| `Ctrl+,` | Buka Settings |
| `Ctrl+PageUp`, `Ctrl+PageDown` | Bagian EPUB sebelumnya atau berikutnya di Reader |
| `Ctrl+N`, `Ctrl+O` | Draft baru atau buka file di Writer |
| `Ctrl+S`, `Ctrl+Shift+S` | Simpan atau simpan sebagai di Writer |

Shortcut Reader hanya aktif untuk EPUB di halaman Reader; bagian tidak berputar
saat mencapai awal atau akhir. Shortcut Writer hanya aktif saat halaman Writer
terbuka. Keduanya tetap mengikuti konfirmasi serta status operasi file. Kontrol yang sedang fokus atau shortcut
sistem dapat menangani kombinasi tombol lebih dahulu.

---

## 🛠️ Tech Stack yang Direncanakan

Tidak semua komponen di bawah sudah ditambahkan atau diimplementasikan.

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
| **Markdown** | Iced Markdown (`pulldown-cmark`) dan Iced highlighter (`syntect`) | Rendering dasar, mode sumber, dan highlighting editor |
| **Export PDF** | [`printpdf`](https://crates.io/crates/printpdf) *(roadmap)* | Generate PDF dari Markdown |
| **Export EPUB** | [`epub-builder`](https://crates.io/crates/epub-builder) *(roadmap)* | Build EPUB dari chapter |

### Integrasi Sistem

| Fungsi | Teknologi | Keterangan |
|---|---|---|
| **File Dialog** | [`rfd`](https://crates.io/crates/rfd) | Native dialog, support Wayland portal |
| **Hyprland IPC** | `hyprctl` via `std::process` | Window rules, workspace, idle inhibit |
| **Desktop Entry** | `.desktop` file | Terintegrasi dengan app launcher |
| **Packaging** | `PKGBUILD` (AUR) | Mudah install di Arch |

### Arsitektur yang Direncanakan

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
git clone https://github.com/Indrawan007/NovelCraft.git
cd NovelCraft

# Development build
cargo run --locked

# Pemeriksaan dan unit test
cargo check --locked --all-targets
cargo test --locked --all-targets

# Release build (optimized)
cargo build --release --locked

# Binary ada di:
./target/release/novelcraft
```

`Cargo.lock` disertakan agar build memakai versi dependensi yang sama dengan CI.
Perbarui lockfile secara sengaja ketika mengubah dependensi; `--locked` akan
gagal jika lockfile perlu diperbarui. Ini tidak menjamin binary identik di
semua sistem atau menghilangkan kebutuhan dependency runtime sistem.

### Install via AUR *(coming soon)*

```bash
yay -S novelcraft
```

### Desktop Entry

Launcher dan ikon tersedia untuk instalasi per pengguna. Perintah berikut
memasang binary release ke `~/.local/bin` dan launcher/ikon ke direktori data
XDG (atau `~/.local/share` jika `XDG_DATA_HOME` tidak absolut):

```bash
cargo build --release --locked
case "${XDG_DATA_HOME:-}" in
  /*) data_home="$XDG_DATA_HOME" ;;
  *)  data_home="$HOME/.local/share" ;;
esac
install -Dm755 target/release/novelcraft "$HOME/.local/bin/novelcraft"
install -Dm644 packaging/novelcraft.desktop "$data_home/applications/novelcraft.desktop"
install -Dm644 packaging/novelcraft.svg "$data_home/icons/hicolor/scalable/apps/novelcraft.svg"
```

Perintah `install` mengganti file dengan nama yang sama. Pastikan `~/.local/bin`
ada dalam `PATH` **sesi desktop** yang menjalankan launcher; jika tidak, menu
mungkin muncul tetapi gagal menjalankan aplikasi. Segarkan menu atau masuk ulang
jika entri belum muncul. Launcher mendeklarasikan dukungan EPUB, teks biasa,
dan Markdown sehingga dapat ditawarkan dalam menu **Open With**; launcher tidak
mengubah aplikasi default. Memilih satu file lokal membuka Reader dari awal,
tanpa otomatis memulihkan penanda atau menambahkan file ke Library. Dukungan MIME
dan pembaruan menu bergantung pada lingkungan desktop. Beberapa file atau URI
jarak jauh tidak didukung.

Dari terminal, satu path file juga dapat diberikan (path relatif dihitung dari
direktori kerja):

```bash
cargo run --locked -- "buku.epub"
```

Hanya ekstensi EPUB, TXT, MD, dan Markdown yang diterima (tanpa membedakan huruf
besar/kecil). Argumen tidak valid atau file gagal dibuka ditampilkan sebagai
kesalahan di Reader; aplikasi tetap terbuka. Path tidak dikanonisasi, sehingga
penanda tetap mengikuti aturan identitas path yang digunakan aplikasi. Periksa
launcher dengan `desktop-file-validate packaging/novelcraft.desktop` (paket
`desktop-file-utils` di Arch). Portal Wayland tetap diperlukan untuk dialog
file. Aturan dan integrasi Hyprland belum tersedia.

---

## 🗺️ Roadmap

### Phase 1 — MVP *(sedang dikerjakan)*
> Fondasi aplikasi, bisa baca & tulis dasar.
- [x] Project structure & Iced setup dasar
- [x] Sidebar navigation
- [x] Text editor dasar (Writer)
- [x] Reader dasar untuk TXT dan Markdown UTF-8
- [x] EPUB reader dasar berbasis teks
- [x] Rendering Markdown dasar
- [x] File open/save dialog untuk Writer
- [x] Library dasar: daftar lokasi, pencarian, dan buka di Reader
- [x] Tema gelap NovelCraft dan tema bawaan Iced

### Phase 2 — Novel Writing Suite *(target: v0.3)*
> Fitur khusus untuk menulis novel secara serius.
- [ ] Chapter management lengkap (operasi daftar dasar tersedia; impor file yang ada, tree view dan drag & drop belum)
- [ ] Character database (nama, deskripsi, relasi)
- [ ] World-building notes (tempat, waktu, lore)
- [ ] Word count target harian (target sementara per draft sudah tersedia)
- [ ] Writing streak tracker (kalender)
- [ ] Distraction-free / Zen mode lengkap (mode fokus sidebar dan fullscreen terpisah sudah tersedia)
- [ ] Auto-save setiap 30 detik
- [ ] SQLite storage untuk semua metadata

### Phase 3 — Advanced Reader *(target: v0.5)*
> Pengalaman membaca yang lebih baik.
- [ ] EPUB parsing lengkap (gambar, footnote, navigasi jangkar, dan format nav lanjutan)
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

## 📁 Struktur Project Saat Ini

```
NovelCraft/
├── Cargo.toml       # Metadata dan dependensi Rust
├── README.md        # Visi, status, dan roadmap
├── AGENT.md         # Pedoman pengembangan
├── mockup.html      # Referensi desain antarmuka
├── packaging/       # Launcher desktop dan ikon SVG
├── src/
│   ├── main.rs        # Jendela Iced, state, dan navigasi dasar
│   ├── bookmarks.rs   # Penanda baca lokal
│   ├── library.rs     # Daftar dan penyimpanan lokasi file
│   ├── reader.rs      # Pembaca teks, navigasi, dan ukuran font
│   ├── settings.rs    # Pengaturan tema lokal
│   ├── shortcuts.rs   # Pemetaan shortcut keyboard
│   ├── epub_reader.rs # Ekstraksi teks EPUB
│   └── writer.rs      # Editor, hitungan kata, dan operasi file
└── tests/fixtures/    # EPUB kecil untuk pengujian
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
- GitHub Actions menjalankan `cargo check --locked --all-targets` dan `cargo test --locked --all-targets` di Linux pada push dan pull request. Workflow juga dapat dijalankan secara manual.
- Gunakan `cargo clippy` sebelum commit
- Format kode dengan `cargo fmt`
- Test dengan `cargo test --locked`
- Commit message mengikuti [Conventional Commits](https://conventionalcommits.org)

---

## 📜 Lisensi

README ini menyebut lisensi MIT, tetapi file `LICENSE` belum tersedia di repository.
Ketentuan lisensi perlu dikonfirmasi dan ditambahkan oleh pemilik proyek.
Dependensi `epub` yang digunakan untuk membaca EPUB berlisensi GPL-3.0;
pemilik proyek perlu meninjau implikasinya terhadap rencana lisensi dan distribusi.

---

<div align="center">

**Dibuat dengan 🦀 Rust + ☕ kopi di Arch Linux**

*NovelCraft — Karena menulis novel seharusnya menyenangkan, bukan berat.*

</div>
