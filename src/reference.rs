//! Character and world-building notes, stored separately from chapter Markdown.
use iced::widget::{button, column, row, scrollable, text, text_input, Column};
use iced::{Element, Length, Task};
use rusqlite::{params, Connection, OpenFlags, TransactionBehavior};
use std::path::{Path, PathBuf};
use std::time::Duration;

const DB_NAME: &str = ".novelcraft.sqlite3";
const APP_ID: i64 = 1313030726;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind { Character, Place, Time, Lore }
impl Kind {
    fn code(self) -> &'static str { match self { Self::Character => "character", Self::Place => "place", Self::Time => "time", Self::Lore => "lore" } }
    fn label(self) -> &'static str { match self { Self::Character => "Karakter", Self::Place => "Tempat", Self::Time => "Waktu", Self::Lore => "Lore" } }
    fn parse(code: &str) -> Result<Self, String> { match code { "character" => Ok(Self::Character), "place" => Ok(Self::Place), "time" => Ok(Self::Time), "lore" => Ok(Self::Lore), _ => Err("Jenis catatan tidak dikenal.".into()) } }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry { id: i64, revision: i64, kind: Kind, name: String, description: String, relations: String }
#[derive(Debug, Clone, PartialEq, Eq)]
struct Form { kind: Kind, name: String, description: String, relations: String }
impl Form {
    fn blank(kind: Kind) -> Self { Self { kind, name: String::new(), description: String::new(), relations: String::new() } }
    fn from_entry(entry: &Entry) -> Self { Self { kind: entry.kind, name: entry.name.clone(), description: entry.description.clone(), relations: entry.relations.clone() } }
}

#[derive(Default)]
pub struct Reference {
    folder: Option<PathBuf>, entries: Vec<Entry>, loaded: bool, busy: bool,
    selection: Option<(i64, i64)>, form: Option<Form>, baseline: Option<Form>,
    pending_delete: bool, notice: Option<String>,
}
#[derive(Debug, Clone)]
pub enum Message {
    Load(PathBuf), Loaded(Result<Vec<Entry>, String>),
    New(Kind), Select(i64), Name(String), Description(String), Relations(String),
    CancelEdit, Save, AskDelete, CancelDelete, ConfirmDelete,
    Changed(Result<Vec<Entry>, String>),
}
#[derive(Debug, Clone)]
enum Change { Insert(Form), Update(i64, i64, Form), Delete(i64, i64) }

impl Reference {
    pub fn is_busy(&self) -> bool { self.busy }
    pub fn is_dirty(&self) -> bool { self.form != self.baseline }
    pub fn is_loaded(&self) -> bool { self.loaded }
    fn allow_switch(&mut self) -> bool {
        if self.is_dirty() { self.notice = Some("Simpan atau batalkan perubahan catatan terlebih dahulu.".into()); false } else { true }
    }
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Load(folder) if !self.busy && !self.pending_delete && self.allow_switch() => {
                self.busy = true; self.loaded = false; self.folder = Some(folder.clone()); self.notice = None;
                return Task::perform(run_load(folder), Message::Loaded);
            }
            Message::Loaded(result) => {
                self.busy = false;
                match result {
                    Ok(entries) => {
                        self.entries = entries; self.loaded = true; self.selection = None;
                        self.form = None; self.baseline = None; self.notice = None;
                    }
                    Err(e) => { self.loaded = false; self.notice = Some(e); }
                }
            }
            Message::New(kind) if self.loaded && !self.busy && !self.pending_delete && self.allow_switch() => {
                let form = Form::blank(kind); self.selection = None; self.baseline = Some(form.clone()); self.form = Some(form); self.notice = None;
            }
            Message::Select(id) if self.loaded && !self.busy && !self.pending_delete && self.allow_switch() => {
                if let Some(entry) = self.entries.iter().find(|entry| entry.id == id) {
                    let form = Form::from_entry(entry); self.selection = Some((entry.id, entry.revision)); self.baseline = Some(form.clone()); self.form = Some(form); self.notice = None;
                }
            }
            Message::Name(value) if !self.busy && !self.pending_delete => if let Some(form) = &mut self.form { form.name = value; },
            Message::Description(value) if !self.busy && !self.pending_delete => if let Some(form) = &mut self.form { form.description = value; },
            Message::Relations(value) if !self.busy && !self.pending_delete => if let Some(form) = &mut self.form { form.relations = value; },
            Message::CancelEdit if !self.busy && !self.pending_delete => { self.form = None; self.baseline = None; self.selection = None; self.notice = None; }
            Message::Save if self.loaded && !self.busy && !self.pending_delete => {
                if let (Some(folder), Some(form)) = (self.folder.clone(), self.form.clone()) {
                    let change = match self.selection { Some((id, revision)) => Change::Update(id, revision, form), None => Change::Insert(form) };
                    self.busy = true; self.notice = None;
                    return Task::perform(run_change(folder, change), Message::Changed);
                }
            }
            Message::AskDelete if self.loaded && !self.busy && self.selection.is_some() && !self.is_dirty() => self.pending_delete = true,
            Message::CancelDelete if !self.busy => self.pending_delete = false,
            Message::ConfirmDelete if !self.busy && self.pending_delete => {
                if let (Some(folder), Some((id, revision))) = (self.folder.clone(), self.selection) {
                    self.pending_delete = false; self.busy = true; self.notice = None;
                    return Task::perform(run_change(folder, Change::Delete(id, revision)), Message::Changed);
                }
            }
            Message::Changed(result) => {
                self.busy = false;
                match result {
                    Ok(entries) => { self.entries = entries; self.selection = None; self.form = None; self.baseline = None; self.notice = None; }
                    Err(e) => { self.notice = Some(e); }
                }
            }
            _ => {}
        }
        Task::none()
    }
    pub fn view(&self) -> Element<'_, Message> {
        let mut content = column![text("Karakter dan world-building").size(20)].spacing(8);
        if !self.loaded && !self.busy {
            if let Some(folder) = &self.folder { content = content.push(button("Coba muat ulang catatan").on_press(Message::Load(folder.clone()))); }
        }
        if self.busy { content = content.push(text("Memproses catatan...")); }
        if let Some(notice) = &self.notice { content = content.push(text(notice).size(13)); }
        if self.loaded {
            if let Some(folder) = &self.folder {
                let mut reload = button("Muat ulang catatan");
                if !self.busy && !self.pending_delete { reload = reload.on_press(Message::Load(folder.clone())); }
                content = content.push(reload);
            }
            let mut kinds = iced::widget::Row::new().spacing(6);
            for kind in [Kind::Character, Kind::Place, Kind::Time, Kind::Lore] {
                let mut add = button(text(format!("+ {}", kind.label())).size(12));
                if !self.busy && !self.pending_delete { add = add.on_press(Message::New(kind)); }
                kinds = kinds.push(add);
            }
            content = content.push(kinds.wrap());
            let mut list = Column::new().spacing(4);
            for entry in &self.entries {
                let mut select = button(text(format!("{}: {}", entry.kind.label(), entry.name)).size(13)).width(Length::Fill);
                if !self.busy && !self.pending_delete { select = select.on_press(Message::Select(entry.id)); }
                list = list.push(select);
            }
            content = content.push(scrollable(list).height(120));
            if let Some(form) = &self.form {
                content = content.push(text(format!("{}{}", form.kind.label(), if self.is_dirty() { " · belum disimpan" } else { "" })).size(14));
                let mut name = text_input("Nama / judul", &form.name);
                let mut description = text_input("Deskripsi / catatan", &form.description);
                let mut relations = text_input("Relasi (teks bebas)", &form.relations);
                let mut save = button("Simpan catatan");
                let mut cancel = button("Batal edit");
                let mut delete = button("Hapus catatan");
                if !self.busy && !self.pending_delete {
                    name = name.on_input(Message::Name); description = description.on_input(Message::Description);
                    relations = relations.on_input(Message::Relations); save = save.on_press(Message::Save); cancel = cancel.on_press(Message::CancelEdit);
                    if self.selection.is_some() && !self.is_dirty() { delete = delete.on_press(Message::AskDelete); }
                }
                content = content.push(name).push(description);
                if form.kind == Kind::Character { content = content.push(relations); }
                content = content.push(row![save, cancel, delete].spacing(8).wrap());
            }
            if self.pending_delete {
                content = content.push(text("Hapus catatan ini secara permanen dari database?"));
                content = content.push(row![button("Hapus").on_press(Message::ConfirmDelete), button("Batal").on_press(Message::CancelDelete)].spacing(8));
            }
        }
        scrollable(content).height(Length::Fill).into()
    }
}

async fn run_load(folder: PathBuf) -> Result<Vec<Entry>, String> {
    tokio::task::spawn_blocking(move || load(&folder)).await.map_err(|e| format!("Proses database gagal: {e}"))?
}
async fn run_change(folder: PathBuf, change: Change) -> Result<Vec<Entry>, String> {
    tokio::task::spawn_blocking(move || change_entry(&folder, change)).await.map_err(|e| format!("Proses database gagal: {e}"))?
}

pub(crate) fn connection(folder: &Path) -> Result<Connection, String> {
    let path = folder.join(DB_NAME);
    let mut options = std::fs::OpenOptions::new(); options.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
    let created = match options.open(&path) {
        Ok(file) => { drop(file); true }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(e) => return Err(format!("Gagal membuka database catatan: {e}")),
    };
    let meta = std::fs::symlink_metadata(&path).map_err(|e| format!("Gagal memeriksa database: {e}"))?;
    if !meta.is_file() || meta.file_type().is_symlink() { return Err("Database catatan harus berupa file biasa, bukan symbolic link.".into()); }
    let mut db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|e| format!("Gagal membuka database catatan: {e}"))?;
    db.busy_timeout(Duration::from_secs(3)).map_err(|e| e.to_string())?;
    if created {
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e| format!("Gagal menyiapkan database: {e}"))?;
        tx.execute_batch(&format!("PRAGMA application_id={APP_ID}; PRAGMA user_version=1; CREATE TABLE entries (id INTEGER PRIMARY KEY AUTOINCREMENT, revision INTEGER NOT NULL, kind TEXT NOT NULL CHECK(kind IN ('character','place','time','lore')), name TEXT NOT NULL, description TEXT NOT NULL, relations TEXT NOT NULL);"))
            .map_err(|e| format!("Gagal menyiapkan database; periksa {}: {e}", path.display()))?;
        tx.commit().map_err(|e| format!("Gagal menyiapkan database; periksa {}: {e}", path.display()))?;
    } else {
        let app: i64 = db.query_row("PRAGMA application_id", [], |r| r.get(0)).map_err(|e| format!("Database tidak valid: {e}"))?;
        let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(|e| format!("Database tidak valid: {e}"))?;
        if app != APP_ID || !(1..=2).contains(&version) { return Err("Database bukan database catatan NovelCraft atau versinya tidak didukung; file tidak diubah.".into()); }
    }
    // Migrate under a write lock, rechecking the version to handle concurrent opens.
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e| format!("Gagal memeriksa versi database: {e}"))?;
    let version: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(|e| format!("Database tidak valid: {e}"))?;
    if version == 1 {
        tx.execute_batch("CREATE TABLE writing_days (day TEXT PRIMARY KEY, net_words INTEGER NOT NULL); CREATE TABLE writing_settings (key TEXT PRIMARY KEY, value INTEGER NOT NULL); PRAGMA user_version=2;")
            .map_err(|e| format!("Gagal memperbarui database: {e}"))?;
    } else if version != 2 {
        return Err("Versi database tidak didukung; file tidak diubah.".into());
    }
    tx.commit().map_err(|e| format!("Gagal memperbarui database: {e}"))?;
    Ok(db)
}
fn load_entries(db: &Connection) -> Result<Vec<Entry>, String> {
    let mut stmt = db.prepare("SELECT id, revision, kind, name, description, relations FROM entries ORDER BY id").map_err(|e| format!("Gagal membaca catatan: {e}"))?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?)))
        .map_err(|e| format!("Gagal membaca catatan: {e}"))?;
    let mut entries = Vec::new();
    for row in rows {
        let (id, revision, kind, name, description, relations) = row.map_err(|e| format!("Gagal membaca catatan: {e}"))?;
        entries.push(Entry { id, revision, kind: Kind::parse(&kind)?, name, description, relations });
    }
    Ok(entries)
}
fn load(folder: &Path) -> Result<Vec<Entry>, String> { load_entries(&connection(folder)?) }
fn validate(form: &Form) -> Result<(), String> {
    if form.name.trim().is_empty() || form.name.chars().count() > 150 { return Err("Nama / judul wajib diisi, maksimal 150 karakter.".into()); }
    if form.description.len() > 100_000 || form.relations.len() > 100_000 { return Err("Catatan terlalu panjang (maksimal 100.000 byte per kolom).".into()); }
    Ok(())
}
fn change_entry(folder: &Path, change: Change) -> Result<Vec<Entry>, String> {
    let mut db = connection(folder)?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e| format!("Gagal memulai perubahan: {e}"))?;
    let changed = match change {
        Change::Insert(form) => { validate(&form)?; tx.execute("INSERT INTO entries(revision,kind,name,description,relations) VALUES(1,?1,?2,?3,?4)", params![form.kind.code(), form.name.trim(), form.description, form.relations]) }
        Change::Update(id, revision, form) => { validate(&form)?; tx.execute("UPDATE entries SET revision=revision+1, kind=?1, name=?2, description=?3, relations=?4 WHERE id=?5 AND revision=?6", params![form.kind.code(), form.name.trim(), form.description, form.relations, id, revision]) }
        Change::Delete(id, revision) => tx.execute("DELETE FROM entries WHERE id=?1 AND revision=?2", params![id, revision]),
    }.map_err(|e| format!("Gagal menyimpan catatan: {e}"))?;
    if changed != 1 { return Err("Catatan berubah atau dihapus di luar aplikasi; batalkan edit dan muat ulang.".into()); }
    let result = load_entries(&tx)?;
    tx.commit().map_err(|e| format!("Gagal menyimpan catatan: {e}"))?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    fn dir() -> PathBuf { static N: AtomicU64 = AtomicU64::new(0); std::env::temp_dir().join(format!("novelcraft-ref-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed))) }
    #[test]
    fn dirty_form_cannot_be_discarded_by_switching_entries() {
        let mut reference = Reference::default();
        let _ = reference.update(Message::Loaded(Ok(vec![])));
        let _ = reference.update(Message::New(Kind::Character));
        let _ = reference.update(Message::Name("Lintang".into()));
        assert!(reference.is_dirty());
        let _ = reference.update(Message::New(Kind::Lore));
        assert_eq!(reference.form.as_ref().unwrap().name, "Lintang");
        assert_eq!(reference.form.as_ref().unwrap().kind, Kind::Character);
        let _ = reference.update(Message::CancelEdit);
        assert!(!reference.is_dirty());
    }
    #[test]
    fn reload_does_not_discard_dirty_form_and_clears_stale_selection() {
        let mut reference = Reference::default();
        let _ = reference.update(Message::Loaded(Ok(vec![Entry { id: 1, revision: 1, kind: Kind::Character, name: "A".into(), description: "".into(), relations: "".into() }])));
        let _ = reference.update(Message::Select(1));
        let _ = reference.update(Message::Name("unsaved".into()));
        let _ = reference.update(Message::Load("/tmp".into()));
        assert!(!reference.is_busy());
        assert_eq!(reference.form.as_ref().unwrap().name, "unsaved");
        let _ = reference.update(Message::CancelEdit);
        let _ = reference.update(Message::Select(1));
        let _ = reference.update(Message::Loaded(Ok(vec![])));
        assert!(reference.form.is_none());
        assert!(reference.selection.is_none());
    }
    #[test]
    fn world_categories_are_persisted() {
        let dir = dir(); std::fs::create_dir_all(&dir).unwrap();
        for kind in [Kind::Place, Kind::Time, Kind::Lore] {
            let mut form = Form::blank(kind); form.name = kind.label().into(); form.description = "catatan dunia".into();
            change_entry(&dir, Change::Insert(form)).unwrap();
        }
        assert_eq!(load(&dir).unwrap().iter().map(|e| e.kind).collect::<Vec<_>>(), vec![Kind::Place, Kind::Time, Kind::Lore]);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn sqlite_crud_reopens_and_rejects_stale_changes() {
        let dir = dir(); std::fs::create_dir_all(&dir).unwrap();
        assert!(load(&dir).unwrap().is_empty());
        let mut form = Form::blank(Kind::Character); form.name = "Lintang".into(); form.description = "tokoh".into(); form.relations = "teman Ikal".into();
        let entries = change_entry(&dir, Change::Insert(form.clone())).unwrap();
        let entry = entries[0].clone();
        form.description = "tokoh utama".into();
        let updated = change_entry(&dir, Change::Update(entry.id, entry.revision, form.clone())).unwrap();
        assert_eq!(load(&dir).unwrap(), updated);
        assert!(change_entry(&dir, Change::Update(entry.id, entry.revision, form)).is_err());
        assert!(change_entry(&dir, Change::Delete(entry.id, entry.revision)).is_err());
        assert_eq!(load(&dir).unwrap(), updated);
        assert!(change_entry(&dir, Change::Delete(entry.id, updated[0].revision)).unwrap().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn upgrades_version_one_without_losing_notes() {
        let dir = dir(); std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(DB_NAME);
        let db = Connection::open(&path).unwrap();
        db.execute_batch(&format!("PRAGMA application_id={APP_ID}; PRAGMA user_version=1; CREATE TABLE entries (id INTEGER PRIMARY KEY AUTOINCREMENT, revision INTEGER NOT NULL, kind TEXT NOT NULL, name TEXT NOT NULL, description TEXT NOT NULL, relations TEXT NOT NULL); INSERT INTO entries(revision,kind,name,description,relations) VALUES(1,'character','Nama','Detail','Relasi');")).unwrap();
        drop(db);
        let entries = load(&dir).unwrap(); assert_eq!(entries[0].name, "Nama"); assert_eq!(entries[0].relations, "Relasi");
        let db = connection(&dir).unwrap();
        assert_eq!(db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)).unwrap(), 2);
        assert_eq!(db.query_row("SELECT COUNT(*) FROM writing_days", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        drop(db); std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn refuses_foreign_database_and_invalid_form() {
        let dir = dir(); std::fs::create_dir_all(&dir).unwrap();
        let db = Connection::open(dir.join(DB_NAME)).unwrap(); db.execute("CREATE TABLE other(value TEXT)", []).unwrap(); drop(db);
        let before = std::fs::read(dir.join(DB_NAME)).unwrap();
        assert!(load(&dir).is_err());
        assert_eq!(before, std::fs::read(dir.join(DB_NAME)).unwrap());
        assert!(validate(&Form::blank(Kind::Lore)).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn refuses_database_symlink() {
        let dir = dir(); std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("other"); std::fs::write(&target, b"unchanged").unwrap();
        std::os::unix::fs::symlink(&target, dir.join(DB_NAME)).unwrap();
        assert!(load(&dir).is_err()); assert_eq!(std::fs::read(target).unwrap(), b"unchanged");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
