use crate::ThemeChoice;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use rusqlite::{params, Connection, OpenFlags, TransactionBehavior};
use std::time::Duration;

pub(crate) const MIN_READER_FONT_SIZE: u16 = 12;
pub(crate) const MAX_READER_FONT_SIZE: u16 = 32;
pub(crate) const DEFAULT_READER_FONT_SIZE: u16 = 18;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    pub theme: ThemeChoice,
    pub reader_font_size: u16,
}

impl Default for Settings {
    fn default() -> Self {
        Self { theme: ThemeChoice::default(), reader_font_size: DEFAULT_READER_FONT_SIZE }
    }
}

#[derive(Debug, Clone, Copy)]
enum SettingUpdate {
    Theme(ThemeChoice),
    ReaderFontSize(u16),
}

fn storage_path() -> Result<PathBuf, String> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .map(|home| home.join(".config"))
        })
        .ok_or_else(|| "Tidak dapat menentukan direktori konfigurasi (HOME tidak tersedia).".to_owned())?;
    Ok(base.join("novelcraft/settings.json"))
}

pub async fn load() -> Result<Settings, String> {
    let path = storage_path()?;
    tokio::task::spawn_blocking(move || database_operation(&path, None)).await
        .map_err(|e| format!("Proses database Settings gagal: {e}"))?
}

pub async fn save(theme: ThemeChoice) -> Result<ThemeChoice, String> {
    let path = storage_path()?;
    tokio::task::spawn_blocking(move || database_operation(&path, Some(SettingUpdate::Theme(theme)))).await
        .map_err(|e| format!("Proses database Settings gagal: {e}"))??;
    Ok(theme)
}

pub async fn save_reader_font_size(size: u16) -> Result<u16, String> {
    let path = storage_path()?;
    tokio::task::spawn_blocking(move || database_operation(&path, Some(SettingUpdate::ReaderFontSize(size)))).await
        .map_err(|e| format!("Proses database Settings gagal: {e}"))??;
    Ok(size)
}

const DB_APP_ID: i64 = 1313030739;
const DB_FILE: &str = "settings.sqlite3";

fn legacy_snapshot(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() =>
            Err("Settings lama harus berupa file biasa, bukan symbolic link.".into()),
        Ok(_) => std::fs::read(path).map(Some).map_err(|e| format!("Gagal membaca Settings lama: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("Gagal membaca Settings lama: {e}")),
    }
}

// Keep the complete legacy map, including keys introduced by other versions.
// The exact original JSON is retained and checked on every access so old versions
// cannot silently overwrite new settings or have their changes silently ignored.
fn database_operation(path: &Path, update: Option<SettingUpdate>) -> Result<Settings, String> {
    if let Some(SettingUpdate::ReaderFontSize(size)) = update {
        if !(MIN_READER_FONT_SIZE..=MAX_READER_FONT_SIZE).contains(&size) { return Err("Ukuran font Reader di luar batas.".into()); }
    }
    let dir = path.parent().ok_or("Lokasi Settings tidak valid.")?;
    let original = legacy_snapshot(path)?;
    if let Some(bytes) = &original { parse_settings(bytes)?; }
    std::fs::create_dir_all(dir).map_err(|e| format!("Gagal membuat direktori Settings: {e}"))?;
    let db_path = dir.join(DB_FILE);
    let mut options = std::fs::OpenOptions::new(); options.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
    let created = match options.open(&db_path) {
        Ok(file) => { drop(file); true },
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(e) => return Err(format!("Gagal membuat database Settings: {e}")),
    };
    let meta = std::fs::symlink_metadata(&db_path).map_err(|e| format!("Gagal memeriksa database Settings: {e}"))?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err("Database Settings harus berupa file biasa, bukan symbolic link.".into());
    }
    let mut db = Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|e| format!("Gagal membuka database Settings: {e}"))?;
    db.busy_timeout(Duration::from_secs(3)).map_err(|e| e.to_string())?;
    if !created {
        let app: i64 = db.query_row("PRAGMA application_id", [], |r| r.get(0)).map_err(|e| format!("Database Settings tidak valid: {e}"))?;
        let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(|e| format!("Database Settings tidak valid: {e}"))?;
        if app != DB_APP_ID || version != 1 { return Err("Database Settings bukan milik NovelCraft atau versinya tidak didukung; file tidak diubah.".into()); }
    }
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e| format!("Gagal membuka transaksi Settings: {e}"))?;
    if created {
        tx.execute_batch(&format!("PRAGMA application_id={DB_APP_ID}; PRAGMA user_version=1; CREATE TABLE settings (id INTEGER PRIMARY KEY CHECK(id=1), data TEXT NOT NULL, legacy BLOB);"))
            .map_err(|e| format!("Gagal menyiapkan database Settings: {e}"))?;
        let initial = original.as_deref().unwrap_or(b"{}");
        let (map, _) = parse_settings(initial)?;
        let data = serde_json::to_string(&map).map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO settings(id,data,legacy) VALUES(1,?1,?2)", params![data, original])
            .map_err(|e| format!("Gagal memigrasikan Settings: {e}"))?;
    }
    let (data, expected): (String, Option<Vec<u8>>) = tx.query_row("SELECT data, legacy FROM settings WHERE id=1", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|e| format!("Gagal membaca database Settings: {e}"))?;
    if legacy_snapshot(path)? != expected {
        return Err("Settings JSON lama berubah setelah migrasi; perubahan tidak digabung otomatis. Pulihkan atau bandingkan cadangan sebelum melanjutkan.".into());
    }
    let (mut map, _) = parse_settings(data.as_bytes())?;
    if let Some(update) = update {
        match update {
            SettingUpdate::Theme(theme) => { map.insert("theme".into(), serde_json::to_value(theme).map_err(|e| e.to_string())?); }
            SettingUpdate::ReaderFontSize(size) => {
                if !(MIN_READER_FONT_SIZE..=MAX_READER_FONT_SIZE).contains(&size) { return Err("Ukuran font Reader di luar batas.".into()); }
                map.insert("reader_font_size".into(), serde_json::json!(size));
            }
        }
        let data = serde_json::to_string(&map).map_err(|e| e.to_string())?;
        tx.execute("UPDATE settings SET data=?1 WHERE id=1", [data]).map_err(|e| format!("Gagal menyimpan Settings: {e}"))?;
    }
    let data = serde_json::to_vec(&map).map_err(|e| e.to_string())?;
    let (_, settings) = parse_settings(&data)?;
    tx.commit().map_err(|e| format!("Gagal menyimpan Settings: {e}"))?;
    Ok(settings)
}

fn parse_settings(bytes: &[u8]) -> Result<(Map<String, Value>, Settings), String> {
    let map: Map<String, Value> = serde_json::from_slice(bytes)
        .map_err(|error| format!("Settings tidak valid; file asli tidak diubah: {error}"))?;
    let theme = match map.get("theme") {
        Some(value) => serde_json::from_value(value.clone())
            .map_err(|error| format!("Tema Settings tidak dikenal; file asli tidak diubah: {error}"))?,
        None => ThemeChoice::default(),
    };
    let reader_font_size = match map.get("reader_font_size") {
        Some(value) => {
            let size: u16 = serde_json::from_value(value.clone())
                .map_err(|error| format!("Ukuran font Reader tidak valid; file asli tidak diubah: {error}"))?;
            if !(MIN_READER_FONT_SIZE..=MAX_READER_FONT_SIZE).contains(&size) {
                return Err("Ukuran font Reader di luar batas; file asli tidak diubah.".into());
            }
            size
        }
        None => DEFAULT_READER_FONT_SIZE,
    };
    Ok((map, Settings { theme, reader_font_size }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_path() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!("novelcraft-settings-db-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))).join("settings.json")
    }
    fn clean(path: &Path) { std::fs::remove_dir_all(path.parent().unwrap()).unwrap(); }

    #[test]
    fn migrates_preserves_json_and_unknown_fields_and_reopens() {
        let path = temp_path(); std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = br#"{"theme":"nord","reader_font_size":24,"future_setting":{"value":1}}"#;
        std::fs::write(&path, original).unwrap();
        assert_eq!(database_operation(&path, None).unwrap(), Settings { theme: ThemeChoice::Nord, reader_font_size: 24 });
        database_operation(&path, Some(SettingUpdate::Theme(ThemeChoice::Dracula))).unwrap();
        database_operation(&path, Some(SettingUpdate::ReaderFontSize(30))).unwrap();
        assert_eq!(database_operation(&path, None).unwrap(), Settings { theme: ThemeChoice::Dracula, reader_font_size: 30 });
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let db = Connection::open(path.parent().unwrap().join(DB_FILE)).unwrap();
        let data: String = db.query_row("SELECT data FROM settings", [], |r| r.get(0)).unwrap();
        let (map, _) = parse_settings(data.as_bytes()).unwrap();
        assert_eq!(map.get("future_setting"), Some(&serde_json::json!({"value":1})));
        drop(db); clean(&path);
    }

    #[test]
    fn detects_old_version_changes_and_preserves_both_copies() {
        let path = temp_path(); std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = br#"{"theme":"nord"}"#; std::fs::write(&path, original).unwrap();
        database_operation(&path, Some(SettingUpdate::Theme(ThemeChoice::Dark))).unwrap();
        let changed = br#"{"theme":"dracula"}"#; std::fs::write(&path, changed).unwrap();
        assert!(database_operation(&path, None).is_err());
        assert!(database_operation(&path, Some(SettingUpdate::ReaderFontSize(25))).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), changed);
        std::fs::write(&path, original).unwrap();
        assert_eq!(database_operation(&path, None).unwrap().theme, ThemeChoice::Dark);
        clean(&path);
    }

    #[test]
    fn detects_legacy_creation_and_deletion() {
        let path = temp_path();
        database_operation(&path, None).unwrap();
        std::fs::write(&path, b"{}").unwrap();
        assert!(database_operation(&path, None).is_err());
        std::fs::remove_file(&path).unwrap();
        assert!(database_operation(&path, None).is_ok());
        clean(&path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap(); std::fs::write(&path, b"{}").unwrap();
        database_operation(&path, None).unwrap(); std::fs::remove_file(&path).unwrap();
        assert!(database_operation(&path, None).is_err()); clean(&path);
    }

    #[test]
    fn invalid_legacy_or_font_never_overwrites_or_creates_database() {
        let path = temp_path(); std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        for bytes in [b"invalid".as_slice(), br#"{"theme":"unknown"}"#, br#"{"reader_font_size":33}"#, b"[]"] {
            std::fs::write(&path, bytes).unwrap();
            assert!(database_operation(&path, None).is_err());
            assert!(database_operation(&path, Some(SettingUpdate::Theme(ThemeChoice::Dark))).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
            assert!(!path.parent().unwrap().join(DB_FILE).exists());
        }
        std::fs::write(&path, b"{}").unwrap();
        assert!(database_operation(&path, Some(SettingUpdate::ReaderFontSize(11))).is_err());
        assert!(!path.parent().unwrap().join(DB_FILE).exists()); clean(&path);
    }

    #[test]
    fn concurrent_updates_preserve_both_settings() {
        let path = temp_path(); database_operation(&path, None).unwrap();
        let p1 = path.clone(); let p2 = path.clone();
        let first = std::thread::spawn(move || database_operation(&p1, Some(SettingUpdate::Theme(ThemeChoice::Nord))));
        let second = std::thread::spawn(move || database_operation(&p2, Some(SettingUpdate::ReaderFontSize(26))));
        first.join().unwrap().unwrap(); second.join().unwrap().unwrap();
        assert_eq!(database_operation(&path, None).unwrap(), Settings { theme: ThemeChoice::Nord, reader_font_size: 26 });
        clean(&path);
    }

    #[test]
    fn refuses_unrelated_database_without_modifying_it() {
        let path = temp_path(); std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let db_path = path.parent().unwrap().join(DB_FILE);
        let db = Connection::open(&db_path).unwrap(); db.execute("CREATE TABLE other(value TEXT)", []).unwrap(); drop(db);
        let before = std::fs::read(&db_path).unwrap();
        assert!(database_operation(&path, None).is_err());
        assert_eq!(std::fs::read(&db_path).unwrap(), before); clean(&path);
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlinks_without_changing_targets() {
        use std::os::unix::fs::symlink;
        let path = temp_path(); std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let target = path.parent().unwrap().join("other.json"); std::fs::write(&target, b"{}").unwrap();
        symlink(&target, &path).unwrap(); assert!(database_operation(&path, None).is_err());
        std::fs::remove_file(&path).unwrap();
        let db_path = path.parent().unwrap().join(DB_FILE); symlink(&target, &db_path).unwrap();
        assert!(database_operation(&path, None).is_err()); assert_eq!(std::fs::read(&target).unwrap(), b"{}"); clean(&path);
    }
}
