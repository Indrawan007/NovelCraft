//! A project is an ordered index of ordinary Markdown files in one directory.
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use tokio::io::AsyncWriteExt;
use rusqlite::{params, Connection, OpenFlags, TransactionBehavior};
use std::sync::Mutex;
use std::time::Duration;

const MANIFEST: &str = ".novelcraft.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    chapters: Vec<Chapter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chapter {
    pub title: String,
    file: String,
}

#[derive(Debug, Clone)]
pub struct Project {
    folder: PathBuf,
    manifest: Manifest,
    snapshot: Vec<u8>,
}

impl Project {
    pub fn folder(&self) -> &Path { &self.folder }
    pub fn chapters(&self) -> &[Chapter] { &self.manifest.chapters }
    pub fn chapter_path(&self, index: usize) -> Option<PathBuf> {
        self.manifest.chapters.get(index).map(|chapter| self.folder.join(&chapter.file))
    }
}

fn validate(manifest: &Manifest) -> Result<(), String> {
    if manifest.version != 1 { return Err("Versi proyek tidak didukung.".into()); }
    let mut names = HashSet::new();
    for chapter in &manifest.chapters {
        validate_title(&chapter.title)?;
        if !valid_filename(&chapter.file) || !names.insert(&chapter.file) {
            return Err("Daftar file chapter tidak valid atau duplikat.".into());
        }
    }
    Ok(())
}

// Only a single filename in the project folder; never allow traversal.
fn valid_filename(name: &str) -> bool {
    let path = Path::new(name);
    let mut components = path.components();
    matches!((components.next(), components.next()), (Some(Component::Normal(_)), None))
        && !name.contains('/') && !name.contains('\\')
        && !name.chars().any(char::is_control)
        && path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| {
            ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown")
        })
}

fn validate_title(title: &str) -> Result<(), String> {
    if title.trim().is_empty() || title.chars().count() > 150 || title.chars().any(char::is_control) {
        return Err("Judul chapter harus berisi 1–150 karakter tanpa karakter kontrol.".into());
    }
    Ok(())
}

const PROJECT_DB: &str = ".novelcraft-project.sqlite3";
const PROJECT_APP_ID: i64 = 1313030736;

fn legacy_snapshot(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() =>
            Err("Metadata JSON proyek harus berupa file biasa, bukan symbolic link.".into()),
        Ok(_) => std::fs::read(path).map(Some).map_err(|e| format!("Gagal membaca metadata proyek: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("Gagal membaca metadata proyek: {e}")),
    }
}

fn parse_manifest(bytes: &[u8]) -> Result<Manifest, String> {
    let manifest: Manifest = serde_json::from_slice(bytes).map_err(|e| format!("Metadata proyek tidak valid: {e}"))?;
    validate(&manifest)?;
    Ok(manifest)
}

enum DbAction<'a> { Create, Open, Check(&'a [u8]), Save(&'a [u8], &'a Manifest) }

// The token contains database identity, revision and contents: stale instances and
// direct content changes are rejected without modifying either the index or Markdown.
fn database_operation(folder: &Path, action: DbAction<'_>) -> Result<(Manifest, Vec<u8>), String> {
    static INITIALIZATION: Mutex<()> = Mutex::new(());
    let _guard = INITIALIZATION.lock().map_err(|_| "Proses database proyek gagal; mulai ulang aplikasi.")?;
    let original = legacy_snapshot(&folder.join(MANIFEST))?;
    let legacy = original.as_deref().map(parse_manifest).transpose()?;
    let db_path = folder.join(PROJECT_DB);
    let created = match &action {
        DbAction::Create => {
            if original.is_some() { return Err("Metadata proyek sudah ada; gunakan Buka proyek, bukan Proyek baru.".into()); }
            // Avoid creating a new index next to orphaned notes or activity data.
            match std::fs::symlink_metadata(folder.join(".novelcraft.sqlite3")) {
                Ok(_) => return Err("Database catatan proyek sudah ada; periksa folder atau gunakan Buka proyek.".into()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
                Err(e) => return Err(format!("Gagal memeriksa folder proyek: {e}")),
            }
            create_database_file(&db_path, false)?
        }
        DbAction::Open => {
            if legacy.is_some() { create_database_file(&db_path, true)? } else { false }
        }
        // Never recreate a missing DB during a stale write/check.
        DbAction::Check(_) | DbAction::Save(_, _) => false,
    };
    let meta = std::fs::symlink_metadata(&db_path).map_err(|e| format!("Database proyek tidak ditemukan atau tidak dapat diperiksa: {e}"))?;
    if !meta.is_file() || meta.file_type().is_symlink() { return Err("Database proyek harus berupa file biasa, bukan symbolic link.".into()); }
    let mut db = Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|e| format!("Gagal membuka database proyek: {e}"))?;
    db.busy_timeout(Duration::from_secs(3)).map_err(|e| e.to_string())?;
    if !created {
        let app: i64 = db.query_row("PRAGMA application_id", [], |r| r.get(0)).map_err(|e| format!("Database proyek tidak valid: {e}"))?;
        let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(|e| format!("Database proyek tidak valid: {e}"))?;
        if app != PROJECT_APP_ID || version != 1 { return Err("Database proyek bukan milik NovelCraft atau versinya tidak didukung; file tidak diubah.".into()); }
    }
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e| format!("Gagal membuka transaksi proyek: {e}"))?;
    if created {
        tx.execute_batch(&format!("PRAGMA application_id={PROJECT_APP_ID}; PRAGMA user_version=1; CREATE TABLE project_index (id INTEGER PRIMARY KEY CHECK(id=1), identity BLOB NOT NULL, revision INTEGER NOT NULL, data TEXT NOT NULL, legacy BLOB);"))
            .map_err(|e| format!("Gagal menyiapkan database proyek: {e}"))?;
        let initial = match legacy {
            Some(manifest) => manifest,
            None => Manifest { version: 1, chapters: Vec::new() },
        };
        let data = serde_json::to_string(&initial).map_err(|e| format!("Gagal menyiapkan proyek: {e}"))?;
        tx.execute("INSERT INTO project_index(id,identity,revision,data,legacy) VALUES(1,randomblob(16),1,?1,?2)", params![data, original])
            .map_err(|e| format!("Gagal memigrasikan proyek: {e}"))?;
    }
    let (identity, mut revision, data, expected_legacy): (Vec<u8>, i64, String, Option<Vec<u8>>) =
        tx.query_row("SELECT identity,revision,data,legacy FROM project_index WHERE id=1", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))
            .map_err(|e| format!("Gagal membaca database proyek: {e}"))?;
    if identity.len() != 16 || revision < 1 { return Err("Identitas atau revisi proyek tidak valid.".into()); }
    if legacy_snapshot(&folder.join(MANIFEST))? != expected_legacy {
        return Err("Metadata JSON proyek berubah setelah migrasi; perubahan tidak digabung otomatis. Pulihkan atau bandingkan cadangan sebelum membuka ulang proyek.".into());
    }
    let mut manifest = parse_manifest(data.as_bytes())?;
    let mut token = identity;
    token.extend_from_slice(&revision.to_be_bytes()); token.extend_from_slice(data.as_bytes());
    let expected = match &action { DbAction::Check(expected) | DbAction::Save(expected, _) => Some(*expected), _ => None };
    if expected.is_some_and(|expected| token != expected) { return Err("Metadata proyek berubah di luar aplikasi; buka ulang proyek.".into()); }
    if let DbAction::Save(_, replacement) = action {
        validate(replacement)?;
        let data = serde_json::to_string(replacement).map_err(|e| format!("Gagal menyiapkan proyek: {e}"))?;
        revision = revision.checked_add(1).ok_or("Revisi proyek habis.")?;
        tx.execute("UPDATE project_index SET data=?1,revision=?2 WHERE id=1", params![data, revision])
            .map_err(|e| format!("Gagal menyimpan daftar proyek: {e}"))?;
        manifest = replacement.clone();
        token.truncate(16); token.extend_from_slice(&revision.to_be_bytes()); token.extend_from_slice(data.as_bytes());
    }
    tx.commit().map_err(|e| format!("Gagal menyimpan proyek: {e}"))?;
    Ok((manifest, token))
}

fn create_database_file(path: &Path, existing_ok: bool) -> Result<bool, String> {
    let mut options = std::fs::OpenOptions::new(); options.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
    match options.open(path) {
        Ok(file) => { drop(file); Ok(true) },
        Err(e) if existing_ok && e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(format!("Gagal membuat proyek; metadata yang sudah ada tidak ditimpa: {e}")),
    }
}

pub async fn open(folder: PathBuf) -> Result<Project, String> {
    let folder = tokio::fs::canonicalize(folder).await.map_err(|e| format!("Gagal membuka folder proyek: {e}"))?;
    let db_folder = folder.clone();
    let (manifest, snapshot) = tokio::task::spawn_blocking(move || database_operation(&db_folder, DbAction::Open)).await
        .map_err(|e| format!("Proses database proyek gagal: {e}"))??;
    Ok(Project { folder, manifest, snapshot })
}

pub async fn create(folder: PathBuf) -> Result<Project, String> {
    let folder = tokio::fs::canonicalize(folder).await.map_err(|e| format!("Gagal membuka folder proyek: {e}"))?;
    let db_folder = folder.clone();
    let (manifest, snapshot) = tokio::task::spawn_blocking(move || database_operation(&db_folder, DbAction::Create)).await
        .map_err(|e| format!("Proses database proyek gagal: {e}"))??;
    Ok(Project { folder, manifest, snapshot })
}

async fn check(project: &Project) -> Result<(), String> {
    let folder = project.folder.clone(); let snapshot = project.snapshot.clone();
    tokio::task::spawn_blocking(move || database_operation(&folder, DbAction::Check(&snapshot))).await
        .map_err(|e| format!("Proses database proyek gagal: {e}"))??;
    Ok(())
}

async fn save(mut project: Project) -> Result<Project, String> {
    let folder = project.folder.clone(); let snapshot = project.snapshot.clone(); let manifest = project.manifest.clone();
    let (_, token) = tokio::task::spawn_blocking(move || database_operation(&folder, DbAction::Save(&snapshot, &manifest))).await
        .map_err(|e| format!("Proses database proyek gagal: {e}"))??;
    project.snapshot = token;
    Ok(project)
}

pub async fn add_chapter(mut project: Project, title: String) -> Result<Project, String> {
    let title = title.trim().to_owned();
    validate_title(&title)?;
    check(&project).await?;
    // Create exclusively: an existing file, even one outside the index, is never overwritten.
    let mut number = 1_u64;
    let (name, mut file) = loop {
        let name = format!("chapter-{number}.md");
        if project.manifest.chapters.iter().any(|chapter| chapter.file == name) {
            number = number.checked_add(1).ok_or("Nomor chapter habis.")?;
            continue;
        }
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        match options.open(project.folder.join(&name)).await {
            Ok(file) => break (name, file),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                number = number.checked_add(1).ok_or("Nomor chapter habis.")?;
            }
            Err(e) => return Err(format!("Gagal membuat file chapter: {e}")),
        }
    };
    file.flush().await.map_err(|e| format!("Gagal menyiapkan {}; file kosong mungkin tersisa: {e}", name))?;
    file.sync_all().await.map_err(|e| format!("Gagal menyiapkan {}; file kosong mungkin tersisa: {e}", name))?;
    drop(file);
    project.manifest.chapters.push(Chapter { title, file: name.clone() });
    save(project).await.map_err(|e| format!("Gagal menyimpan daftar proyek; file {name} tetap ada tetapi belum masuk daftar: {e}"))
}

/// Index an existing regular Markdown file already in the project directory.
pub async fn attach_chapter(mut project: Project, path: PathBuf, title: String) -> Result<Project, String> {
    let title = title.trim().to_owned();
    validate_title(&title)?;
    let parent = path.parent().ok_or("Lokasi file tidak valid.")?;
    let parent = tokio::fs::canonicalize(parent).await.map_err(|e| format!("Gagal memeriksa folder: {e}"))?;
    if parent != project.folder { return Err("Pilih file Markdown yang berada langsung di folder proyek.".into()); }
    let name = path.file_name().and_then(|name| name.to_str()).ok_or("Nama file harus berupa UTF-8.")?;
    if !valid_filename(name) { return Err("File chapter harus berupa Markdown (.md atau .markdown) dengan nama file yang valid.".into()); }
    if project.manifest.chapters.iter().any(|chapter| chapter.file == name) {
        return Err("File ini sudah ada dalam daftar chapter.".into());
    }
    let meta = tokio::fs::symlink_metadata(&path).await.map_err(|e| format!("Gagal memeriksa file chapter: {e}"))?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err("File chapter harus berupa file biasa, bukan symbolic link.".into());
    }
    project.manifest.chapters.push(Chapter { title, file: name.to_owned() });
    save(project).await
}

#[derive(Debug, Clone)]
pub enum ChapterChange {
    Rename { index: usize, title: String },
    Move { index: usize, to: usize },
    Remove { index: usize },
}

/// Changes only the index; chapter filenames and contents are never changed.
pub async fn change_chapter(mut project: Project, change: ChapterChange) -> Result<Project, String> {
    match change {
        ChapterChange::Rename { index, title } => {
            let title = title.trim().to_owned();
            validate_title(&title)?;
            let chapter = project.manifest.chapters.get_mut(index).ok_or("Chapter tidak ditemukan.")?;
            chapter.title = title;
        }
        ChapterChange::Move { index, to } => {
            if index >= project.manifest.chapters.len() || to >= project.manifest.chapters.len() {
                return Err("Posisi chapter tidak valid.".into());
            }
            let chapter = project.manifest.chapters.remove(index);
            project.manifest.chapters.insert(to, chapter);
        }
        ChapterChange::Remove { index } => {
            if index >= project.manifest.chapters.len() { return Err("Chapter tidak ditemukan.".into()); }
            project.manifest.chapters.remove(index);
        }
    }
    save(project).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    fn temp_dir() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!("novelcraft-project-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)))
    }

    #[tokio::test]
    async fn chapters_are_separate_files_and_reopen_in_order() {
        let dir = temp_dir();
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("chapter-1.md"), "jangan timpa").await.unwrap();
        let project = create(dir.clone()).await.unwrap();
        assert!(create(dir.clone()).await.is_err());
        let project = add_chapter(project, " Prolog ".into()).await.unwrap();
        let project = add_chapter(project, "Bab dua".into()).await.unwrap();
        assert_eq!(tokio::fs::read_to_string(dir.join("chapter-1.md")).await.unwrap(), "jangan timpa");
        assert_eq!(tokio::fs::read_to_string(dir.join("chapter-2.md")).await.unwrap(), "");
        let reopened = open(dir.clone()).await.unwrap();
        assert_eq!(reopened.chapters().iter().map(|c| c.title.as_str()).collect::<Vec<_>>(), vec!["Prolog", "Bab dua"]);
        assert_eq!(reopened.chapter_path(0).unwrap(), project.chapter_path(0).unwrap());
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn missing_indexed_chapter_name_is_not_reused() {
        let dir = temp_dir();
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let project = add_chapter(create(dir.clone()).await.unwrap(), "Satu".into()).await.unwrap();
        tokio::fs::remove_file(project.chapter_path(0).unwrap()).await.unwrap();
        let project = add_chapter(project, "Dua".into()).await.unwrap();
        assert_eq!(project.chapter_path(1).unwrap().file_name().unwrap(), "chapter-2.md");
        assert!(!project.chapter_path(0).unwrap().exists());
        assert_eq!(open(dir.clone()).await.unwrap().chapters().len(), 2);
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn conflicting_or_invalid_metadata_is_not_overwritten() {
        let dir = temp_dir();
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let project = create(dir.clone()).await.unwrap();
        tokio::fs::write(dir.join(MANIFEST), b"invalid").await.unwrap();
        assert!(add_chapter(project, "Chapter".into()).await.is_err());
        assert!(open(dir.clone()).await.is_err());
        assert_eq!(tokio::fs::read(dir.join(MANIFEST)).await.unwrap(), b"invalid");
        assert!(!dir.join("chapter-1.md").exists());
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn rename_move_and_remove_only_change_the_index() {
        let dir = temp_dir();
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let project = create(dir.clone()).await.unwrap();
        let project = add_chapter(project, "Satu".into()).await.unwrap();
        let project = add_chapter(project, "Dua".into()).await.unwrap();
        let first = project.chapter_path(0).unwrap();
        let second = project.chapter_path(1).unwrap();
        tokio::fs::write(&first, "naskah satu").await.unwrap();
        tokio::fs::write(&second, "naskah dua").await.unwrap();
        let project = change_chapter(project, ChapterChange::Rename { index: 0, title: " Prolog ".into() }).await.unwrap();
        assert_eq!(project.chapters()[0].title, "Prolog");
        let project = change_chapter(project, ChapterChange::Move { index: 0, to: 1 }).await.unwrap();
        assert_eq!(project.chapter_path(1).unwrap(), first);
        let project = change_chapter(project, ChapterChange::Remove { index: 1 }).await.unwrap();
        assert_eq!(project.chapters().len(), 1);
        assert_eq!(project.chapter_path(0).unwrap(), second);
        assert_eq!(tokio::fs::read_to_string(&first).await.unwrap(), "naskah satu");
        assert_eq!(tokio::fs::read_to_string(&second).await.unwrap(), "naskah dua");
        let reopened = open(dir.clone()).await.unwrap();
        assert_eq!(reopened.chapters()[0].title, "Dua");
        assert_eq!(reopened.chapter_path(0).unwrap(), second);
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn changes_reject_invalid_input_and_stale_snapshots() {
        let dir = temp_dir();
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let project = add_chapter(create(dir.clone()).await.unwrap(), "Satu".into()).await.unwrap();
        let original = open(dir.clone()).await.unwrap();
        assert!(change_chapter(project.clone(), ChapterChange::Rename { index: 0, title: "  ".into() }).await.is_err());
        assert!(change_chapter(project.clone(), ChapterChange::Move { index: 0, to: 1 }).await.is_err());
        assert!(change_chapter(project.clone(), ChapterChange::Remove { index: 10 }).await.is_err());
        assert_eq!(open(dir.clone()).await.unwrap().snapshot, original.snapshot);
        tokio::fs::write(dir.join(MANIFEST), b"external").await.unwrap();
        for change in [ChapterChange::Rename { index: 0, title: "Dua".into() }, ChapterChange::Move { index: 0, to: 0 }, ChapterChange::Remove { index: 0 }] {
            assert!(change_chapter(project.clone(), change).await.is_err());
            assert_eq!(tokio::fs::read(dir.join(MANIFEST)).await.unwrap(), b"external");
        }
        assert!(project.chapter_path(0).unwrap().exists());
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn attach_existing_markdown_without_changing_file() {
        let dir = temp_dir();
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let file = dir.join("prolog.md");
        tokio::fs::write(&file, "naskah asli\r\n").await.unwrap();
        let project = create(dir.clone()).await.unwrap();
        let project = attach_chapter(project, file.clone(), "Prolog".into()).await.unwrap();
        assert_eq!(project.chapter_path(0).unwrap(), file);
        assert!(attach_chapter(project.clone(), file.clone(), "Duplikat".into()).await.is_err());
        assert_eq!(tokio::fs::read(&file).await.unwrap(), b"naskah asli\r\n");
        let project = change_chapter(project, ChapterChange::Remove { index: 0 }).await.unwrap();
        let project = attach_chapter(project, file.clone(), "Pulih".into()).await.unwrap();
        assert_eq!(open(dir.clone()).await.unwrap().chapters()[0].title, "Pulih");
        assert_eq!(project.chapter_path(0).unwrap(), file);
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn attach_rejects_other_directories_and_stale_index() {
        let dir = temp_dir();
        let other = temp_dir();
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::create_dir_all(&other).await.unwrap();
        let file = dir.join("ada.markdown");
        tokio::fs::write(&file, "original").await.unwrap();
        let external = other.join("luar.md");
        tokio::fs::write(&external, "luar").await.unwrap();
        let project = create(dir.clone()).await.unwrap();
        assert!(attach_chapter(project.clone(), external, "Luar".into()).await.is_err());
        tokio::fs::write(dir.join(MANIFEST), b"perubahan lain").await.unwrap();
        assert!(attach_chapter(project, file.clone(), "Ada".into()).await.is_err());
        assert_eq!(tokio::fs::read_to_string(file).await.unwrap(), "original");
        assert_eq!(tokio::fs::read(dir.join(MANIFEST)).await.unwrap(), b"perubahan lain");
        tokio::fs::remove_dir_all(dir).await.unwrap();
        tokio::fs::remove_dir_all(other).await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn attach_rejects_symbolic_link() {
        let dir = temp_dir();
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let target = dir.join("target.md");
        tokio::fs::write(&target, "original").await.unwrap();
        let link = dir.join("link.md");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let project = create(dir.clone()).await.unwrap();
        assert!(attach_chapter(project, link, "Tautan".into()).await.is_err());
        assert_eq!(tokio::fs::read_to_string(target).await.unwrap(), "original");
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[test]
    fn paths_titles_and_version_are_validated() {
        for name in ["../chapter-1.md", "chapter-a.txt", "chapter-1.md/../x", "chapter-1.md\\..\\x.md", "/absolute.md"] {
            let manifest = Manifest { version: 1, chapters: vec![Chapter { title: "x".into(), file: name.into() }] };
            assert!(validate(&manifest).is_err());
        }
        assert!(validate_title("  ").is_err());
        assert!(validate_title("nama\nberikut").is_err());
        assert!(validate(&Manifest { version: 2, chapters: vec![] }).is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn metadata_symlink_is_rejected() {
        let dir = temp_dir();
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let target = dir.join("target");
        tokio::fs::write(&target, b"{} ").await.unwrap();
        std::os::unix::fs::symlink(&target, dir.join(MANIFEST)).unwrap();
        assert!(open(dir.clone()).await.is_err());
        assert!(create(dir.clone()).await.is_err());
        assert_eq!(tokio::fs::read(target).await.unwrap(), b"{} ");
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn migrates_manifest_without_changing_chapters_or_reference_database() {
        let dir = temp_dir(); tokio::fs::create_dir_all(&dir).await.unwrap();
        let original = br#"{"version":1,"chapters":[{"title":"Awal","file":"awal.md"},{"title":"Akhir","file":"akhir.md"}]}"#;
        tokio::fs::write(dir.join(MANIFEST), original).await.unwrap();
        tokio::fs::write(dir.join("awal.md"), b"Awal\r\n").await.unwrap();
        tokio::fs::write(dir.join("akhir.md"), b"Akhir").await.unwrap();
        let db = crate::reference::connection(&dir).unwrap();
        db.execute("INSERT INTO entries(revision,kind,name,description,relations) VALUES(1,'character','A','B','C')", []).unwrap(); drop(db);
        let project = open(dir.clone()).await.unwrap();
        assert_eq!(project.chapters().iter().map(|c| c.title.as_str()).collect::<Vec<_>>(), vec!["Awal", "Akhir"]);
        let project = change_chapter(project, ChapterChange::Move { index: 0, to: 1 }).await.unwrap();
        assert_eq!(project.chapters()[0].title, "Akhir");
        assert_eq!(open(dir.clone()).await.unwrap().chapters()[1].title, "Awal");
        assert_eq!(tokio::fs::read(dir.join(MANIFEST)).await.unwrap(), original);
        assert_eq!(tokio::fs::read(dir.join("awal.md")).await.unwrap(), b"Awal\r\n");
        assert_eq!(tokio::fs::read(dir.join("akhir.md")).await.unwrap(), b"Akhir");
        let db = crate::reference::connection(&dir).unwrap();
        assert_eq!(db.query_row("SELECT name FROM entries", [], |r| r.get::<_, String>(0)).unwrap(), "A"); drop(db);
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn detects_old_version_edits_and_stale_updates_including_aba() {
        let dir = temp_dir(); tokio::fs::create_dir_all(&dir).await.unwrap();
        let original = br#"{"version":1,"chapters":[{"title":"Bab","file":"bab.md"}]}"#;
        tokio::fs::write(dir.join(MANIFEST), original).await.unwrap();
        let first = open(dir.clone()).await.unwrap(); let stale = first.clone();
        let changed = change_chapter(first, ChapterChange::Rename { index: 0, title: "Baru".into() }).await.unwrap();
        let _ = change_chapter(changed, ChapterChange::Rename { index: 0, title: "Bab".into() }).await.unwrap();
        assert!(change_chapter(stale, ChapterChange::Remove { index: 0 }).await.is_err());
        let current = open(dir.clone()).await.unwrap();
        tokio::fs::write(dir.join(MANIFEST), br#"{"version":1,"chapters":[]}"#).await.unwrap();
        assert!(open(dir.clone()).await.is_err());
        assert!(change_chapter(current.clone(), ChapterChange::Remove { index: 0 }).await.is_err());
        tokio::fs::remove_file(dir.join(MANIFEST)).await.unwrap(); assert!(open(dir.clone()).await.is_err());
        tokio::fs::write(dir.join(MANIFEST), original).await.unwrap(); assert_eq!(open(dir.clone()).await.unwrap().chapters().len(), 1);
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn stale_save_does_not_recreate_deleted_database() {
        let dir = temp_dir(); tokio::fs::create_dir_all(&dir).await.unwrap();
        let project = add_chapter(create(dir.clone()).await.unwrap(), "Bab".into()).await.unwrap();
        let file = project.chapter_path(0).unwrap(); tokio::fs::write(&file, "naskah").await.unwrap();
        tokio::fs::remove_file(dir.join(PROJECT_DB)).await.unwrap();
        assert!(change_chapter(project, ChapterChange::Remove { index: 0 }).await.is_err());
        assert!(!dir.join(PROJECT_DB).exists()); assert_eq!(tokio::fs::read(&file).await.unwrap(), b"naskah");
        assert!(open(dir.clone()).await.is_err()); tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn concurrent_edits_reject_one_stale_index() {
        let dir = temp_dir(); tokio::fs::create_dir_all(&dir).await.unwrap();
        let project = add_chapter(create(dir.clone()).await.unwrap(), "Bab".into()).await.unwrap();
        let a = project.clone(); let b = project;
        let first = tokio::spawn(async move { change_chapter(a, ChapterChange::Rename { index: 0, title: "Satu".into() }).await });
        let second = tokio::spawn(async move { change_chapter(b, ChapterChange::Rename { index: 0, title: "Dua".into() }).await });
        assert_ne!(first.await.unwrap().is_ok(), second.await.unwrap().is_ok());
        assert_eq!(open(dir.clone()).await.unwrap().chapters().len(), 1);
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn rejects_invalid_legacy_and_unrelated_database_without_overwriting() {
        let dir = temp_dir(); tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join(MANIFEST), br#"{"version":1,"chapters":[{"title":"X","file":"../x.md"}]}"#).await.unwrap();
        assert!(open(dir.clone()).await.is_err()); assert!(!dir.join(PROJECT_DB).exists());
        tokio::fs::remove_file(dir.join(MANIFEST)).await.unwrap();
        let db = Connection::open(dir.join(PROJECT_DB)).unwrap(); db.execute("CREATE TABLE other(value TEXT)", []).unwrap(); drop(db);
        let before = std::fs::read(dir.join(PROJECT_DB)).unwrap();
        assert!(open(dir.clone()).await.is_err()); assert!(create(dir.clone()).await.is_err());
        assert_eq!(std::fs::read(dir.join(PROJECT_DB)).unwrap(), before);
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_database_symlink_without_altering_target() {
        let dir = temp_dir(); tokio::fs::create_dir_all(&dir).await.unwrap();
        let target = dir.join("target"); tokio::fs::write(&target, b"unchanged").await.unwrap();
        std::os::unix::fs::symlink(&target, dir.join(PROJECT_DB)).unwrap();
        assert!(open(dir.clone()).await.is_err()); assert!(create(dir.clone()).await.is_err());
        assert_eq!(tokio::fs::read(target).await.unwrap(), b"unchanged"); tokio::fs::remove_dir_all(dir).await.unwrap();
    }

}
