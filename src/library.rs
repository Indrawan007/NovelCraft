use crate::bookmarks::{self, Bookmark};
use iced::widget::{button, column, container, row, scrollable, text, text_input, Column};
use iced::{Element, Length, Task};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::AsyncWriteExt;

#[derive(Default)]
pub struct Library {
    books: Vec<PathBuf>,
    query: String,
    loaded: bool,
    snapshot: Option<Vec<u8>>,
    busy: bool,
    pending_relink: Option<(PathBuf, PathBuf)>,
    pending_remove: Option<PathBuf>,
    error: Option<String>,
    bookmarks: Option<Vec<Bookmark>>,
    bookmark_error: Option<String>,
    bookmark_revision: u64,
    availability: HashMap<PathBuf, Availability>,
    availability_revision: u64,
    availability_active: Option<u64>,
    availability_pending: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    File,
    Missing,
    NotFile,
    Unknown,
}

#[derive(Debug, Clone)]
pub enum Message {
    Load,
    Reload,
    Loaded(Result<(Vec<PathBuf>, Option<Vec<u8>>), String>),
    RefreshBookmarks,
    BookmarksLoaded(u64, Result<Vec<Bookmark>, String>),
    RefreshAvailability,
    AvailabilityLoaded(u64, HashMap<PathBuf, Availability>),
    Search(String),
    Import,
    Imported(Result<Option<PathBuf>, String>),
    Remove(PathBuf),
    ConfirmRemove,
    CancelRemove,
    Relink(PathBuf),
    Relinked(PathBuf, Result<Option<PathBuf>, String>),
    ConfirmRelink,
    CancelRelink,
    Saved(Result<(Vec<PathBuf>, Vec<u8>), String>),
    Open(PathBuf),
    Continue(PathBuf),
}

impl Library {
    pub fn is_busy(&self) -> bool {
        self.busy
    }

    fn controls_blocked(&self) -> bool {
        self.busy || self.pending_relink.is_some() || self.pending_remove.is_some()
    }

    pub(crate) fn can_open_book(&self) -> bool {
        !self.controls_blocked()
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Load if !self.busy && !self.loaded => {
                self.busy = true;
                self.error = None;
                return Task::perform(load(), Message::Loaded);
            }
            Message::Reload if !self.controls_blocked() && self.loaded => {
                self.busy = true;
                self.error = None;
                return Task::perform(load(), Message::Loaded);
            }
            Message::Loaded(result) => {
                self.busy = false;
                match result {
                    Ok((books, snapshot)) => {
                        self.books = books;
                        self.snapshot = snapshot;
                        self.loaded = true;
                        self.error = None;
                        return self.refresh_availability();
                    }
                    Err(error) => self.error = Some(error),
                }
            }
            Message::RefreshBookmarks => {
                self.bookmark_revision = self.bookmark_revision.wrapping_add(1);
                let revision = self.bookmark_revision;
                self.bookmarks = None;
                self.bookmark_error = None;
                return Task::perform(bookmarks::load_all(), move |result| Message::BookmarksLoaded(revision, result));
            }
            Message::BookmarksLoaded(revision, result) if revision == self.bookmark_revision => {
                match result {
                    Ok(bookmarks) => {
                        self.bookmarks = Some(bookmarks);
                        self.bookmark_error = None;
                    }
                    Err(error) => {
                        self.bookmarks = None;
                        self.bookmark_error = Some(error);
                    }
                }
            }
            Message::RefreshAvailability if self.loaded => return self.refresh_availability(),
            Message::AvailabilityLoaded(revision, availability) if self.availability_active == Some(revision) => {
                self.availability_active = None;
                if revision == self.availability_revision {
                    self.availability = availability;
                }
                if self.availability_pending {
                    self.availability_pending = false;
                    return self.start_availability_check();
                }
            }
            Message::Search(query) if !self.controls_blocked() => self.query = query,
            Message::Import if self.loaded && !self.controls_blocked() => {
                self.busy = true;
                self.error = None;
                return Task::perform(pick_book(), Message::Imported);
            }
            Message::Imported(result) => {
                self.busy = false;
                match result {
                    Ok(Some(path)) if self.books.contains(&path) => {
                        self.error = Some("File ini sudah ada di Library.".into());
                    }
                    Ok(Some(path)) => {
                        let mut books = self.books.clone();
                        books.push(path);
                        return self.save(books);
                    }
                    Ok(None) => {}
                    Err(error) => self.error = Some(error),
                }
            }
            Message::Remove(path) if self.loaded && !self.controls_blocked() && self.books.contains(&path) => {
                self.error = None;
                self.pending_remove = Some(path);
            }
            Message::CancelRemove if !self.busy => self.pending_remove = None,
            Message::ConfirmRemove if self.loaded && !self.busy => {
                if let Some(path) = self.pending_remove.take() {
                    if self.books.contains(&path) {
                        let books = without_book(&self.books, &path);
                        return self.save(books);
                    }
                    self.error = Some("Entri tidak lagi ada di Library; muat ulang Library.".into());
                }
            }
            Message::Relink(old) if self.loaded && !self.controls_blocked() && self.books.contains(&old) => {
                self.busy = true;
                self.error = None;
                return Task::perform(pick_relink_target(), move |result| Message::Relinked(old.clone(), result));
            }
            Message::Relinked(old, result) => {
                self.busy = false;
                match result {
                    Ok(Some(path)) => match replace_book(&self.books, &old, &path) {
                        Ok(_) if self.loaded => self.pending_relink = Some((old, path)),
                        Ok(_) => self.error = Some("Library belum dimuat.".into()),
                        Err(error) => self.error = Some(error),
                    },
                    Ok(None) => {}
                    Err(error) => self.error = Some(error),
                }
            }
            Message::CancelRelink if !self.busy => self.pending_relink = None,
            Message::ConfirmRelink if self.loaded && !self.busy => {
                if let Some((old, new)) = self.pending_relink.take() {
                    match replace_book(&self.books, &old, &new) {
                        Ok(books) => {
                            self.busy = true;
                            self.error = None;
                            let snapshot = self.snapshot.clone();
                            return Task::perform(async move {
                                save_relinked_to(&library_path()?, books, snapshot, &new).await
                            }, Message::Saved);
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
            }
            Message::Saved(result) => {
                self.busy = false;
                match result {
                    Ok((books, snapshot)) => {
                        self.books = books;
                        self.snapshot = Some(snapshot);
                        self.error = None;
                        return self.refresh_availability();
                    }
                    Err(error) => self.error = Some(error),
                }
            }
            _ => {}
        }
        Task::none()
    }

    fn refresh_availability(&mut self) -> Task<Message> {
        self.availability_revision = self.availability_revision.wrapping_add(1);
        self.availability.clear();
        if self.availability_active.is_some() {
            // Keep at most one scan in flight, then scan the newest book list.
            self.availability_pending = true;
            return Task::none();
        }
        self.start_availability_check()
    }

    fn start_availability_check(&mut self) -> Task<Message> {
        if self.books.is_empty() {
            return Task::none();
        }
        let revision = self.availability_revision;
        self.availability_active = Some(revision);
        Task::perform(check_availability(self.books.clone()), move |result| Message::AvailabilityLoaded(revision, result))
    }

    fn save(&mut self, books: Vec<PathBuf>) -> Task<Message> {
        self.busy = true;
        self.error = None;
        Task::perform(save(books, self.snapshot.clone()), Message::Saved)
    }

    pub fn set_error(&mut self, error: &str) {
        self.error = Some(error.to_owned());
    }

    pub fn view(&self) -> Element<'_, Message> {
        let mut import = button("Tambah file");
        let mut reload = button("Muat ulang Library");
        if self.loaded && !self.controls_blocked() {
            import = import.on_press(Message::Import);
            reload = reload.on_press(Message::Reload);
        }
        let mut search = text_input("Cari nama file...", &self.query).width(Length::Fill);
        if !self.controls_blocked() { search = search.on_input(Message::Search); }
        let mut view = column![
            text("Library").size(32),
            text("File tetap berada di lokasi aslinya; Library hanya menyimpan daftar lokasi.").size(14),
            row![
                search,
                import,
                reload,
            ].spacing(12),
        ]
        .spacing(12)
        .height(Length::Fill);

        if self.loaded && !self.books.is_empty() {
            let mut refresh = button("Periksa ulang lokasi");
            if !self.controls_blocked() && self.availability_active.is_none() {
                refresh = refresh.on_press(Message::RefreshAvailability);
            }
            view = view.push(refresh);
        }
        if let Some(path) = &self.pending_remove {
            let count = self.books.iter().filter(|book| *book == path).count();
            view = view.push(text("Hapus entri ini dari Library? File buku dan penanda tidak dihapus."));
            view = view.push(text(path.display().to_string()).size(14));
            if count > 1 {
                view = view.push(text(format!("Semua {count} entri dengan lokasi ini akan dihapus dari daftar.")).size(14));
            }
            view = view.push(row![
                button("Hapus dari daftar").on_press(Message::ConfirmRemove),
                button("Batal").on_press(Message::CancelRemove),
            ].spacing(12));
        }
        if let Some((old, new)) = &self.pending_relink {
            view = view.push(text("Ubah lokasi entri Library ini? File tidak dipindahkan atau dihapus."));
            view = view.push(text(format!("Lokasi lama: {}", old.display())).size(14));
            view = view.push(text(format!("Lokasi baru: {}", new.display())).size(14));
            view = view.push(text("Penanda lama tidak dipindahkan. Penanda yang sudah ada di lokasi baru tetap digunakan; isi file tidak dibandingkan.").size(14));
            view = view.push(row![
                button("Ubah lokasi").on_press(Message::ConfirmRelink),
                button("Batal").on_press(Message::CancelRelink),
            ].spacing(12));
        }
        if self.busy {
            view = view.push(text("Memproses Library..."));
        }
        if let Some(error) = &self.error {
            view = view.push(text(error));
        }
        if let Some(error) = &self.bookmark_error {
            view = view.push(text(format!("Penanda tidak dapat dimuat: {error}")).size(14));
            view = view.push(button("Coba muat ulang penanda").on_press(Message::RefreshBookmarks));
        } else if self.bookmarks.is_none() {
            view = view.push(text("Memuat penanda baca...").size(14));
        }
        if !self.loaded && !self.busy {
            view = view.push(button("Coba muat ulang").on_press(Message::Load));
        } else if self.loaded && self.books.is_empty() {
            view = view.push(text("Library masih kosong. Tambahkan file EPUB, TXT, atau Markdown."));
        }

        let query = self.query.to_lowercase();
        let mut list = Column::new().spacing(10).width(Length::Fill);
        let mut matches = 0;
        for path in &self.books {
            let name = path.file_name().unwrap_or(path.as_os_str()).to_string_lossy();
            if !matches_name(&name, &query) {
                continue;
            }
            matches += 1;
            let mut open = button("Baca");
            let mut remove = button("Hapus dari daftar");
            let mut relink = button("Ubah lokasi");
            if self.loaded && !self.controls_blocked() {
                open = open.on_press(Message::Open(path.clone()));
                remove = remove.on_press(Message::Remove(path.clone()));
                relink = relink.on_press(Message::Relink(path.clone()));
            }
            let mut card = column![
                text(name.to_string()).size(18),
                text(path.display().to_string()).size(12),
                text(availability_label(self.availability.get(path), self.availability_active.is_some())).size(14),
            ]
            .width(Length::Fill)
            .spacing(8);
            let mut actions = row![open].spacing(12);
            if let Some(bookmarks) = &self.bookmarks {
                let bookmark = bookmarks.iter().find(|item| item.path == *path);
                card = card.push(text(bookmark_label(path, bookmark)).size(14));
                if let Some(bookmark) = bookmark {
                    let mut continue_reading = button("Lanjut baca");
                    if self.loaded && !self.controls_blocked() {
                        continue_reading = continue_reading.on_press(Message::Continue(path.clone()));
                    }
                    actions = actions.push(continue_reading);
                    let estimate = match estimated_position(path, bookmark) {
                        Some(percent) => format!("Perkiraan posisi keseluruhan: {percent}%"),
                        None => "Perkiraan posisi keseluruhan tidak tersedia".to_owned(),
                    };
                    card = card.push(text(estimate).size(14));
                }
            }
            card = card.push(actions.push(remove)).push(relink);
            list = list.push(
                container(card)
                    .width(Length::Fill)
                    .padding(16)
                    .style(container::rounded_box),
            );
        }
        if self.loaded && !self.books.is_empty() && matches == 0 {
            view = view.push(text("Tidak ada file yang cocok."));
        }
        view.push(scrollable(list).height(Length::Fill)).into()
    }
}

fn availability_label(status: Option<&Availability>, checking: bool) -> &'static str {
    match status {
        Some(Availability::File) => "Lokasi: file ditemukan (isi belum diperiksa)",
        Some(Availability::Missing) => "Lokasi: tidak ditemukan",
        Some(Availability::NotFile) => "Lokasi: bukan file biasa",
        Some(Availability::Unknown) => "Lokasi: tidak dapat diperiksa",
        None if checking => "Lokasi: sedang diperiksa…",
        None => "Lokasi: belum diperiksa",
    }
}

async fn check_availability(books: Vec<PathBuf>) -> HashMap<PathBuf, Availability> {
    let mut statuses = HashMap::new();
    for path in books {
        if statuses.contains_key(&path) { continue; }
        let status = availability_from_metadata(tokio::fs::metadata(&path).await);
        statuses.insert(path, status);
    }
    statuses
}

fn availability_from_metadata(metadata: std::io::Result<std::fs::Metadata>) -> Availability {
    match metadata {
        Ok(metadata) if metadata.is_file() => Availability::File,
        Ok(_) => Availability::NotFile,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Availability::Missing,
        Err(_) => Availability::Unknown,
    }
}

fn estimated_position(path: &Path, bookmark: &Bookmark) -> Option<u8> {
    let scroll = bookmark.scroll_percent.filter(|percent| *percent <= 100)?;
    let is_epub = path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case("epub"));
    if !is_epub {
        return (bookmark.section == 0).then_some(scroll);
    }
    let count = bookmark.section_count?;
    if count == 0 || bookmark.section >= count {
        return None;
    }
    // Every spine section has equal weight, regardless of its length.
    let position = (bookmark.section as u128 * 100 + u128::from(scroll)) / count as u128;
    Some(position as u8)
}

fn bookmark_label(path: &Path, bookmark: Option<&Bookmark>) -> String {
    let Some(bookmark) = bookmark else {
        return "Belum ada penanda tersimpan".into();
    };
    let scroll = match bookmark.scroll_percent {
        Some(percent) => format!("gulir {percent}%"),
        None => "gulir tidak tersedia".to_owned(),
    };
    if path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case("epub")) {
        format!("Penanda tersimpan: bagian {} · {scroll}", bookmark.section as u128 + 1)
    } else {
        format!("Penanda tersimpan: {scroll}")
    }
}

fn matches_name(name: &str, lowercase_query: &str) -> bool {
    name.to_lowercase().contains(lowercase_query)
}

fn library_path() -> Result<PathBuf, String> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .ok_or_else(|| "Tidak dapat menentukan lokasi data Library (HOME tidak tersedia).".to_owned())?;
    Ok(base.join("novelcraft/library.json"))
}

async fn load() -> Result<(Vec<PathBuf>, Option<Vec<u8>>), String> {
    load_from(&library_path()?).await
}

async fn read_snapshot(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput,
                "Library harus berupa file biasa, bukan symbolic link"));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    }
    tokio::fs::read(path).await.map(Some)
}

async fn load_from(path: &Path) -> Result<(Vec<PathBuf>, Option<Vec<u8>>), String> {
    let snapshot = read_snapshot(path).await
        .map_err(|error| format!("Gagal memuat Library; file asli tidak diubah: {error}"))?;
    let books = match &snapshot {
        Some(bytes) => serde_json::from_slice(bytes)
            .map_err(|error| format!("Data Library tidak valid; file asli tidak diubah: {error}"))?,
        None => Vec::new(),
    };
    Ok((books, snapshot))
}

async fn pick_book() -> Result<Option<PathBuf>, String> {
    let Some(file) = rfd::AsyncFileDialog::new()
        .add_filter("Buku dan teks", &["epub", "txt", "md", "markdown"])
        .pick_file()
        .await
    else {
        return Ok(None);
    };
    let path = file.path().to_owned();
    let supported = path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| {
        ["epub", "txt", "md", "markdown"].iter().any(|supported| ext.eq_ignore_ascii_case(supported))
    });
    if !supported {
        return Err("Format file tidak didukung. Pilih EPUB, TXT, atau Markdown.".into());
    }
    if !path.is_absolute() {
        return Err("Lokasi file harus berupa path absolut.".into());
    }
    Ok(Some(path))
}

fn without_book(books: &[PathBuf], path: &Path) -> Vec<PathBuf> {
    books.iter().filter(|book| *book != path).cloned().collect()
}

fn replace_book(books: &[PathBuf], old: &Path, new: &Path) -> Result<Vec<PathBuf>, String> {
    if old == new { return Err("Lokasi tidak berubah; Library tetap seperti semula.".into()); }
    if books.iter().any(|path| path == new) { return Err("Lokasi baru sudah ada di Library.".into()); }
    if books.iter().filter(|path| *path == old).count() != 1 {
        return Err("Entri lama tidak ditemukan atau terduplikasi; periksa dan muat ulang Library.".into());
    }
    Ok(books.iter().map(|path| if path == old { new.to_owned() } else { path.clone() }).collect())
}

async fn pick_relink_target() -> Result<Option<PathBuf>, String> {
    let target = pick_book().await?;
    if let Some(path) = &target {
        validate_relink_target(path).await?;
    }
    Ok(target)
}

async fn validate_relink_target(path: &Path) -> Result<(), String> {
    let metadata = tokio::fs::metadata(path).await
        .map_err(|error| format!("Lokasi baru tidak dapat diakses: {error}"))?;
    if !metadata.is_file() {
        return Err("Lokasi baru harus berupa file biasa.".into());
    }
    Ok(())
}

async fn save_relinked_to(path: &Path, books: Vec<PathBuf>, expected: Option<Vec<u8>>, target: &Path) -> Result<(Vec<PathBuf>, Vec<u8>), String> {
    validate_relink_target(target).await?;
    let snapshot = save_to(path, &books, expected).await?;
    Ok((books, snapshot))
}

async fn save(books: Vec<PathBuf>, expected: Option<Vec<u8>>) -> Result<(Vec<PathBuf>, Vec<u8>), String> {
    let snapshot = save_to(&library_path()?, &books, expected).await?;
    Ok((books, snapshot))
}

// Write a temporary file in the same directory, checking the loaded snapshot
// both before starting and immediately before replacing the list.
async fn save_to(path: &Path, books: &[PathBuf], expected: Option<Vec<u8>>) -> Result<Vec<u8>, String> {
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    check_snapshot(path, &expected).await?;
    let bytes = serde_json::to_vec_pretty(books)
        .map_err(|error| format!("Gagal menyiapkan data Library: {error}"))?;
    let dir = path.parent().ok_or_else(|| "Lokasi Library tidak valid.".to_owned())?;
    tokio::fs::create_dir_all(dir).await
        .map_err(|error| format!("Gagal membuat folder Library: {error}"))?;
    let (temp, mut file) = loop {
        let temp = dir.join(format!(".library-{}-{}.tmp", std::process::id(), NEXT_TEMP.fetch_add(1, Ordering::Relaxed)));
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        match options.open(&temp).await {
            Ok(file) => break (temp, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("Gagal membuat file sementara Library: {error}")),
        }
    };
    let result: Result<(), String> = async {
        file.write_all(&bytes).await.map_err(|error| format!("Gagal menulis Library: {error}"))?;
        file.sync_all().await.map_err(|error| format!("Gagal menyimpan Library: {error}"))?;
        drop(file);
        check_snapshot(path, &expected).await?;
        tokio::fs::rename(&temp, path).await.map_err(|error| format!("Gagal mengganti Library: {error}"))
    }.await;
    if let Err(error) = result {
        let _ = tokio::fs::remove_file(&temp).await;
        return Err(error);
    }
    Ok(bytes)
}

async fn check_snapshot(path: &Path, expected: &Option<Vec<u8>>) -> Result<(), String> {
    let current = read_snapshot(path).await
        .map_err(|error| format!("Gagal memeriksa Library; file asli tidak diubah: {error}"))?;
    if &current != expected {
        return Err("Library berubah di luar aplikasi; muat ulang Library sebelum mencoba lagi.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn availability_checks_file_missing_nonfile_and_symlinks_without_changing_them() {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("novelcraft-library-availability-{}-{unique}", std::process::id()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let file = dir.join("book.txt");
        let missing = dir.join("missing.txt");
        tokio::fs::write(&file, b"isi buku").await.unwrap();
        let statuses = check_availability(vec![file.clone(), file.clone(), missing.clone(), dir.clone()]).await;
        assert_eq!(statuses.len(), 3);
        assert_eq!(statuses.get(&file), Some(&Availability::File));
        assert_eq!(statuses.get(&missing), Some(&Availability::Missing));
        assert_eq!(statuses.get(&dir), Some(&Availability::NotFile));
        assert_eq!(availability_from_metadata(Err(std::io::ErrorKind::PermissionDenied.into())), Availability::Unknown);
        assert_eq!(availability_from_metadata(Err(std::io::ErrorKind::NotFound.into())), Availability::Missing);
        assert!(availability_label(Some(&Availability::File), false).contains("isi belum diperiksa"));
        assert!(availability_label(None, true).contains("sedang diperiksa"));
        assert!(availability_label(None, false).contains("belum diperiksa"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let link = dir.join("link.txt");
            let broken = dir.join("broken.txt");
            symlink(&file, &link).unwrap();
            symlink(&missing, &broken).unwrap();
            let statuses = check_availability(vec![link.clone(), broken.clone()]).await;
            assert_eq!(statuses.get(&link), Some(&Availability::File));
            assert_eq!(statuses.get(&broken), Some(&Availability::Missing));
            assert!(tokio::fs::symlink_metadata(link).await.unwrap().file_type().is_symlink());
            assert!(tokio::fs::symlink_metadata(broken).await.unwrap().file_type().is_symlink());
        }
        assert_eq!(tokio::fs::read(&file).await.unwrap(), b"isi buku".to_vec());
        assert!(!missing.exists());
        tokio::fs::remove_file(&file).await.unwrap();
        let after_removal = check_availability(vec![file.clone()]).await;
        assert_eq!(after_removal.get(&file), Some(&Availability::Missing));
        tokio::fs::write(&file, b"dipulihkan").await.unwrap();
        let after_restore = check_availability(vec![file.clone()]).await;
        assert_eq!(after_restore.get(&file), Some(&Availability::File));
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[test]
    fn availability_discards_stale_scans_and_coalesces_rechecks() {
        let old = PathBuf::from("/old.txt");
        let new = PathBuf::from("/new.txt");
        let mut library = Library::default();
        let _ = library.update(Message::Loaded(Ok((vec![old.clone()], None))));
        let first = library.availability_active.unwrap();
        assert!(!library.is_busy());
        let _ = library.update(Message::RefreshAvailability);
        let _ = library.update(Message::RefreshAvailability);
        assert_eq!(library.availability_active, Some(first));
        assert!(library.availability_pending);
        let _ = library.update(Message::Saved(Ok((vec![new.clone()], b"[]".to_vec()))));
        let latest = library.availability_revision;
        assert_eq!(library.availability_active, Some(first));
        let _ = library.update(Message::AvailabilityLoaded(first, HashMap::from([(old.clone(), Availability::Missing)])));
        assert_eq!(library.availability_active, Some(latest));
        assert!(!library.availability_pending);
        assert!(library.availability.is_empty());
        let _ = library.update(Message::AvailabilityLoaded(first, HashMap::from([(old, Availability::File)])));
        assert_eq!(library.availability_active, Some(latest));
        let _ = library.update(Message::AvailabilityLoaded(latest, HashMap::from([(new.clone(), Availability::File)])));
        assert!(library.availability_active.is_none());
        assert_eq!(library.availability.get(&new), Some(&Availability::File));
        drop(library.view());
        let _ = library.update(Message::RefreshAvailability);
        assert!(library.availability.is_empty());
        assert!(library.availability_active.is_some());
        let _ = library.update(Message::Loaded(Ok((vec![], None))));
        assert!(library.availability_pending);
        let active = library.availability_active.unwrap();
        let _ = library.update(Message::AvailabilityLoaded(active, HashMap::from([(new, Availability::Missing)])));
        assert!(library.availability_active.is_none());
        assert!(!library.availability_pending);
        assert!(library.availability.is_empty());
    }

    #[test]
    fn returning_to_library_requests_availability_refresh() {
        let mut app = crate::NovelCraft::default();
        let _ = app.library.update(Message::Loaded(Ok((vec![PathBuf::from("/book.txt")], None))));
        let revision = app.library.availability_revision;
        let _ = app.update(crate::Message::Navigate(crate::Page::Reader));
        let _ = app.update(crate::Message::Navigate(crate::Page::Library));
        assert_ne!(app.library.availability_revision, revision);
        assert!(app.library.availability_pending);
    }

    #[test]
    fn search_matches_filename_without_case_sensitivity() {
        assert!(matches_name("Laskar Pelangi.EPUB", "pelangi"));
        assert!(!matches_name("Laskar Pelangi.EPUB", "bulan"));
        assert!(matches_name("Novel.txt", ""));
    }

    #[test]
    fn bookmark_labels_are_explicit_about_saved_section_and_scroll() {
        let item = Bookmark { path: "/a.epub".into(), section: 2, section_count: Some(4), scroll_percent: Some(40), markdown_source: false };
        assert_eq!(bookmark_label(Path::new("/a.EPUB"), Some(&item)), "Penanda tersimpan: bagian 3 · gulir 40%");
        let unmeasured = Bookmark { scroll_percent: None, ..item };
        assert_eq!(bookmark_label(Path::new("/a.md"), Some(&unmeasured)), "Penanda tersimpan: gulir tidak tersedia");
        assert_eq!(bookmark_label(Path::new("/a.md"), None), "Belum ada penanda tersimpan");
    }

    #[test]
    fn estimated_position_uses_saved_scroll_and_equal_epub_sections() {
        let mut item = Bookmark { path: "/a.epub".into(), section: 1, section_count: Some(4), scroll_percent: Some(50), markdown_source: false };
        assert_eq!(estimated_position(Path::new("/a.EPUB"), &item), Some(37));
        item.section = 3;
        item.scroll_percent = Some(100);
        assert_eq!(estimated_position(Path::new("/a.epub"), &item), Some(100));
        item.section_count = None;
        assert_eq!(estimated_position(Path::new("/a.epub"), &item), None);
        item.section_count = Some(0);
        assert_eq!(estimated_position(Path::new("/a.epub"), &item), None);
        item.section_count = Some(3);
        assert_eq!(estimated_position(Path::new("/a.epub"), &item), None);
        item.section = 0;
        item.scroll_percent = None;
        assert_eq!(estimated_position(Path::new("/a.epub"), &item), None);
        item.scroll_percent = Some(101);
        assert_eq!(estimated_position(Path::new("/a.txt"), &item), None);
        item.scroll_percent = Some(42);
        assert_eq!(estimated_position(Path::new("/a.txt"), &item), Some(42));
        item.section = 1;
        assert_eq!(estimated_position(Path::new("/a.txt"), &item), None);
    }

    #[test]
    fn bookmark_refresh_discards_old_results_and_distinguishes_errors() {
        let mut library = Library::default();
        let _ = library.update(Message::RefreshBookmarks);
        let first = library.bookmark_revision;
        let _ = library.update(Message::RefreshBookmarks);
        let second = library.bookmark_revision;
        let _ = library.update(Message::BookmarksLoaded(first, Ok(vec![])));
        assert!(library.bookmarks.is_none());
        let _ = library.update(Message::BookmarksLoaded(second, Err("rusak".into())));
        assert!(library.bookmarks.is_none());
        assert_eq!(library.bookmark_error.as_deref(), Some("rusak"));
        let _ = library.update(Message::RefreshBookmarks);
        let third = library.bookmark_revision;
        assert!(library.bookmark_error.is_none());
        let _ = library.update(Message::BookmarksLoaded(third, Ok(vec![])));
        assert!(library.bookmarks.as_ref().unwrap().is_empty());
        assert!(library.bookmark_error.is_none());
    }

    #[test]
    fn returning_to_library_and_successful_save_request_fresh_bookmarks() {
        let mut app = crate::NovelCraft::default();
        let _ = app.update(crate::Message::Navigate(crate::Page::Reader));
        let _ = app.update(crate::Message::Navigate(crate::Page::Library));
        let requested_on_navigation = app.library.bookmark_revision;
        assert_ne!(requested_on_navigation, 0);
        let _ = app.update(crate::Message::Reader(crate::reader::Message::BookmarkSaved(Ok(true))));
        assert_ne!(app.library.bookmark_revision, requested_on_navigation);
        let latest = app.library.bookmark_revision;
        let _ = app.update(crate::Message::Library(Message::BookmarksLoaded(requested_on_navigation, Ok(vec![]))));
        assert!(app.library.bookmarks.is_none());
        let _ = app.update(crate::Message::Library(Message::BookmarksLoaded(latest, Ok(vec![]))));
        assert!(app.library.bookmarks.is_some());
        let _ = app.update(crate::Message::Reader(crate::reader::Message::BookmarkSaved(Err("gagal".into()))));
        assert_eq!(app.library.bookmark_revision, latest);
        let _ = app.update(crate::Message::Reader(crate::reader::Message::BookmarkRemoved(Err("gagal".into()))));
        assert_eq!(app.library.bookmark_revision, latest);
        let _ = app.update(crate::Message::Reader(crate::reader::Message::BookmarkRemoved(Ok(true))));
        assert_ne!(app.library.bookmark_revision, latest);
        assert!(app.library.bookmarks.is_none());
    }

    #[test]
    fn card_view_supports_search_and_busy_state_without_changing_library() {
        let mut library = Library {
            books: vec![PathBuf::from("/books/Novel.epub"), PathBuf::from("/books/notes.md")],
            loaded: true,
            ..Library::default()
        };
        let _ = library.update(Message::Search("novel".into()));
        drop(library.view());
        library.bookmarks = Some(vec![Bookmark {
            path: "/books/Novel.epub".into(), section: 0, section_count: Some(1),
            scroll_percent: Some(20), markdown_source: false,
        }]);
        drop(library.view());
        library.busy = true;
        drop(library.view());
        assert_eq!(library.books.len(), 2);
        assert_eq!(library.query, "novel");
    }

    #[test]
    fn failed_load_keeps_library_locked() {
        let mut library = Library::default();
        let _ = library.update(Message::Loaded(Err("rusak".into())));
        assert!(!library.loaded);
        let _ = library.update(Message::Import);
        assert!(!library.busy);
        assert_eq!(library.error.as_deref(), Some("rusak"));
    }

    #[test]
    fn duplicate_import_does_not_save_or_duplicate() {
        let path = PathBuf::from("/novel.txt");
        let mut library = Library { books: vec![path.clone()], loaded: true, busy: true, ..Library::default() };
        let _ = library.update(Message::Imported(Ok(Some(path))));
        assert_eq!(library.books.len(), 1);
        assert!(!library.busy);
        assert!(library.error.is_some());
    }

    #[test]
    fn removing_entry_requires_confirmation_and_cancel_preserves_state() {
        let path = PathBuf::from("/books/novel.epub");
        let other = PathBuf::from("/books/other.txt");
        let bookmark = Bookmark { path: path.clone(), section: 1, section_count: Some(2), scroll_percent: Some(40), markdown_source: false };
        let mut library = Library { books: vec![path.clone(), other.clone()], loaded: true,
            snapshot: Some(b"original".to_vec()), bookmarks: Some(vec![bookmark.clone()]), ..Library::default() };
        let _ = library.update(Message::Remove(PathBuf::from("/missing")));
        assert!(library.pending_remove.is_none());
        let _ = library.update(Message::ConfirmRemove);
        assert!(!library.busy);
        let _ = library.update(Message::Remove(path.clone()));
        assert_eq!(library.pending_remove.as_ref(), Some(&path));
        assert!(!library.busy);
        assert!(!library.can_open_book());
        drop(library.view());
        let _ = library.update(Message::Remove(other.clone()));
        let _ = library.update(Message::Relink(other.clone()));
        let _ = library.update(Message::Import);
        let _ = library.update(Message::Reload);
        let _ = library.update(Message::Search("ignored".into()));
        assert_eq!(library.pending_remove.as_ref(), Some(&path));
        assert!(library.query.is_empty());
        assert!(!library.busy);
        assert_eq!(library.books, vec![path.clone(), other.clone()]);
        let _ = library.update(Message::CancelRemove);
        assert!(library.pending_remove.is_none());
        assert!(library.can_open_book());
        assert_eq!(library.books, vec![path.clone(), other.clone()]);
        assert_eq!(library.snapshot.as_deref(), Some(b"original".as_slice()));
        assert_eq!(library.bookmarks.as_ref().unwrap(), &vec![bookmark.clone()]);
        let _ = library.update(Message::Remove(path.clone()));
        let _ = library.update(Message::ConfirmRemove);
        assert!(library.busy);
        assert!(library.pending_remove.is_none());
        assert_eq!(library.books, vec![path.clone(), other.clone()]);
        let _ = library.update(Message::Saved(Err("konflik".into())));
        assert!(!library.busy);
        assert_eq!(library.books, vec![path.clone(), other.clone()]);
        assert_eq!(library.snapshot.as_deref(), Some(b"original".as_slice()));
        assert_eq!(library.bookmarks.as_ref().unwrap(), &vec![bookmark.clone()]);
        let _ = library.update(Message::Remove(path));
        let _ = library.update(Message::ConfirmRemove);
        let _ = library.update(Message::Saved(Ok((vec![other.clone()], b"baru".to_vec()))));
        assert_eq!(library.books, vec![other]);
        assert_eq!(library.snapshot.as_deref(), Some(b"baru".as_slice()));
        assert_eq!(library.bookmarks.as_ref().unwrap(), &vec![bookmark]);
    }

    #[test]
    fn remove_confirmation_blocks_open_and_continue_messages() {
        let mut app = crate::NovelCraft::default();
        let path = PathBuf::from("/novel.txt");
        let _ = app.library.update(Message::Loaded(Ok((vec![path.clone()], None))));
        let _ = app.library.update(Message::Remove(path.clone()));
        let _ = app.update(crate::Message::Library(Message::Open(path.clone())));
        let _ = app.update(crate::Message::Library(Message::Continue(path)));
        assert_eq!(app.page, crate::Page::Library);
        assert!(!app.reader.is_busy());
        let _ = app.library.update(Message::CancelRemove);
        assert!(app.library.can_open_book());
    }

    #[test]
    fn removing_duplicate_paths_displays_confirmation_and_waits_for_save() {
        let path = PathBuf::from("/duplicate.txt");
        let mut library = Library { books: vec![path.clone(), path.clone()], loaded: true, ..Library::default() };
        let _ = library.update(Message::Remove(path.clone()));
        assert_eq!(library.pending_remove.as_ref(), Some(&path));
        assert_eq!(library.books.len(), 2);
        assert!(without_book(&library.books, &path).is_empty());
        drop(library.view());
        let _ = library.update(Message::ConfirmRemove);
        assert!(library.busy);
        assert_eq!(library.books.len(), 2);
        let _ = library.update(Message::Saved(Ok((vec![], b"[]".to_vec()))));
        assert!(library.books.is_empty());
    }

    #[tokio::test]
    async fn removing_entry_keeps_book_and_bookmark_and_detects_library_conflicts() {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("novelcraft-library-remove-{}-{unique}", std::process::id()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let storage = dir.join("library.json");
        let bookmarks = dir.join("bookmarks.json");
        let book = dir.join("novel.txt");
        let other = dir.join("other.txt");
        let saved_bookmark = Bookmark { path: book.clone(), section: 0, section_count: None, scroll_percent: Some(35), markdown_source: false };
        let bookmark_bytes = serde_json::to_vec(&vec![saved_bookmark.clone()]).unwrap();
        tokio::fs::write(&book, b"isi buku").await.unwrap();
        tokio::fs::write(&bookmarks, &bookmark_bytes).await.unwrap();
        let initial = vec![book.clone(), other.clone()];
        let snapshot = save_to(&storage, &initial, None).await.unwrap();
        let mut library = Library { books: initial.clone(), snapshot: Some(snapshot.clone()), loaded: true,
            bookmarks: Some(vec![saved_bookmark]), ..Library::default() };
        let _ = library.update(Message::Remove(book.clone()));
        let _ = library.update(Message::CancelRemove);
        assert_eq!(tokio::fs::read(&storage).await.unwrap(), snapshot);
        assert_eq!(tokio::fs::read(&book).await.unwrap(), b"isi buku".to_vec());
        assert_eq!(tokio::fs::read(&bookmarks).await.unwrap(), bookmark_bytes);
        let _ = library.update(Message::Remove(book.clone()));
        let _ = library.update(Message::ConfirmRemove);
        let external = b"[\"/external.txt\"]";
        tokio::fs::write(&storage, external).await.unwrap();
        let remaining = without_book(&library.books, &book);
        assert_eq!(remaining, vec![other.clone()]);
        let result = save_to(&storage, &remaining, Some(snapshot.clone())).await.map(|bytes| (remaining.clone(), bytes));
        let _ = library.update(Message::Saved(result));
        assert_eq!(library.books, initial);
        assert_eq!(library.snapshot.as_deref(), Some(snapshot.as_slice()));
        assert_eq!(tokio::fs::read(&storage).await.unwrap(), external.to_vec());
        assert!(library.error.is_some());
        // After reloading the matching snapshot, a confirmed removal can succeed.
        let snapshot = save_to(&storage, &initial, Some(external.to_vec())).await.unwrap();
        let _ = library.update(Message::Loaded(Ok((initial, Some(snapshot.clone())))));
        let _ = library.update(Message::Remove(book.clone()));
        let _ = library.update(Message::ConfirmRemove);
        let result = save_to(&storage, &remaining, Some(snapshot)).await.map(|bytes| (remaining.clone(), bytes));
        let _ = library.update(Message::Saved(result));
        assert_eq!(library.books, vec![other]);
        assert_eq!(load_from(&storage).await.unwrap().0, remaining);
        assert_eq!(tokio::fs::read(&book).await.unwrap(), b"isi buku".to_vec());
        assert_eq!(tokio::fs::read(&bookmarks).await.unwrap(), bookmark_bytes);
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[test]
    fn relink_requires_confirmation_and_cancellation_preserves_state() {
        let old = PathBuf::from("/books/old.epub");
        let new = PathBuf::from("/books/new.epub");
        let bookmark = Bookmark { path: old.clone(), section: 1, section_count: Some(2), scroll_percent: Some(40), markdown_source: false };
        let mut library = Library { books: vec![old.clone()], loaded: true, snapshot: Some(b"original".to_vec()),
            bookmarks: Some(vec![bookmark.clone()]), busy: true, ..Library::default() };
        let _ = library.update(Message::Relinked(old.clone(), Ok(Some(new.clone()))));
        assert_eq!(library.pending_relink, Some((old.clone(), new.clone())));
        assert!(!library.busy);
        assert!(!library.can_open_book());
        drop(library.view());
        let _ = library.update(Message::Remove(old.clone()));
        let _ = library.update(Message::Reload);
        let _ = library.update(Message::Import);
        let _ = library.update(Message::Search("ignored".into()));
        assert!(!library.busy);
        assert!(library.query.is_empty());
        assert_eq!(library.books, vec![old.clone()]);
        let _ = library.update(Message::CancelRelink);
        assert!(library.can_open_book());
        assert!(library.pending_relink.is_none());
        assert_eq!(library.books, vec![old.clone()]);
        assert_eq!(library.snapshot.as_deref(), Some(b"original".as_slice()));
        assert_eq!(library.bookmarks.as_ref().unwrap(), &vec![bookmark]);
        // Confirmation starts a write, but does not update the visible list until success.
        let _ = library.update(Message::Relinked(old.clone(), Ok(Some(new.clone()))));
        let _ = library.update(Message::ConfirmRelink);
        assert!(library.busy);
        assert_eq!(library.books, vec![old.clone()]);
        let _ = library.update(Message::Saved(Err("konflik".into())));
        assert!(!library.busy);
        assert_eq!(library.books, vec![old.clone()]);
        assert_eq!(library.snapshot.as_deref(), Some(b"original".as_slice()));
        let _ = library.update(Message::Relinked(old, Ok(Some(new.clone()))));
        let _ = library.update(Message::ConfirmRelink);
        let _ = library.update(Message::Saved(Ok((vec![new.clone()], b"baru".to_vec()))));
        assert_eq!(library.books, vec![new]);
        assert_eq!(library.snapshot.as_deref(), Some(b"baru".as_slice()));
        assert_eq!(library.bookmarks.as_ref().unwrap()[0].path, PathBuf::from("/books/old.epub"));
    }

    #[test]
    fn relink_rejects_duplicate_same_and_missing_entries() {
        let old = PathBuf::from("/old.txt");
        let new = PathBuf::from("/new.txt");
        let books = vec![old.clone(), new.clone()];
        assert!(replace_book(&books, &old, &new).is_err());
        assert!(replace_book(&books, &old, &old).is_err());
        assert!(replace_book(&books, Path::new("/missing"), Path::new("/next")).is_err());
        assert!(replace_book(&[old.clone(), old.clone()], &old, Path::new("/next")).is_err());
        assert_eq!(replace_book(&books, &old, Path::new("/next")).unwrap(), vec![PathBuf::from("/next"), new.clone()]);
        let mut library = Library { books, loaded: true, busy: true, ..Library::default() };
        let _ = library.update(Message::Relinked(old.clone(), Ok(Some(new))));
        assert!(library.pending_relink.is_none());
        assert!(library.error.as_deref().unwrap().contains("sudah ada"));
        let _ = library.update(Message::Relinked(old.clone(), Ok(None)));
        assert!(library.pending_relink.is_none());
        let _ = library.update(Message::Relinked(old, Err("pilih gagal".into())));
        assert_eq!(library.error.as_deref(), Some("pilih gagal"));
        assert_eq!(library.books.len(), 2);
    }

    #[test]
    fn relink_confirmation_blocks_open_and_continue_messages() {
        let mut app = crate::NovelCraft::default();
        let old = PathBuf::from("/old.txt");
        let new = PathBuf::from("/new.txt");
        let _ = app.library.update(Message::Loaded(Ok((vec![old.clone()], None))));
        let _ = app.library.update(Message::Relinked(old.clone(), Ok(Some(new))));
        let _ = app.update(crate::Message::Library(Message::Open(old.clone())));
        let _ = app.update(crate::Message::Library(Message::Continue(old)));
        assert_eq!(app.page, crate::Page::Library);
        assert!(!app.reader.is_busy());
    }

    #[tokio::test]
    async fn relink_checks_target_and_conflict_without_changing_files_or_bookmarks() {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("novelcraft-library-relink-{}-{unique}", std::process::id()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let storage = dir.join("library.json");
        let bookmarks = dir.join("bookmarks.json");
        let old = dir.join("old.txt");
        let new = dir.join("new.txt");
        tokio::fs::write(&old, b"old contents").await.unwrap();
        tokio::fs::write(&new, b"new contents").await.unwrap();
        tokio::fs::write(&bookmarks, b"existing bookmark data").await.unwrap();
        let initial = vec![old.clone()];
        let snapshot = save_to(&storage, &initial, None).await.unwrap();
        let updated = replace_book(&initial, &old, &new).unwrap();
        assert!(validate_relink_target(&dir).await.is_err());
        assert!(validate_relink_target(&dir.join("missing.txt")).await.is_err());
        tokio::fs::write(&storage, b"[\"/external.txt\"]").await.unwrap();
        assert!(save_relinked_to(&storage, updated.clone(), Some(snapshot), &new).await.is_err());
        assert_eq!(tokio::fs::read(&storage).await.unwrap(), b"[\"/external.txt\"]".to_vec());
        let current = tokio::fs::read(&storage).await.unwrap();
        tokio::fs::remove_file(&new).await.unwrap();
        assert!(save_relinked_to(&storage, updated.clone(), Some(current.clone()), &new).await.is_err());
        assert_eq!(tokio::fs::read(&storage).await.unwrap(), current);
        tokio::fs::write(&new, b"new contents").await.unwrap();
        let (saved, _) = save_relinked_to(&storage, updated.clone(), Some(current), &new).await.unwrap();
        assert_eq!(saved, updated);
        assert_eq!(load_from(&storage).await.unwrap().0, updated);
        assert_eq!(tokio::fs::read(&old).await.unwrap(), b"old contents".to_vec());
        assert_eq!(tokio::fs::read(&new).await.unwrap(), b"new contents".to_vec());
        assert_eq!(tokio::fs::read(&bookmarks).await.unwrap(), b"existing bookmark data".to_vec());
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[test]
    fn failed_save_keeps_original_list() {
        let mut library = Library { books: vec![PathBuf::from("/old.txt")], loaded: true, busy: true, ..Library::default() };
        let _ = library.update(Message::Saved(Err("gagal".into())));
        assert_eq!(library.books, vec![PathBuf::from("/old.txt")]);
        assert!(!library.busy);
    }

    #[test]
    fn reload_keeps_current_library_on_error_and_replaces_it_on_success() {
        let mut library = Library { books: vec![PathBuf::from("/old.txt")], loaded: true,
            snapshot: Some(b"old".to_vec()), ..Library::default() };
        let _ = library.update(Message::Reload);
        assert!(library.busy);
        let _ = library.update(Message::Loaded(Err("gagal".into())));
        assert_eq!(library.books, vec![PathBuf::from("/old.txt")]);
        assert_eq!(library.snapshot.as_deref(), Some(b"old".as_slice()));
        let _ = library.update(Message::Reload);
        let _ = library.update(Message::Loaded(Ok((vec![PathBuf::from("/new.txt")], Some(b"new".to_vec())))));
        assert_eq!(library.books, vec![PathBuf::from("/new.txt")]);
        assert_eq!(library.snapshot.as_deref(), Some(b"new".as_slice()));
        assert!(!library.busy);
        assert!(library.error.is_none());
    }

    #[test]
    fn successful_save_updates_library_and_expected_snapshot() {
        let mut library = Library { books: vec![PathBuf::from("/old.txt")], loaded: true,
            snapshot: Some(b"old".to_vec()), busy: true, ..Library::default() };
        let _ = library.update(Message::Saved(Ok((vec![PathBuf::from("/new.txt")], b"new".to_vec()))));
        assert_eq!(library.books, vec![PathBuf::from("/new.txt")]);
        assert_eq!(library.snapshot.as_deref(), Some(b"new".as_slice()));
        assert!(!library.busy);
        assert!(library.error.is_none());
    }

    #[tokio::test]
    async fn external_changes_are_not_overwritten_and_can_be_reloaded() {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("novelcraft-library-conflict-{}-{unique}", std::process::id()));
        let path = dir.join("library.json");
        let initial = vec![PathBuf::from("/first.txt")];
        save_to(&path, &initial, None).await.unwrap();
        let (_, snapshot) = load_from(&path).await.unwrap();
        let external = br#"["/external.txt"]"#;
        tokio::fs::write(&path, external).await.unwrap();
        assert!(save_to(&path, &[PathBuf::from("/other.txt")], snapshot).await.is_err());
        assert_eq!(tokio::fs::read(&path).await.unwrap(), external.to_vec());
        let (reloaded, current) = load_from(&path).await.unwrap();
        assert_eq!(reloaded, vec![PathBuf::from("/external.txt")]);
        let replacement = vec![PathBuf::from("/new.txt")];
        save_to(&path, &replacement, current).await.unwrap();
        assert_eq!(load_from(&path).await.unwrap().0, replacement);
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn corrupt_external_change_is_not_overwritten() {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("novelcraft-library-corrupt-{}-{unique}", std::process::id()));
        let path = dir.join("library.json");
        save_to(&path, &[PathBuf::from("/old.txt")], None).await.unwrap();
        let (_, snapshot) = load_from(&path).await.unwrap();
        tokio::fs::write(&path, b"not json").await.unwrap();
        assert!(save_to(&path, &[PathBuf::from("/new.txt")], snapshot).await.is_err());
        assert!(load_from(&path).await.is_err());
        assert_eq!(tokio::fs::read(&path).await.unwrap(), b"not json".to_vec());
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn detects_file_created_or_removed_since_load() {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("novelcraft-library-change-{}-{unique}", std::process::id()));
        let path = dir.join("library.json");
        let (_, absent) = load_from(&path).await.unwrap();
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(&path, b"[]").await.unwrap();
        assert!(save_to(&path, &[PathBuf::from("/new.txt")], absent).await.is_err());
        assert_eq!(tokio::fs::read(&path).await.unwrap(), b"[]".to_vec());
        let (_, present) = load_from(&path).await.unwrap();
        tokio::fs::remove_file(&path).await.unwrap();
        assert!(save_to(&path, &[PathBuf::from("/new.txt")], present).await.is_err());
        assert!(!path.exists());
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn saved_library_uses_private_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("novelcraft-library-mode-{}-{unique}", std::process::id()));
        let path = dir.join("library.json");
        save_to(&path, &[PathBuf::from("/book.txt")], None).await.unwrap();
        assert_eq!(tokio::fs::metadata(&path).await.unwrap().permissions().mode() & 0o077, 0);
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlink_is_not_followed_or_replaced() {
        use std::os::unix::fs::symlink;
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("novelcraft-library-link-{}-{unique}", std::process::id()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("library.json");
        let target = dir.join("target.json");
        tokio::fs::write(&target, b"[]").await.unwrap();
        symlink(&target, &path).unwrap();
        assert!(load_from(&path).await.is_err());
        assert!(save_to(&path, &[PathBuf::from("/book.txt")], None).await.is_err());
        assert!(tokio::fs::symlink_metadata(&path).await.unwrap().file_type().is_symlink());
        assert_eq!(tokio::fs::read(&target).await.unwrap(), b"[]".to_vec());
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn failed_replace_keeps_existing_directory_and_cleans_temp_file() {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("novelcraft-library-error-{}-{unique}", std::process::id()));
        let path = dir.join("library.json");
        tokio::fs::create_dir_all(&path).await.unwrap();
        assert!(save_to(&path, &[PathBuf::from("/book.txt")], None).await.is_err());
        assert!(tokio::fs::metadata(&path).await.unwrap().is_dir());
        let mut entries = tokio::fs::read_dir(&dir).await.unwrap();
        assert_eq!(entries.next_entry().await.unwrap().unwrap().file_name(), "library.json");
        assert!(entries.next_entry().await.unwrap().is_none());
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn saves_and_loads_list_and_rejects_corrupt_data() {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("novelcraft-library-{}-{unique}", std::process::id()));
        let path = dir.join("library.json");
        assert!(load_from(&path).await.unwrap().0.is_empty());
        let books = vec![PathBuf::from("/novel.md"), PathBuf::from("/buku.epub")];
        save_to(&path, &books, None).await.unwrap();
        assert_eq!(load_from(&path).await.unwrap().0, books);
        tokio::fs::write(&path, b"not json").await.unwrap();
        assert!(load_from(&path).await.is_err());
        assert_eq!(tokio::fs::read(&path).await.unwrap(), b"not json".to_vec());
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }
}
