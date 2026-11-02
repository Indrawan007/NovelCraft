use crate::bookmarks::{self, Bookmark};
use iced::widget::{button, column, container, row, scrollable, text, text_input, Column};
use iced::{Element, Length, Task};
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
    error: Option<String>,
    bookmarks: Option<Vec<Bookmark>>,
    bookmark_error: Option<String>,
    bookmark_revision: u64,
}

#[derive(Debug, Clone)]
pub enum Message {
    Load,
    Reload,
    Loaded(Result<(Vec<PathBuf>, Option<Vec<u8>>), String>),
    RefreshBookmarks,
    BookmarksLoaded(u64, Result<Vec<Bookmark>, String>),
    Search(String),
    Import,
    Imported(Result<Option<PathBuf>, String>),
    Remove(PathBuf),
    Saved(Result<(Vec<PathBuf>, Vec<u8>), String>),
    Open(PathBuf),
    Continue(PathBuf),
}

impl Library {
    pub fn is_busy(&self) -> bool {
        self.busy
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Load if !self.busy && !self.loaded => {
                self.busy = true;
                self.error = None;
                return Task::perform(load(), Message::Loaded);
            }
            Message::Reload if !self.busy && self.loaded => {
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
            Message::Search(query) => self.query = query,
            Message::Import if self.loaded && !self.busy => {
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
            Message::Remove(path) if self.loaded && !self.busy => {
                let books = self.books.iter().filter(|book| *book != &path).cloned().collect();
                return self.save(books);
            }
            Message::Saved(result) => {
                self.busy = false;
                match result {
                    Ok((books, snapshot)) => {
                        self.books = books;
                        self.snapshot = Some(snapshot);
                        self.error = None;
                    }
                    Err(error) => self.error = Some(error),
                }
            }
            _ => {}
        }
        Task::none()
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
        if self.loaded && !self.busy {
            import = import.on_press(Message::Import);
            reload = reload.on_press(Message::Reload);
        }
        let mut view = column![
            text("Library").size(32),
            text("File tetap berada di lokasi aslinya; Library hanya menyimpan daftar lokasi.").size(14),
            row![
                text_input("Cari nama file...", &self.query)
                    .on_input(Message::Search)
                    .width(Length::Fill),
                import,
                reload,
            ].spacing(12),
        ]
        .spacing(12)
        .height(Length::Fill);

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
            if self.loaded && !self.busy {
                open = open.on_press(Message::Open(path.clone()));
                remove = remove.on_press(Message::Remove(path.clone()));
            }
            let mut card = column![
                text(name.to_string()).size(18),
                text(path.display().to_string()).size(12),
            ]
            .width(Length::Fill)
            .spacing(8);
            let mut actions = row![open].spacing(12);
            if let Some(bookmarks) = &self.bookmarks {
                let bookmark = bookmarks.iter().find(|item| item.path == *path);
                card = card.push(text(bookmark_label(path, bookmark)).size(14));
                if let Some(bookmark) = bookmark {
                    let mut continue_reading = button("Lanjut baca");
                    if self.loaded && !self.busy {
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
            card = card.push(actions.push(remove));
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
