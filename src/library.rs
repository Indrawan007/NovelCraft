use crate::bookmarks::{self, Bookmark};
use iced::widget::{button, column, container, progress_bar, row, scrollable, text, text_input, Row, Space};
use iced::{Element, Length, Task, Theme};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use rusqlite::{params, Connection, OpenFlags, TransactionBehavior};
use std::sync::Mutex;
use std::time::Duration;

#[derive(Default)]
pub struct Library {
    books: Vec<PathBuf>,
    query: String,
    sort: LibrarySort,
    missing_only: bool,
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

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
enum LibrarySort {
    #[default]
    Added,
    NameAscending,
    NameDescending,
}

impl LibrarySort {
    fn next(self) -> Self {
        match self {
            Self::Added => Self::NameAscending,
            Self::NameAscending => Self::NameDescending,
            Self::NameDescending => Self::Added,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Added => "Urutan: daftar",
            Self::NameAscending => "Urutan: nama A–Z",
            Self::NameDescending => "Urutan: nama Z–A",
        }
    }
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
    CycleSort,
    ToggleMissingOnly,
    Import,
    NewDraft,
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
            Message::CycleSort if self.loaded && !self.controls_blocked() => self.sort = self.sort.next(),
            Message::ToggleMissingOnly if self.loaded && !self.controls_blocked() => self.missing_only = !self.missing_only,
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

    fn visible_books(&self) -> Vec<&PathBuf> {
        let query = self.query.to_lowercase();
        let mut books: Vec<_> = self.books.iter().filter(|path| {
            matches_name(&book_name(path), &query)
                && (!self.missing_only || self.availability.get(*path) == Some(&Availability::Missing))
        }).collect();
        match self.sort {
            LibrarySort::Added => {}
            LibrarySort::NameAscending => books.sort_by_cached_key(|path| book_name(path).to_lowercase()),
            LibrarySort::NameDescending => books.sort_by_cached_key(|path| std::cmp::Reverse(book_name(path).to_lowercase())),
        }
        books
    }

    fn save(&mut self, books: Vec<PathBuf>) -> Task<Message> {
        self.busy = true;
        self.error = None;
        Task::perform(save(books, self.snapshot.clone()), Message::Saved)
    }

    pub fn set_error(&mut self, error: &str) {
        self.error = Some(error.to_owned());
    }

    pub fn view<'a>(&'a self, theme: &Theme) -> Element<'a, Message> {
        let mut import = button(text("Import").size(13)).padding([9, 16]).style(crate::mockup_style::secondary);
        let mut new_draft = button(text("Draft baru").size(13)).padding([9, 16]).style(crate::mockup_style::primary);
        let mut reload = button(text("Muat ulang Library").size(12)).style(button::secondary);
        if !self.controls_blocked() {
            new_draft = new_draft.on_press(Message::NewDraft);
        }
        if !self.controls_blocked() {
            reload = reload.on_press(if self.loaded { Message::Reload } else { Message::Load });
            if self.loaded { import = import.on_press(Message::Import); }
        }
        let mut search = text_input("Cari nama file...", &self.query).size(13).padding([9, 14]).width(Length::Fill).style(crate::mockup_style::search);
        if !self.controls_blocked() { search = search.on_input(Message::Search); }
        let mut view = column![
            text("Library").size(32),
            text(if self.loaded { format!("{} file · tersimpan di lokasi aslinya", self.books.len()) } else { "Memuat Library lokal".to_owned() })
                .size(14).color(crate::mockup_style::muted(theme)),
            row![container(search).width(Length::Fill).max_width(360), import, new_draft].align_y(iced::Alignment::Center).spacing(10),
        ]
        .spacing(12)
        .height(Length::Fill);

        let mut tools = Row::new().spacing(10).push(reload);
        if self.loaded && !self.books.is_empty() {
            let mut sort = button(text(self.sort.label()).size(12)).style(button::secondary);
            let mut filter = button(text(if self.missing_only { "Tampilkan semua lokasi" } else { "Hanya tidak ditemukan" }).size(12)).style(button::secondary);
            let mut refresh = button(text("Periksa ulang lokasi").size(12)).style(button::secondary);
            if !self.controls_blocked() {
                sort = sort.on_press(Message::CycleSort);
                filter = filter.on_press(Message::ToggleMissingOnly);
                if self.availability_active.is_none() {
                    refresh = refresh.on_press(Message::RefreshAvailability);
                }
            }
            tools = tools.push(sort).push(filter).push(refresh);
            if self.missing_only {
                view = view.push(text("Filter berdasarkan pemeriksaan terakhir; lokasi yang belum dapat diperiksa tidak ditampilkan.").size(12));
            }
        }
        view = view.push(tools.padding(iced::Padding { bottom: 10.0, ..iced::Padding::default() }).wrap());
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

        let mut list = Row::new().spacing(16).width(Length::Fill);
        let visible = self.visible_books();
        let matches = visible.len();
        // Build once per view rather than scanning every bookmark for each card.
        let bookmarks_by_path = self.bookmarks.as_ref().map(|items| bookmark_index(items));
        for path in visible {
            let name = book_name(path);
            let mut open = button(text("Baca").size(12)).style(button::secondary);
            let mut remove = button(text("Hapus dari daftar").size(12)).style(button::secondary);
            let mut relink = button(text("Ubah lokasi").size(12)).style(button::secondary);
            if self.loaded && !self.controls_blocked() {
                open = open.on_press(Message::Open(path.clone()));
                remove = remove.on_press(Message::Remove(path.clone()));
                relink = relink.on_press(Message::Relink(path.clone()));
            }
            let mut card = column![
                text(name.to_string()).size(16),
                text(path.display().to_string()).size(12).color(crate::mockup_style::muted(theme)),
                text(availability_label(self.availability.get(path), self.availability_active.is_some())).size(12).color(crate::mockup_style::subtle(theme)),
            ]
            .width(Length::Fill)
            .spacing(8);
            let mut actions = row![open].spacing(12);
            if let Some(bookmarks) = &bookmarks_by_path {
                let bookmark = bookmarks.get(path.as_path()).copied();
                card = card.push(text(bookmark_label(path, bookmark)).size(14));
                if let Some(bookmark) = bookmark {
                    let mut continue_reading = button(text("Lanjut baca").size(12)).style(button::secondary);
                    if self.loaded && !self.controls_blocked() {
                        continue_reading = continue_reading.on_press(Message::Continue(path.clone()));
                    }
                    actions = actions.push(continue_reading);
                    let percent = estimated_position(path, bookmark);
                    let estimate = match percent {
                        Some(percent) => format!("Perkiraan posisi keseluruhan: {percent}%"),
                        None => "Perkiraan posisi keseluruhan tidak tersedia".to_owned(),
                    };
                    card = card.push(text(estimate).size(12).color(crate::mockup_style::muted(theme)));
                    if let Some(percent) = percent {
                        card = card.push(progress_bar(0.0..=100.0, f32::from(percent)).height(3));
                    }
                }
            }
            card = card.push(actions.push(remove).push(relink).padding(iced::Padding { bottom: 10.0, ..iced::Padding::default() }).wrap());
            let spine = if self.availability.get(path) == Some(&Availability::Missing) {
                theme.palette().danger
            } else {
                theme.palette().primary
            };
            let spine = container(Space::new(4, 120)).style(move |_| container::Style::default().background(spine));
            list = list.push(
                container(row![spine, card].spacing(14))
                    .width(320)
                    .padding(20)
                    .style(crate::mockup_style::card),
            );
        }
        if self.loaded && !self.books.is_empty() && matches == 0 {
            let notice = if self.missing_only && self.availability_active.is_some() {
                "Menunggu pemeriksaan lokasi selesai."
            } else if self.missing_only {
                "Tidak ada file yang cocok dengan pencarian dan filter lokasi."
            } else {
                "Tidak ada file yang cocok."
            };
            view = view.push(text(notice));
        }
        view.push(scrollable(list.padding(iced::Padding { bottom: 16.0, ..iced::Padding::default() }).wrap()).height(Length::Fill)).into()
    }
}

// Match the previous linear lookup: when legacy data has duplicates, use the first.
fn bookmark_index(bookmarks: &[Bookmark]) -> HashMap<&Path, &Bookmark> {
    let mut index = HashMap::with_capacity(bookmarks.len());
    for bookmark in bookmarks { index.entry(bookmark.path.as_path()).or_insert(bookmark); }
    index
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

fn book_name(path: &Path) -> std::borrow::Cow<'_, str> {
    path.file_name().unwrap_or(path.as_os_str()).to_string_lossy()
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

const LIBRARY_DB: &str = "library.sqlite3";
const LIBRARY_APP_ID: i64 = 1313030732;

fn legacy_snapshot(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() =>
            Err("Library JSON harus berupa file biasa, bukan symbolic link.".into()),
        Ok(_) => std::fs::read(path).map(Some).map_err(|e| format!("Gagal membaca Library JSON: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("Gagal membaca Library JSON: {e}")),
    }
}

// Snapshots combine database identity, revision and contents. The revision detects
// changes made through the app even if the list is later changed back.
fn database_operation(path: &Path, replacement: Option<(&[PathBuf], &[u8])>) -> Result<(Vec<PathBuf>, Vec<u8>), String> {
    static INITIALIZATION: Mutex<()> = Mutex::new(());
    let _guard = INITIALIZATION.lock().map_err(|_| "Proses database Library gagal; mulai ulang aplikasi.")?;
    let replacement_data = replacement.map(|(books, _)| serde_json::to_string(books)
        .map_err(|e| format!("Gagal menyiapkan data Library: {e}"))).transpose()?;
    let dir = path.parent().ok_or("Lokasi Library tidak valid.")?;
    let original = legacy_snapshot(path)?;
    if let Some(bytes) = &original {
        serde_json::from_slice::<Vec<PathBuf>>(bytes).map_err(|e| format!("Data Library tidak valid; file asli tidak diubah: {e}"))?;
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("Gagal membuat folder Library: {e}"))?;
    let db_path = dir.join(LIBRARY_DB);
    let created = if replacement.is_some() {
        // A stale save must never recreate a database that disappeared after load.
        false
    } else {
        let mut options = std::fs::OpenOptions::new(); options.write(true).create_new(true);
        #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
        match options.open(&db_path) {
            Ok(file) => { drop(file); true },
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
            Err(e) => return Err(format!("Gagal membuat database Library: {e}")),
        }
    };
    let meta = std::fs::symlink_metadata(&db_path).map_err(|e| format!("Gagal memeriksa database Library: {e}"))?;
    if !meta.is_file() || meta.file_type().is_symlink() { return Err("Database Library harus berupa file biasa, bukan symbolic link.".into()); }
    let mut db = Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|e| format!("Gagal membuka database Library: {e}"))?;
    db.busy_timeout(Duration::from_secs(3)).map_err(|e| e.to_string())?;
    if !created {
        let app: i64 = db.query_row("PRAGMA application_id", [], |r| r.get(0)).map_err(|e| format!("Database Library tidak valid: {e}"))?;
        let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(|e| format!("Database Library tidak valid: {e}"))?;
        if app != LIBRARY_APP_ID || version != 1 { return Err("Database Library bukan milik NovelCraft atau versinya tidak didukung; file tidak diubah.".into()); }
    }
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e| format!("Gagal membuka transaksi Library: {e}"))?;
    if created {
        tx.execute_batch(&format!("PRAGMA application_id={LIBRARY_APP_ID}; PRAGMA user_version=1; CREATE TABLE library (id INTEGER PRIMARY KEY CHECK(id=1), identity BLOB NOT NULL, revision INTEGER NOT NULL, data TEXT NOT NULL, legacy BLOB);"))
            .map_err(|e| format!("Gagal menyiapkan database Library: {e}"))?;
        let data = std::str::from_utf8(original.as_deref().unwrap_or(b"[]")).map_err(|e| format!("Data Library tidak valid: {e}"))?;
        tx.execute("INSERT INTO library(id,identity,revision,data,legacy) VALUES(1,randomblob(16),1,?1,?2)", params![data, original])
            .map_err(|e| format!("Gagal memigrasikan Library: {e}"))?;
    }
    let (identity, mut revision, data, expected_legacy): (Vec<u8>, i64, String, Option<Vec<u8>>) =
        tx.query_row("SELECT identity,revision,data,legacy FROM library WHERE id=1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .map_err(|e| format!("Gagal membaca database Library: {e}"))?;
    if identity.len() != 16 || revision < 1 { return Err("Identitas atau revisi Library tidak valid.".into()); }
    if legacy_snapshot(path)? != expected_legacy {
        return Err("Library JSON lama berubah setelah migrasi; perubahan tidak digabung otomatis. Pulihkan atau bandingkan cadangan sebelum melanjutkan.".into());
    }
    let mut books: Vec<PathBuf> = serde_json::from_str(&data).map_err(|e| format!("Data Library tidak valid: {e}"))?;
    let mut token = identity;
    token.extend_from_slice(&revision.to_be_bytes());
    token.extend_from_slice(data.as_bytes());
    if let Some((new_books, expected)) = replacement {
        if token != expected { return Err("Library berubah di luar aplikasi; muat ulang Library sebelum mencoba lagi.".into()); }
        revision = revision.checked_add(1).ok_or("Revisi Library habis.")?;
        let new_data = replacement_data.as_ref().ok_or("Data Library tidak tersedia.")?;
        tx.execute("UPDATE library SET data=?1,revision=?2 WHERE id=1", params![new_data, revision])
            .map_err(|e| format!("Gagal menyimpan Library: {e}"))?;
        books = new_books.to_vec();
        token.truncate(16); token.extend_from_slice(&revision.to_be_bytes()); token.extend_from_slice(new_data.as_bytes());
    }
    tx.commit().map_err(|e| format!("Gagal menyimpan Library: {e}"))?;
    Ok((books, token))
}

async fn load_from(path: &Path) -> Result<(Vec<PathBuf>, Option<Vec<u8>>), String> {
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || database_operation(&path, None)).await
        .map_err(|e| format!("Proses database Library gagal: {e}"))?
        .map(|(books, token)| (books, Some(token)))
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

async fn save_to(path: &Path, books: &[PathBuf], expected: Option<Vec<u8>>) -> Result<Vec<u8>, String> {
    let expected = expected.ok_or("Library belum dimuat; muat ulang sebelum menyimpan.")?;
    let path = path.to_owned();
    let books = books.to_vec();
    tokio::task::spawn_blocking(move || database_operation(&path, Some((&books, &expected)))).await
        .map_err(|e| format!("Proses database Library gagal: {e}"))?
        .map(|(_, token)| token)
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
        drop(library.view(&Theme::TokyoNight));
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
    fn library_sort_is_stable_and_does_not_change_stored_order() {
        let a1 = PathBuf::from("/first/novel.TXT");
        let z = PathBuf::from("/books/Zebra.epub");
        let a2 = PathBuf::from("/second/NOVEL.txt");
        let b = PathBuf::from("/books/Bulan.md");
        let original = vec![a1.clone(), z.clone(), a2.clone(), b.clone()];
        let mut library = Library { books: original.clone(), loaded: true, snapshot: Some(b"initial".to_vec()), ..Library::default() };
        assert_eq!(library.visible_books().into_iter().cloned().collect::<Vec<_>>(), original);
        let _ = library.update(Message::CycleSort);
        assert_eq!(library.sort, LibrarySort::NameAscending);
        assert_eq!(library.visible_books().into_iter().cloned().collect::<Vec<_>>(), vec![b.clone(), a1.clone(), a2.clone(), z.clone()]);
        let _ = library.update(Message::CycleSort);
        assert_eq!(library.visible_books().into_iter().cloned().collect::<Vec<_>>(), vec![z, a1.clone(), a2.clone(), b]);
        let _ = library.update(Message::Search("NoVeL".into()));
        assert_eq!(library.visible_books().into_iter().cloned().collect::<Vec<_>>(), vec![a1, a2]);
        let _ = library.update(Message::CycleSort);
        assert_eq!(library.sort, LibrarySort::Added);
        assert_eq!(library.books, original);
        assert_eq!(library.snapshot.as_deref(), Some(b"initial".as_slice()));
        assert!(!library.busy);
        drop(library.view(&Theme::TokyoNight));
    }

    #[test]
    fn missing_filter_combines_search_and_sort_without_treating_unknown_as_missing() {
        let a = PathBuf::from("/books/A.txt");
        let b = PathBuf::from("/books/B.txt");
        let c = PathBuf::from("/books/C.txt");
        let d = PathBuf::from("/books/D.txt");
        let e = PathBuf::from("/books/E.txt");
        let mut library = Library { books: vec![c.clone(), b.clone(), a.clone(), d.clone(), e.clone()], loaded: true,
            availability: HashMap::from([(a.clone(), Availability::Missing), (b.clone(), Availability::File),
                (c.clone(), Availability::Missing), (d.clone(), Availability::Unknown), (e.clone(), Availability::NotFile)]),
            ..Library::default() };
        let _ = library.update(Message::ToggleMissingOnly);
        assert_eq!(library.visible_books().into_iter().cloned().collect::<Vec<_>>(), vec![c.clone(), a.clone()]);
        let _ = library.update(Message::CycleSort);
        assert_eq!(library.visible_books().into_iter().cloned().collect::<Vec<_>>(), vec![a.clone(), c.clone()]);
        let _ = library.update(Message::Search("c.".into()));
        assert_eq!(library.visible_books().into_iter().cloned().collect::<Vec<_>>(), vec![c.clone()]);
        let _ = library.update(Message::RefreshAvailability);
        assert!(library.visible_books().is_empty());
        assert!(library.availability_active.is_some());
        drop(library.view(&Theme::TokyoNight));
        let revision = library.availability_active.unwrap();
        let _ = library.update(Message::AvailabilityLoaded(revision, HashMap::from([(a.clone(), Availability::Missing), (c, Availability::File)])));
        assert!(library.visible_books().is_empty());
        let _ = library.update(Message::Search(String::new()));
        assert_eq!(library.visible_books().into_iter().cloned().collect::<Vec<_>>(), vec![a]);
        let _ = library.update(Message::ToggleMissingOnly);
        assert_eq!(library.visible_books().len(), 5);
        assert!(!library.busy);
    }

    #[test]
    fn sort_and_filter_respect_busy_and_confirmation_guards() {
        let path = PathBuf::from("/book.txt");
        let mut library = Library { books: vec![path.clone()], loaded: true, ..Library::default() };
        let _ = library.update(Message::Remove(path));
        let _ = library.update(Message::CycleSort);
        let _ = library.update(Message::ToggleMissingOnly);
        assert_eq!(library.sort, LibrarySort::Added);
        assert!(!library.missing_only);
        let _ = library.update(Message::CancelRemove);
        let _ = library.update(Message::CycleSort);
        let _ = library.update(Message::ToggleMissingOnly);
        assert_eq!(library.sort, LibrarySort::NameAscending);
        assert!(library.missing_only);
        library.busy = true;
        let _ = library.update(Message::CycleSort);
        let _ = library.update(Message::ToggleMissingOnly);
        assert_eq!(library.sort, LibrarySort::NameAscending);
        assert!(library.missing_only);
    }


    #[test]
    fn search_matches_filename_without_case_sensitivity() {
        assert!(matches_name("Laskar Pelangi.EPUB", "pelangi"));
        assert!(!matches_name("Laskar Pelangi.EPUB", "bulan"));
        assert!(matches_name("Novel.txt", ""));
    }

    #[test]
    fn indexed_bookmark_lookup_matches_linear_lookup_including_duplicates() {
        let mut bookmarks: Vec<Bookmark> = (0..4000).map(|i| Bookmark {
            path: format!("/book-{i}.epub").into(), section: i, section_count: None, scroll_percent: Some(10), markdown_source: false,
        }).collect();
        bookmarks.push(Bookmark { path: "/book-25.epub".into(), section: 9999, section_count: None, scroll_percent: None, markdown_source: false });
        let index = bookmark_index(&bookmarks);
        for i in [0, 25, 3999, 4000] {
            let path = PathBuf::from(format!("/book-{i}.epub"));
            assert_eq!(index.get(path.as_path()).copied(), bookmarks.iter().find(|item| item.path == path));
        }
        assert_eq!(index.len(), 4000);
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
        drop(library.view(&Theme::TokyoNight));
        library.bookmarks = Some(vec![Bookmark {
            path: "/books/Novel.epub".into(), section: 0, section_count: Some(1),
            scroll_percent: Some(20), markdown_source: false,
        }]);
        drop(library.view(&Theme::TokyoNight));
        library.busy = true;
        drop(library.view(&Theme::TokyoNight));
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
        let _ = library.update(Message::Load);
        assert!(library.busy);
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
        drop(library.view(&Theme::TokyoNight));
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
        drop(library.view(&Theme::TokyoNight));
        let _ = library.update(Message::ConfirmRemove);
        assert!(library.busy);
        assert_eq!(library.books.len(), 2);
        let _ = library.update(Message::Saved(Ok((vec![], b"[]".to_vec()))));
        assert!(library.books.is_empty());
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
        drop(library.view(&Theme::TokyoNight));
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

    fn test_storage(name: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!("novelcraft-library-db-{name}-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))).join("library.json")
    }
    fn clean_storage(path: &Path) { std::fs::remove_dir_all(path.parent().unwrap()).unwrap(); }

    #[tokio::test]
    async fn migration_round_trips_order_and_does_not_touch_legacy_or_books() {
        let path = test_storage("migrate"); std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let old = path.parent().unwrap().join("old.txt"); let new = path.parent().unwrap().join("new.txt");
        std::fs::write(&old, b"old").unwrap(); std::fs::write(&new, b"new").unwrap();
        let initial = vec![old.clone(), old.clone(), new.clone()];
        let original = serde_json::to_vec_pretty(&initial).unwrap(); std::fs::write(&path, &original).unwrap();
        let (books, token) = load_from(&path).await.unwrap(); assert_eq!(books, initial);
        let remaining = without_book(&books, &old); assert_eq!(remaining, vec![new.clone()]);
        let token = save_to(&path, &remaining, token).await.unwrap();
        let (books, current) = load_from(&path).await.unwrap(); assert_eq!(books, remaining); assert_eq!(current, Some(token));
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert_eq!(std::fs::read(old).unwrap(), b"old"); assert_eq!(std::fs::read(new).unwrap(), b"new"); clean_storage(&path);
    }

    #[tokio::test]
    async fn stale_revisions_and_database_replacement_do_not_overwrite() {
        let path = test_storage("conflict");
        let (_, initial) = load_from(&path).await.unwrap();
        let initial = initial.unwrap(); let a = vec![PathBuf::from("/a")]; let b = vec![PathBuf::from("/b")];
        let changed = save_to(&path, &a, Some(initial.clone())).await.unwrap();
        assert!(save_to(&path, &b, Some(initial.clone())).await.is_err());
        let _ = save_to(&path, &[], Some(changed)).await.unwrap();
        assert!(save_to(&path, &b, Some(initial.clone())).await.is_err()); // ABA
        let (_, before_external) = load_from(&path).await.unwrap();
        let db = Connection::open(path.parent().unwrap().join(LIBRARY_DB)).unwrap();
        db.execute("UPDATE library SET data='[\"/external\"]'", []).unwrap(); drop(db);
        assert!(save_to(&path, &b, before_external).await.is_err());
        assert_eq!(load_from(&path).await.unwrap().0, vec![PathBuf::from("/external")]);
        let (_, before_delete) = load_from(&path).await.unwrap();
        std::fs::remove_file(path.parent().unwrap().join(LIBRARY_DB)).unwrap();
        assert!(save_to(&path, &b, before_delete.clone()).await.is_err());
        assert!(!path.parent().unwrap().join(LIBRARY_DB).exists());
        load_from(&path).await.unwrap();
        assert!(save_to(&path, &b, before_delete).await.is_err()); // new DB identity
        clean_storage(&path);
    }

    #[tokio::test]
    async fn relink_and_removal_preserve_books_and_bookmarks() {
        let path = test_storage("relink"); std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let old = path.parent().unwrap().join("old.txt"); let new = path.parent().unwrap().join("new.txt");
        let bookmarks = path.parent().unwrap().join("bookmarks.json");
        std::fs::write(&old, b"old").unwrap(); std::fs::write(&new, b"new").unwrap(); std::fs::write(&bookmarks, b"bookmark").unwrap();
        let (_, token) = load_from(&path).await.unwrap();
        let token = save_to(&path, &[old.clone()], token).await.unwrap();
        let updated = replace_book(&[old.clone()], &old, &new).unwrap();
        assert!(validate_relink_target(path.parent().unwrap()).await.is_err());
        std::fs::remove_file(&new).unwrap();
        assert!(save_relinked_to(&path, updated.clone(), Some(token.clone()), &new).await.is_err());
        std::fs::write(&new, b"new").unwrap();
        let (saved, token) = save_relinked_to(&path, updated.clone(), Some(token), &new).await.unwrap();
        assert_eq!(saved, updated); assert_eq!(load_from(&path).await.unwrap().0, updated);
        save_to(&path, &without_book(&saved, &new), Some(token)).await.unwrap();
        assert_eq!(std::fs::read(old).unwrap(), b"old"); assert_eq!(std::fs::read(new).unwrap(), b"new");
        assert_eq!(std::fs::read(bookmarks).unwrap(), b"bookmark"); clean_storage(&path);
    }

    #[tokio::test]
    async fn detects_changed_created_deleted_or_invalid_legacy_file() {
        let path = test_storage("legacy"); std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = b"[\"/old\"]"; std::fs::write(&path, original).unwrap();
        let (_, token) = load_from(&path).await.unwrap();
        save_to(&path, &[PathBuf::from("/new")], token).await.unwrap();
        std::fs::write(&path, b"[]").unwrap(); assert!(load_from(&path).await.is_err());
        std::fs::remove_file(&path).unwrap(); assert!(load_from(&path).await.is_err());
        std::fs::write(&path, original).unwrap(); assert_eq!(load_from(&path).await.unwrap().0, vec![PathBuf::from("/new")]);
        clean_storage(&path);
        load_from(&path).await.unwrap(); std::fs::write(&path, b"[]").unwrap(); assert!(load_from(&path).await.is_err()); clean_storage(&path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap(); std::fs::write(&path, b"invalid").unwrap();
        assert!(load_from(&path).await.is_err()); assert!(!path.parent().unwrap().join(LIBRARY_DB).exists());
        assert_eq!(std::fs::read(&path).unwrap(), b"invalid"); clean_storage(&path);
    }

    #[tokio::test]
    async fn concurrent_saves_only_one_succeeds() {
        let path = test_storage("concurrent"); let (_, token) = load_from(&path).await.unwrap(); let token = token.unwrap();
        let a = path.clone(); let b = path.clone(); let t = token.clone();
        let first = tokio::spawn(async move { save_to(&a, &[PathBuf::from("/a")], Some(t)).await });
        let second = tokio::spawn(async move { save_to(&b, &[PathBuf::from("/b")], Some(token)).await });
        let first = first.await.unwrap(); let second = second.await.unwrap(); assert_ne!(first.is_ok(), second.is_ok());
        assert_eq!(load_from(&path).await.unwrap().0.len(), 1); clean_storage(&path);
    }

    #[tokio::test]
    async fn view_controls_do_not_change_library_or_book() {
        let path = test_storage("view"); std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = path.parent().unwrap().join("book.txt"); std::fs::write(&file, b"content").unwrap();
        let (_, token) = load_from(&path).await.unwrap(); let token = save_to(&path, &[file.clone()], token).await.unwrap();
        let mut library = Library { books: vec![file.clone()], loaded: true, snapshot: Some(token.clone()), ..Library::default() };
        let _ = library.update(Message::Search("book".into())); let _ = library.update(Message::CycleSort); let _ = library.update(Message::ToggleMissingOnly);
        assert_eq!(load_from(&path).await.unwrap(), (vec![file.clone()], Some(token)));
        assert_eq!(std::fs::read(file).unwrap(), b"content"); clean_storage(&path);
    }

    #[test]
    fn refuses_unrelated_database() {
        let path = test_storage("foreign"); std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let db_path = path.parent().unwrap().join(LIBRARY_DB); let db = Connection::open(&db_path).unwrap();
        db.execute("CREATE TABLE other(value TEXT)", []).unwrap(); drop(db);
        let before = std::fs::read(&db_path).unwrap(); assert!(database_operation(&path, None).is_err());
        assert_eq!(std::fs::read(db_path).unwrap(), before); clean_storage(&path);
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlinks_non_utf8_and_checks_permissions() {
        use std::os::unix::{ffi::OsStringExt, fs::{symlink, PermissionsExt}};
        let path = test_storage("symlink"); std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let target = path.parent().unwrap().join("target"); std::fs::write(&target, b"[]").unwrap();
        symlink(&target, &path).unwrap(); assert!(database_operation(&path, None).is_err()); std::fs::remove_file(&path).unwrap();
        symlink(&target, path.parent().unwrap().join(LIBRARY_DB)).unwrap(); assert!(database_operation(&path, None).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"[]"); std::fs::remove_file(path.parent().unwrap().join(LIBRARY_DB)).unwrap();
        let (_, token) = database_operation(&path, None).unwrap();
        let invalid = PathBuf::from(std::ffi::OsString::from_vec(vec![0xff]));
        assert!(database_operation(&path, Some((&[invalid], &token))).is_err());
        assert_eq!(std::fs::metadata(path.parent().unwrap().join(LIBRARY_DB)).unwrap().permissions().mode() & 0o077, 0);
        clean_storage(&path);
    }
}
