use serde::{Deserialize, Serialize};
use rusqlite::{params, Connection, OpenFlags, TransactionBehavior};
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bookmark {
    pub path: PathBuf,
    pub section: usize,
    #[serde(default)]
    pub section_count: Option<usize>,
    pub scroll_percent: Option<u8>,
    pub markdown_source: bool,
}

fn storage_path() -> Result<PathBuf, String> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .ok_or_else(|| "Tidak dapat menentukan lokasi data penanda baca (HOME tidak tersedia).".to_owned())?;
    Ok(base.join("novelcraft/bookmarks.json"))
}

pub async fn save(bookmark: Bookmark) -> Result<bool, String> {
    let has_scroll = bookmark.scroll_percent.is_some();
    let storage = storage_path()?;
    tokio::task::spawn_blocking(move || database_operation(&storage, Operation::Save(bookmark))).await
        .map_err(|e| format!("Proses database penanda gagal: {e}"))??;
    Ok(has_scroll)
}

pub async fn remove(path: PathBuf) -> Result<bool, String> {
    let storage = storage_path()?;
    tokio::task::spawn_blocking(move || database_operation(&storage, Operation::Remove(path))).await
        .map_err(|e| format!("Proses database penanda gagal: {e}"))?
        .map(|(_, removed)| removed)
}

pub async fn load(path: PathBuf) -> Result<Option<Bookmark>, String> {
    Ok(load_all().await?.into_iter().find(|item| item.path == path))
}

pub async fn load_all() -> Result<Vec<Bookmark>, String> {
    let storage = storage_path()?;
    tokio::task::spawn_blocking(move || database_operation(&storage, Operation::Load)).await
        .map_err(|e| format!("Proses database penanda gagal: {e}"))?
        .map(|(items, _)| items)
}

const DB_APP_ID: i64 = 1313030738;
const DB_FILE: &str = "bookmarks.sqlite3";

enum Operation { Load, Save(Bookmark), Remove(PathBuf) }

fn parse_bookmarks(bytes: &[u8]) -> Result<Vec<Bookmark>, String> {
    let bookmarks: Vec<Bookmark> = serde_json::from_slice(bytes)
        .map_err(|error| format!("Data penanda tidak valid; file asli tidak diubah: {error}"))?;
    if bookmarks.iter().any(|item| item.scroll_percent.is_some_and(|value| value > 100)) {
        return Err("Data penanda tidak valid; file asli tidak diubah.".into());
    }
    Ok(bookmarks)
}

fn legacy_snapshot(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() =>
            Err("Penanda JSON harus berupa file biasa, bukan symbolic link.".into()),
        Ok(_) => std::fs::read(path).map(Some).map_err(|e| format!("Gagal membaca penanda JSON: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("Gagal membaca penanda JSON: {e}")),
    }
}

// Keep the old file untouched and retain its exact snapshot to detect edits by older versions.
// Store the complete array to preserve unknown fields on entries from other versions.
fn database_operation(path: &Path, operation: Operation) -> Result<(Vec<Bookmark>, bool), String> {
    // Avoid exposing a partially initialized database to another task in this process.
    static INITIALIZATION: Mutex<()> = Mutex::new(());
    let _guard = INITIALIZATION.lock().map_err(|_| "Proses database penanda gagal; mulai ulang aplikasi.")?;
    if let Operation::Save(bookmark) = &operation {
        if bookmark.scroll_percent.is_some_and(|value| value > 100) { return Err("Posisi gulir tidak valid.".into()); }
        // Fail before creating a database for non-UTF-8 paths, as JSON cannot represent them.
        serde_json::to_value(bookmark).map_err(|e| format!("Gagal menyiapkan penanda: {e}"))?;
    }
    let dir = path.parent().ok_or("Lokasi penanda tidak valid.")?;
    let original = legacy_snapshot(path)?;
    if let Some(bytes) = &original { parse_bookmarks(bytes)?; }
    std::fs::create_dir_all(dir).map_err(|e| format!("Gagal membuat folder penanda: {e}"))?;
    let db_path = dir.join(DB_FILE);
    let mut options = std::fs::OpenOptions::new(); options.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
    let created = match options.open(&db_path) {
        Ok(file) => { drop(file); true },
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(e) => return Err(format!("Gagal membuat database penanda: {e}")),
    };
    let meta = std::fs::symlink_metadata(&db_path).map_err(|e| format!("Gagal memeriksa database penanda: {e}"))?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err("Database penanda harus berupa file biasa, bukan symbolic link.".into());
    }
    let mut db = Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|e| format!("Gagal membuka database penanda: {e}"))?;
    db.busy_timeout(Duration::from_secs(3)).map_err(|e| e.to_string())?;
    if !created {
        let app: i64 = db.query_row("PRAGMA application_id", [], |r| r.get(0)).map_err(|e| format!("Database penanda tidak valid: {e}"))?;
        let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(|e| format!("Database penanda tidak valid: {e}"))?;
        if app != DB_APP_ID || version != 1 { return Err("Database penanda bukan milik NovelCraft atau versinya tidak didukung; file tidak diubah.".into()); }
    }
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e| format!("Gagal membuka transaksi penanda: {e}"))?;
    if created {
        tx.execute_batch(&format!("PRAGMA application_id={DB_APP_ID}; PRAGMA user_version=1; CREATE TABLE bookmarks (id INTEGER PRIMARY KEY CHECK(id=1), data TEXT NOT NULL, legacy BLOB);"))
            .map_err(|e| format!("Gagal menyiapkan database penanda: {e}"))?;
        let initial = original.as_deref().unwrap_or(b"[]");
        let data = std::str::from_utf8(initial).map_err(|e| format!("Data penanda tidak valid: {e}"))?;
        tx.execute("INSERT INTO bookmarks(id,data,legacy) VALUES(1,?1,?2)", params![data, original])
            .map_err(|e| format!("Gagal memigrasikan penanda: {e}"))?;
    }
    let (data, expected): (String, Option<Vec<u8>>) = tx.query_row("SELECT data, legacy FROM bookmarks WHERE id=1", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|e| format!("Gagal membaca database penanda: {e}"))?;
    if legacy_snapshot(path)? != expected {
        return Err("Penanda JSON lama berubah setelah migrasi; perubahan tidak digabung otomatis. Pulihkan atau bandingkan cadangan sebelum melanjutkan.".into());
    }
    let mut bookmarks = parse_bookmarks(data.as_bytes())?;
    let mut values: Vec<serde_json::Value> = serde_json::from_str(&data).map_err(|e| format!("Data penanda tidak valid: {e}"))?;
    let mut removed = false;
    let changed = match operation {
        Operation::Load => false,
        Operation::Save(bookmark) => {
            let mut new = serde_json::to_value(&bookmark).map_err(|e| format!("Gagal menyiapkan penanda: {e}"))?;
            // Preserve extension fields from an existing entry while replacing known fields.
            if let Some(index) = bookmarks.iter().position(|item| item.path == bookmark.path) {
                if let (Some(old), Some(new_fields)) = (values[index].as_object(), new.as_object_mut()) {
                    for (key, value) in old { new_fields.entry(key.clone()).or_insert_with(|| value.clone()); }
                }
            }
            let mut i = 0;
            values.retain(|_| { let keep = bookmarks[i].path != bookmark.path; i += 1; keep });
            bookmarks.retain(|item| item.path != bookmark.path);
            values.push(new); bookmarks.push(bookmark);
            true
        }
        Operation::Remove(book_path) => {
            let mut i = 0;
            values.retain(|_| { let keep = bookmarks[i].path != book_path; i += 1; keep });
            let before = bookmarks.len(); bookmarks.retain(|item| item.path != book_path);
            removed = bookmarks.len() != before;
            removed
        }
    };
    if changed {
        let data = serde_json::to_string(&values).map_err(|e| format!("Gagal menyiapkan penanda: {e}"))?;
        tx.execute("UPDATE bookmarks SET data=?1 WHERE id=1", [data]).map_err(|e| format!("Gagal menyimpan penanda: {e}"))?;
    }
    tx.commit().map_err(|e| format!("Gagal menyimpan penanda: {e}"))?;
    Ok((bookmarks, removed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_path() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!("novelcraft-bookmarks-db-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))).join("bookmarks.json")
    }
    fn clean(path: &Path) { std::fs::remove_dir_all(path.parent().unwrap()).unwrap(); }
    fn entry(path: &str, section: usize, scroll_percent: Option<u8>) -> Bookmark {
        Bookmark { path: path.into(), section, section_count: None, scroll_percent, markdown_source: false }
    }
    fn load(path: &Path) -> Vec<Bookmark> { database_operation(path, Operation::Load).unwrap().0 }

    #[test]
    fn migrates_old_data_preserves_json_and_extra_fields() {
        let path = temp_path(); std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = br#"[{"path":"/old.epub","section":1,"scroll_percent":50,"markdown_source":false,"future":{"v":2}}]"#;
        std::fs::write(&path, original).unwrap();
        let old = load(&path).pop().unwrap(); assert_eq!(old.section_count, None);
        let mut updated = old.clone(); updated.section = 3; updated.section_count = Some(8);
        database_operation(&path, Operation::Save(updated.clone())).unwrap();
        let other = entry("/other.txt", 0, None); database_operation(&path, Operation::Save(other.clone())).unwrap();
        assert_eq!(load(&path), vec![updated, other]);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let db = Connection::open(path.parent().unwrap().join(DB_FILE)).unwrap();
        let data: String = db.query_row("SELECT data FROM bookmarks", [], |r| r.get(0)).unwrap();
        let values: Vec<serde_json::Value> = serde_json::from_str(&data).unwrap();
        assert_eq!(values[0]["future"], serde_json::json!({"v":2}));
        drop(db); clean(&path);
    }

    #[test]
    fn saves_one_per_path_and_removes_without_touching_book() {
        let path = temp_path(); std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let book = path.parent().unwrap().join("book.txt"); std::fs::write(&book, b"isi").unwrap();
        let first = Bookmark { path: book.clone(), ..entry("/unused", 0, Some(25)) };
        let other = entry("/other.epub", 1, Some(80));
        database_operation(&path, Operation::Save(first.clone())).unwrap();
        database_operation(&path, Operation::Save(other.clone())).unwrap();
        database_operation(&path, Operation::Save(first.clone())).unwrap();
        assert_eq!(load(&path), vec![other.clone(), first]);
        assert!(database_operation(&path, Operation::Remove(book.clone())).unwrap().1);
        assert!(!database_operation(&path, Operation::Remove(book.clone())).unwrap().1);
        assert_eq!(load(&path), vec![other]); assert_eq!(std::fs::read(book).unwrap(), b"isi"); clean(&path);
    }

    #[test]
    fn detects_legacy_change_creation_and_deletion() {
        let path = temp_path(); std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = br#"[{"path":"/old","section":0,"scroll_percent":1,"markdown_source":false}]"#;
        std::fs::write(&path, original).unwrap();
        database_operation(&path, Operation::Save(entry("/new", 1, None))).unwrap();
        std::fs::write(&path, b"[]").unwrap();
        assert!(database_operation(&path, Operation::Load).is_err());
        assert!(database_operation(&path, Operation::Remove(PathBuf::from("/new"))).is_err());
        std::fs::remove_file(&path).unwrap(); assert!(database_operation(&path, Operation::Load).is_err());
        std::fs::write(&path, original).unwrap(); assert_eq!(load(&path).len(), 2); clean(&path);
        assert!(database_operation(&path, Operation::Load).is_ok()); std::fs::write(&path, b"[]").unwrap();
        assert!(database_operation(&path, Operation::Load).is_err()); clean(&path);
    }

    #[test]
    fn rejects_corrupt_json_and_invalid_scroll_without_creating_database() {
        let path = temp_path(); std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        for bad in [b"not json".as_slice(), br#"[{"path":"/x","section":0,"scroll_percent":101,"markdown_source":false}]"#] {
            std::fs::write(&path, bad).unwrap();
            assert!(database_operation(&path, Operation::Load).is_err());
            assert!(database_operation(&path, Operation::Save(entry("/new", 0, None))).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bad);
            assert!(!path.parent().unwrap().join(DB_FILE).exists());
        }
        std::fs::remove_file(&path).unwrap();
        assert!(database_operation(&path, Operation::Save(entry("/new", 0, Some(101)))).is_err());
        assert!(!path.parent().unwrap().join(DB_FILE).exists()); clean(&path);
    }

    #[test]
    fn concurrent_saves_do_not_lose_other_entries() {
        let path = temp_path();
        let a = path.clone(); let b = path.clone();
        let first = std::thread::spawn(move || database_operation(&a, Operation::Save(entry("/a", 1, None))));
        let second = std::thread::spawn(move || database_operation(&b, Operation::Save(entry("/b", 2, None))));
        first.join().unwrap().unwrap(); second.join().unwrap().unwrap();
        let entries = load(&path); assert_eq!(entries.len(), 2);
        assert!(entries.contains(&entry("/a", 1, None))); assert!(entries.contains(&entry("/b", 2, None))); clean(&path);
    }

    #[test]
    fn refuses_unrelated_database_without_modifying_it() {
        let path = temp_path(); std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let db_path = path.parent().unwrap().join(DB_FILE);
        let db = Connection::open(&db_path).unwrap(); db.execute("CREATE TABLE other(value TEXT)", []).unwrap(); drop(db);
        let before = std::fs::read(&db_path).unwrap(); assert!(database_operation(&path, Operation::Load).is_err());
        assert_eq!(std::fs::read(&db_path).unwrap(), before); clean(&path);
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlinks_and_non_utf8_path() {
        use std::os::unix::{ffi::OsStringExt, fs::symlink};
        let path = temp_path(); std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let target = path.parent().unwrap().join("target"); std::fs::write(&target, b"[]").unwrap();
        symlink(&target, &path).unwrap(); assert!(database_operation(&path, Operation::Load).is_err());
        std::fs::remove_file(&path).unwrap(); symlink(&target, path.parent().unwrap().join(DB_FILE)).unwrap();
        assert!(database_operation(&path, Operation::Load).is_err()); assert_eq!(std::fs::read(&target).unwrap(), b"[]");
        std::fs::remove_file(path.parent().unwrap().join(DB_FILE)).unwrap();
        let invalid = Bookmark { path: PathBuf::from(std::ffi::OsString::from_vec(vec![0xff])), ..entry("/unused", 0, None) };
        assert!(database_operation(&path, Operation::Save(invalid)).is_err());
        assert!(!path.parent().unwrap().join(DB_FILE).exists()); clean(&path);
    }
}
