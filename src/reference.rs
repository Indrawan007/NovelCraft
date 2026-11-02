//! Character and world-building notes, stored separately from chapter Markdown.
use iced::widget::{button, column, pick_list, row, scrollable, text, text_input, Column};
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterChoice { id: i64, name: String }
impl std::fmt::Display for CharacterChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{} (#{})", self.name, self.id) }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relationship { id: i64, revision: i64, source: i64, source_name: String, target: i64, target_name: String, label: String }

#[derive(Default)]
pub struct Reference {
    folder: Option<PathBuf>, entries: Vec<Entry>, loaded: bool, busy: bool,
    selection: Option<(i64, i64)>, form: Option<Form>, baseline: Option<Form>,
    pending_delete: bool, pending_unlink: Option<(i64, i64)>, notice: Option<String>,
    relationships: Vec<Relationship>, relationships_loaded: bool,
    relation_target: Option<CharacterChoice>, relation_label: String,
}
#[derive(Debug, Clone)]
pub enum Message {
    Load(PathBuf), Loaded(Result<Vec<Entry>, String>),
    New(Kind), Select(i64), Name(String), Description(String), Relations(String),
    CancelEdit, Save, AskDelete, CancelDelete, ConfirmDelete,
    RelationshipsLoaded(Result<Vec<Relationship>, String>), ReloadRelationships,
    RelationTarget(CharacterChoice), RelationLabel(String), CancelRelation, AddRelation,
    AskUnlink(i64), CancelUnlink, ConfirmUnlink,
    Changed(Result<Vec<Entry>, String>),
}
#[derive(Debug, Clone)]
enum Change { Insert(Form), Update(i64, i64, Form), Delete(i64, i64), Link(i64, i64, i64, String), Unlink(i64, i64, i64) }

impl Reference {
    pub fn is_busy(&self) -> bool { self.busy }
    fn relation_dirty(&self) -> bool { self.relation_target.is_some() || !self.relation_label.is_empty() }
    fn blocked(&self) -> bool { self.busy || self.pending_delete || self.pending_unlink.is_some() }
    pub fn is_dirty(&self) -> bool { self.form != self.baseline || self.relation_dirty() }
    pub fn is_loaded(&self) -> bool { self.loaded }
    fn allow_switch(&mut self) -> bool {
        if self.is_dirty() { self.notice = Some("Simpan atau batalkan perubahan catatan terlebih dahulu.".into()); false } else { true }
    }
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Load(folder) if !self.blocked() && self.allow_switch() => {
                self.busy = true; self.loaded = false; self.folder = Some(folder.clone()); self.notice = None;
                return Task::perform(run_load(folder), Message::Loaded);
            }
            Message::Loaded(result) => {
                self.busy = false;
                match result {
                    Ok(entries) => {
                        self.entries = entries; self.loaded = true; self.selection = None;
                        self.form = None; self.baseline = None; self.relationships.clear(); self.relationships_loaded = false; self.notice = None;
                    }
                    Err(e) => { self.loaded = false; self.notice = Some(e); }
                }
            }
            Message::New(kind) if self.loaded && !self.blocked() && self.allow_switch() => {
                let form = Form::blank(kind); self.selection = None; self.baseline = Some(form.clone()); self.form = Some(form); self.relationships.clear(); self.relationships_loaded = false; self.notice = None;
            }
            Message::Select(id) if self.loaded && !self.blocked() && self.allow_switch() => {
                if let Some(entry) = self.entries.iter().find(|entry| entry.id == id) {
                    let form = Form::from_entry(entry); self.selection = Some((entry.id, entry.revision)); self.baseline = Some(form.clone()); self.form = Some(form); self.notice = None;
                    self.relationships.clear(); self.relationships_loaded = false;
                    if entry.kind == Kind::Character {
                        if let Some(folder) = self.folder.clone() {
                            self.busy = true;
                            return Task::perform(run_load_relationships(folder, id), Message::RelationshipsLoaded);
                        }
                    }
                }
            }
            Message::Name(value) if !self.blocked() && !self.relation_dirty() => if let Some(form) = &mut self.form { form.name = value; },
            Message::Description(value) if !self.blocked() && !self.relation_dirty() => if let Some(form) = &mut self.form { form.description = value; },
            Message::Relations(value) if !self.blocked() && !self.relation_dirty() => if let Some(form) = &mut self.form { form.relations = value; },
            Message::CancelEdit if !self.blocked() => { self.form = None; self.baseline = None; self.selection = None; self.relationships.clear(); self.relationships_loaded = false; self.relation_target = None; self.relation_label.clear(); self.notice = None; }
            Message::Save if self.loaded && !self.blocked() && !self.relation_dirty() => {
                if let (Some(folder), Some(form)) = (self.folder.clone(), self.form.clone()) {
                    let change = match self.selection { Some((id, revision)) => Change::Update(id, revision, form), None => Change::Insert(form) };
                    self.busy = true; self.notice = None;
                    return Task::perform(run_change(folder, change), Message::Changed);
                }
            }
            Message::AskDelete if self.loaded && !self.blocked() && self.selection.is_some() && !self.is_dirty() => self.pending_delete = true,
            Message::CancelDelete if !self.busy => self.pending_delete = false,
            Message::ConfirmDelete if !self.busy && self.pending_delete => {
                if let (Some(folder), Some((id, revision))) = (self.folder.clone(), self.selection) {
                    self.pending_delete = false; self.busy = true; self.notice = None;
                    return Task::perform(run_change(folder, Change::Delete(id, revision)), Message::Changed);
                }
            }
            Message::RelationshipsLoaded(result) => {
                self.busy = false;
                match result {
                    Ok(items) => { self.relationships = items; self.relationships_loaded = true; self.notice = None; }
                    Err(e) => { self.relationships_loaded = false; self.notice = Some(e); }
                }
            }
            Message::ReloadRelationships if self.loaded && !self.blocked() && !self.is_dirty() => {
                if let (Some(folder), Some((id, _))) = (self.folder.clone(), self.selection) {
                    self.busy = true;
                    return Task::perform(run_load_relationships(folder, id), Message::RelationshipsLoaded);
                }
            }
            Message::RelationTarget(target) if !self.blocked() && self.form == self.baseline => self.relation_target = Some(target),
            Message::RelationLabel(label) if !self.blocked() && self.form == self.baseline => self.relation_label = label,
            Message::CancelRelation if !self.blocked() => { self.relation_target = None; self.relation_label.clear(); self.notice = None; }
            Message::AddRelation if self.loaded && !self.blocked() && self.form == self.baseline => {
                if let (Some(folder), Some((source, revision)), Some(target)) = (self.folder.clone(), self.selection, self.relation_target.clone()) {
                    self.busy = true; self.notice = None;
                    return Task::perform(run_change(folder, Change::Link(source, revision, target.id, self.relation_label.clone())), Message::Changed);
                }
            }
            Message::AskUnlink(id) if self.loaded && !self.blocked() && !self.is_dirty() => {
                if let Some(item) = self.relationships.iter().find(|item| item.id == id) { self.pending_unlink = Some((item.id, item.revision)); }
            }
            Message::CancelUnlink if !self.busy => self.pending_unlink = None,
            Message::ConfirmUnlink if !self.busy => {
                if let (Some((id, revision)), Some(folder), Some((selected, _))) = (self.pending_unlink.take(), self.folder.clone(), self.selection) {
                    self.busy = true; self.notice = None;
                    return Task::perform(run_change(folder, Change::Unlink(id, revision, selected)), Message::Changed);
                }
            }
            Message::Changed(result) => {
                self.busy = false;
                match result {
                    Ok(entries) => { self.entries = entries; self.selection = None; self.form = None; self.baseline = None; self.relationships.clear(); self.relationships_loaded = false; self.relation_target = None; self.relation_label.clear(); self.notice = None; }
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
                if !self.blocked() { reload = reload.on_press(Message::Load(folder.clone())); }
                content = content.push(reload);
            }
            let mut kinds = iced::widget::Row::new().spacing(6);
            for kind in [Kind::Character, Kind::Place, Kind::Time, Kind::Lore] {
                let mut add = button(text(format!("+ {}", kind.label())).size(12));
                if !self.blocked() { add = add.on_press(Message::New(kind)); }
                kinds = kinds.push(add);
            }
            content = content.push(kinds.wrap());
            let mut list = Column::new().spacing(4);
            for entry in &self.entries {
                let mut select = button(text(format!("{}: {}", entry.kind.label(), entry.name)).size(13)).width(Length::Fill);
                if !self.blocked() { select = select.on_press(Message::Select(entry.id)); }
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
                if !self.blocked() && !self.relation_dirty() {
                    name = name.on_input(Message::Name); description = description.on_input(Message::Description);
                    relations = relations.on_input(Message::Relations); save = save.on_press(Message::Save); cancel = cancel.on_press(Message::CancelEdit);
                    if self.selection.is_some() && !self.is_dirty() { delete = delete.on_press(Message::AskDelete); }
                }
                content = content.push(name).push(description);
                if form.kind == Kind::Character { content = content.push(relations); }
                content = content.push(row![save, cancel, delete].spacing(8).wrap());
            }
            if self.form.as_ref().is_some_and(|form| form.kind == Kind::Character) && self.selection.is_some() {
                content = content.push(text("Relasi antar-karakter (berarah)").size(16));
                if !self.relationships_loaded && !self.busy {
                    content = content.push(button("Muat ulang relasi").on_press(Message::ReloadRelationships));
                }
                for relation in &self.relationships {
                    let mut remove = button("Hapus relasi");
                    if !self.blocked() && !self.is_dirty() { remove = remove.on_press(Message::AskUnlink(relation.id)); }
                    content = content.push(row![text(format!("{} -> {}: {}", relation.source_name, relation.target_name, relation.label)).size(13), remove].spacing(8).wrap());
                }
                let selected_id = self.selection.unwrap().0;
                let choices: Vec<_> = self.entries.iter().filter(|e| e.kind == Kind::Character && e.id != selected_id)
                    .map(|e| CharacterChoice { id: e.id, name: e.name.clone() }).collect();
                if !choices.is_empty() && self.relationships_loaded {
                    if self.form != self.baseline { content = content.push(text("Simpan atau batalkan edit karakter sebelum menambah relasi.").size(12)); }
                    else {
                        let target = pick_list(choices, self.relation_target.clone(), Message::RelationTarget).placeholder("Pilih karakter tujuan");
                        let mut label = text_input("Jenis relasi, mis. teman", &self.relation_label);
                        let mut add = button("Tambah relasi"); let mut cancel = button("Batal relasi");
                        if !self.blocked() {
                            label = label.on_input(Message::RelationLabel);
                            if self.relation_target.is_some() { add = add.on_press(Message::AddRelation); }
                            cancel = cancel.on_press(Message::CancelRelation);
                        }
                        content = content.push(target).push(label).push(row![add, cancel].spacing(8));
                    }
                }
            }
            if self.pending_unlink.is_some() {
                content = content.push(text("Hapus relasi ini secara permanen?"));
                content = content.push(row![button("Hapus relasi").on_press(Message::ConfirmUnlink), button("Batal").on_press(Message::CancelUnlink)].spacing(8));
            }
            if self.pending_delete {
                content = content.push(text("Hapus catatan ini secara permanen? Semua relasi yang terkait dengan karakter ini juga akan dihapus."));
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
    db.pragma_update(None, "foreign_keys", true).map_err(|e| format!("Gagal mengaktifkan foreign key: {e}"))?;
    if created {
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e| format!("Gagal menyiapkan database: {e}"))?;
        tx.execute_batch(&format!("PRAGMA application_id={APP_ID}; PRAGMA user_version=1; CREATE TABLE entries (id INTEGER PRIMARY KEY AUTOINCREMENT, revision INTEGER NOT NULL, kind TEXT NOT NULL CHECK(kind IN ('character','place','time','lore')), name TEXT NOT NULL, description TEXT NOT NULL, relations TEXT NOT NULL);"))
            .map_err(|e| format!("Gagal menyiapkan database; periksa {}: {e}", path.display()))?;
        tx.commit().map_err(|e| format!("Gagal menyiapkan database; periksa {}: {e}", path.display()))?;
    } else {
        let app: i64 = db.query_row("PRAGMA application_id", [], |r| r.get(0)).map_err(|e| format!("Database tidak valid: {e}"))?;
        let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(|e| format!("Database tidak valid: {e}"))?;
        if app != APP_ID || !(1..=3).contains(&version) { return Err("Database bukan database catatan NovelCraft atau versinya tidak didukung; file tidak diubah.".into()); }
    }
    // Migrate under a write lock, rechecking the version to handle concurrent opens.
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e| format!("Gagal memeriksa versi database: {e}"))?;
    let version: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(|e| format!("Database tidak valid: {e}"))?;
    if version == 1 {
        tx.execute_batch("CREATE TABLE writing_days (day TEXT PRIMARY KEY, net_words INTEGER NOT NULL); CREATE TABLE writing_settings (key TEXT PRIMARY KEY, value INTEGER NOT NULL); PRAGMA user_version=2;")
            .map_err(|e| format!("Gagal memperbarui database: {e}"))?;
    } else if !(2..=3).contains(&version) {
        return Err("Versi database tidak didukung; file tidak diubah.".into());
    }
    if version <= 2 {
        tx.execute_batch("CREATE TABLE character_relationships (id INTEGER PRIMARY KEY AUTOINCREMENT, revision INTEGER NOT NULL DEFAULT 1, source_id INTEGER NOT NULL REFERENCES entries(id) ON DELETE CASCADE, target_id INTEGER NOT NULL REFERENCES entries(id) ON DELETE CASCADE, label TEXT NOT NULL, CHECK(source_id <> target_id), UNIQUE(source_id,target_id,label)); CREATE TRIGGER relationship_added AFTER INSERT ON character_relationships BEGIN UPDATE entries SET revision=revision+1 WHERE id IN (NEW.source_id,NEW.target_id); END; CREATE TRIGGER relationship_removed AFTER DELETE ON character_relationships BEGIN UPDATE entries SET revision=revision+1 WHERE id IN (OLD.source_id,OLD.target_id); END; PRAGMA user_version=3;")
            .map_err(|e| format!("Gagal memperbarui relasi karakter: {e}"))?;
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
async fn run_load_relationships(folder: PathBuf, character: i64) -> Result<Vec<Relationship>, String> {
    tokio::task::spawn_blocking(move || load_relationships(&crate::reference::connection(&folder)?, character)).await
        .map_err(|e| format!("Proses database gagal: {e}"))?
}
fn load_relationships(db: &Connection, character: i64) -> Result<Vec<Relationship>, String> {
    let mut stmt = db.prepare("SELECT r.id,r.revision,r.source_id,s.name,r.target_id,t.name,r.label FROM character_relationships r JOIN entries s ON s.id=r.source_id JOIN entries t ON t.id=r.target_id WHERE r.source_id=?1 OR r.target_id=?1 ORDER BY r.id")
        .map_err(|e| format!("Gagal membaca relasi: {e}"))?;
    let rows = stmt.query_map(params![character], |row| Ok(Relationship { id: row.get(0)?, revision: row.get(1)?, source: row.get(2)?, source_name: row.get(3)?, target: row.get(4)?, target_name: row.get(5)?, label: row.get(6)? }))
        .map_err(|e| format!("Gagal membaca relasi: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| format!("Gagal membaca relasi: {e}"))
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
        Change::Link(source, revision, target, label) => {
            let label = label.trim();
            if source == target || label.is_empty() || label.chars().count() > 100 || label.chars().any(char::is_control) {
                return Err("Relasi membutuhkan karakter berbeda dan label 1–100 karakter tanpa karakter kontrol.".into());
            }
            tx.execute("INSERT INTO character_relationships(source_id,target_id,label) SELECT ?1,?2,?3 WHERE EXISTS (SELECT 1 FROM entries WHERE id=?1 AND revision=?4 AND kind='character') AND EXISTS (SELECT 1 FROM entries WHERE id=?2 AND kind='character')", params![source, target, label, revision])
        }
        Change::Unlink(id, revision, selected) => tx.execute("DELETE FROM character_relationships WHERE id=?1 AND revision=?2 AND (source_id=?3 OR target_id=?3)", params![id, revision, selected]),
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
        assert_eq!(db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)).unwrap(), 3);
        assert_eq!(db.query_row("SELECT COUNT(*) FROM writing_days", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        drop(db); std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn structured_relationships_are_versioned_and_cascade_on_character_delete() {
        let dir = dir(); std::fs::create_dir_all(&dir).unwrap();
        let mut a = Form::blank(Kind::Character); a.name = "Lintang".into(); a.relations = "catatan lama".into();
        let mut b = Form::blank(Kind::Character); b.name = "Ikal".into();
        let mut place = Form::blank(Kind::Place); place.name = "Sekolah".into();
        change_entry(&dir, Change::Insert(a)).unwrap();
        change_entry(&dir, Change::Insert(b)).unwrap();
        let entries = change_entry(&dir, Change::Insert(place)).unwrap();
        let (source, target, invalid) = (&entries[0], &entries[1], &entries[2]);
        assert!(change_entry(&dir, Change::Link(source.id, source.revision, source.id, "diri".into())).is_err());
        assert!(change_entry(&dir, Change::Link(source.id, source.revision, invalid.id, "lokasi".into())).is_err());
        assert!(change_entry(&dir, Change::Link(source.id, source.revision, target.id, "   ".into())).is_err());
        let updated = change_entry(&dir, Change::Link(source.id, source.revision, target.id, "teman".into())).unwrap();
        assert_eq!(updated[0].relations, "catatan lama");
        assert!(updated[0].revision > source.revision && updated[1].revision > target.revision);
        assert!(change_entry(&dir, Change::Delete(target.id, target.revision)).is_err());
        assert!(change_entry(&dir, Change::Link(source.id, source.revision, target.id, "baru".into())).is_err());
        let db = connection(&dir).unwrap();
        let links = load_relationships(&db, target.id).unwrap();
        assert_eq!(links.len(), 1); assert_eq!(links[0].label, "teman");
        assert_eq!(links[0].source_name, "Lintang"); assert_eq!(links[0].target_name, "Ikal");
        drop(db);
        assert!(change_entry(&dir, Change::Unlink(links[0].id, links[0].revision + 1, source.id)).is_err());
        assert!(change_entry(&dir, Change::Unlink(links[0].id, links[0].revision, invalid.id)).is_err());
        let remaining = change_entry(&dir, Change::Delete(target.id, updated[1].revision)).unwrap();
        assert_eq!(remaining.len(), 2);
        assert!(remaining[0].revision > updated[0].revision); // cascade changes the surviving character
        let db = connection(&dir).unwrap(); assert!(load_relationships(&db, source.id).unwrap().is_empty());
        drop(db); std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn deleting_a_relationship_requires_confirmation_and_preserves_drafts() {
        let mut reference = Reference::default();
        let a = Entry { id: 1, revision: 1, kind: Kind::Character, name: "A".into(), description: "".into(), relations: "".into() };
        let b = Entry { id: 2, revision: 1, kind: Kind::Character, name: "B".into(), description: "".into(), relations: "".into() };
        let _ = reference.update(Message::Loaded(Ok(vec![a, b])));
        let _ = reference.update(Message::Select(1));
        reference.relationships_loaded = true;
        reference.relationships.push(Relationship { id: 10, revision: 1, source: 1, source_name: "A".into(), target: 2, target_name: "B".into(), label: "teman".into() });
        let _ = reference.update(Message::RelationTarget(CharacterChoice { id: 2, name: "B".into() }));
        let _ = reference.update(Message::RelationLabel("teman".into()));
        assert!(reference.is_dirty());
        let _ = reference.update(Message::Select(2));
        assert_eq!(reference.selection.unwrap().0, 1);
        let _ = reference.update(Message::AskUnlink(10));
        assert!(reference.pending_unlink.is_none());
        let _ = reference.update(Message::CancelRelation);
        let _ = reference.update(Message::AskUnlink(10));
        assert_eq!(reference.pending_unlink, Some((10, 1)));
        let _ = reference.update(Message::CancelUnlink);
        assert_eq!(reference.relationships.len(), 1);
        drop(reference.view());
    }

    #[test]
    fn upgrades_version_two_preserving_daily_history_and_character_notes() {
        let dir = dir(); std::fs::create_dir_all(&dir).unwrap();
        let db = connection(&dir).unwrap();
        db.execute_batch("DROP TRIGGER relationship_added; DROP TRIGGER relationship_removed; DROP TABLE character_relationships; PRAGMA user_version=2; INSERT INTO entries(revision,kind,name,description,relations) VALUES(1,'character','Nama','Detail','teks lama'); INSERT INTO writing_days VALUES('2025-02-01',42); INSERT INTO writing_settings VALUES('daily_target',500);").unwrap();
        drop(db);
        let db = connection(&dir).unwrap();
        assert_eq!(db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)).unwrap(), 3);
        assert_eq!(db.query_row("SELECT net_words FROM writing_days WHERE day='2025-02-01'", [], |r| r.get::<_, i64>(0)).unwrap(), 42);
        assert_eq!(db.query_row("SELECT value FROM writing_settings WHERE key='daily_target'", [], |r| r.get::<_, i64>(0)).unwrap(), 500);
        assert_eq!(db.query_row("SELECT relations FROM entries WHERE name='Nama'", [], |r| r.get::<_, String>(0)).unwrap(), "teks lama");
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
