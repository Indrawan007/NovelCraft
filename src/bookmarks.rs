use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::AsyncWriteExt;

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
    save_to(&storage_path()?, bookmark).await?;
    Ok(has_scroll)
}

pub async fn remove(path: PathBuf) -> Result<bool, String> {
    remove_from(&storage_path()?, &path).await
}

pub async fn load(path: PathBuf) -> Result<Option<Bookmark>, String> {
    Ok(load_all().await?.into_iter().find(|item| item.path == path))
}

pub async fn load_all() -> Result<Vec<Bookmark>, String> {
    read_from(&storage_path()?).await
}

async fn read_from(path: &Path) -> Result<Vec<Bookmark>, String> {
    let bytes = match tokio::fs::read(path).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("Gagal membaca penanda: {error}")),
    };
    parse_bookmarks(&bytes)
}

fn parse_bookmarks(bytes: &[u8]) -> Result<Vec<Bookmark>, String> {
    let bookmarks: Vec<Bookmark> = serde_json::from_slice(bytes)
        .map_err(|error| format!("Data penanda tidak valid; file asli tidak diubah: {error}"))?;
    if bookmarks.iter().any(|item| item.scroll_percent.is_some_and(|value| value > 100)) {
        return Err("Data penanda tidak valid; file asli tidak diubah.".into());
    }
    Ok(bookmarks)
}

// For writes, reject symlinks and non-files rather than replacing them on rename.
async fn read_write_snapshot(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput,
                "File penanda harus berupa file biasa, bukan symbolic link"));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    }
    tokio::fs::read(path).await.map(Some)
}

async fn read_for_write(path: &Path) -> Result<(Vec<Bookmark>, Option<Vec<u8>>), String> {
    let snapshot = read_write_snapshot(path).await
        .map_err(|error| format!("Gagal membaca penanda untuk ditulis; file asli tidak diubah: {error}"))?;
    let bookmarks = match &snapshot {
        Some(bytes) => parse_bookmarks(bytes)?,
        None => Vec::new(),
    };
    Ok((bookmarks, snapshot))
}

async fn save_to(path: &Path, bookmark: Bookmark) -> Result<(), String> {
    if bookmark.scroll_percent.is_some_and(|value| value > 100) {
        return Err("Posisi gulir tidak valid.".into());
    }
    // Never overwrite a malformed or unreadable bookmarks file.
    let (mut bookmarks, snapshot) = read_for_write(path).await?;
    bookmarks.retain(|item| item.path != bookmark.path);
    bookmarks.push(bookmark);
    write_to(path, &bookmarks, snapshot).await
}

async fn remove_from(storage: &Path, book_path: &Path) -> Result<bool, String> {
    let (mut bookmarks, snapshot) = read_for_write(storage).await?;
    let previous_len = bookmarks.len();
    bookmarks.retain(|item| item.path != book_path);
    if bookmarks.len() == previous_len {
        return Ok(false);
    }
    write_to(storage, &bookmarks, snapshot).await?;
    Ok(true)
}

async fn write_to(path: &Path, bookmarks: &[Bookmark], expected: Option<Vec<u8>>) -> Result<(), String> {
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    let bytes = serde_json::to_vec_pretty(bookmarks)
        .map_err(|error| format!("Gagal menyiapkan penanda: {error}"))?;
    let dir = path.parent().ok_or_else(|| "Lokasi penanda tidak valid.".to_owned())?;
    tokio::fs::create_dir_all(dir).await.map_err(|error| format!("Gagal membuat folder penanda: {error}"))?;
    let (temp, mut file) = loop {
        let temp = dir.join(format!(".bookmarks-{}-{}.tmp", std::process::id(), NEXT_TEMP.fetch_add(1, Ordering::Relaxed)));
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        match options.open(&temp).await {
            Ok(file) => break (temp, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("Gagal membuat file sementara penanda: {error}")),
        }
    };
    let result = async {
        file.write_all(&bytes).await?;
        file.sync_all().await?;
        drop(file);
        if read_write_snapshot(path).await? != expected {
            return Err(std::io::Error::new(std::io::ErrorKind::Other,
                "Penanda berubah saat disimpan; coba ulangi"));
        }
        tokio::fs::rename(&temp, path).await
    }.await;
    if let Err(error) = result {
        let _ = tokio::fs::remove_file(&temp).await;
        return Err(format!("Gagal menyimpan penanda: {error}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path() -> PathBuf {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("novelcraft-bookmarks-{}-{unique}", std::process::id())).join("bookmarks.json")
    }

    fn entry(path: &str, section: usize, scroll_percent: Option<u8>) -> Bookmark {
        Bookmark { path: path.into(), section, section_count: None, scroll_percent, markdown_source: false }
    }

    #[tokio::test]
    async fn stores_one_bookmark_per_path_and_preserves_other_books() {
        let path = temp_path();
        assert!(read_from(&path).await.unwrap().is_empty());
        save_to(&path, entry("/book.epub", 2, Some(45))).await.unwrap();
        save_to(&path, entry("/note.md", 0, None)).await.unwrap();
        save_to(&path, entry("/book.epub", 3, Some(90))).await.unwrap();
        let bookmarks = read_from(&path).await.unwrap();
        assert_eq!(bookmarks.len(), 2);
        assert!(bookmarks.contains(&entry("/book.epub", 3, Some(90))));
        assert!(bookmarks.contains(&entry("/note.md", 0, None)));
        tokio::fs::remove_dir_all(path.parent().unwrap()).await.unwrap();
    }

    #[tokio::test]
    async fn removes_only_the_requested_bookmark_without_touching_book_files() {
        let path = temp_path();
        let book = path.parent().unwrap().join("book.txt");
        assert!(!remove_from(&path, &book).await.unwrap());
        assert!(!path.exists());
        tokio::fs::create_dir_all(path.parent().unwrap()).await.unwrap();
        tokio::fs::write(&book, "isi buku").await.unwrap();
        let one = Bookmark { path: book.clone(), section: 0, section_count: None, scroll_percent: Some(10), markdown_source: false };
        let other = entry("/other.epub", 1, Some(70));
        save_to(&path, one).await.unwrap();
        save_to(&path, other.clone()).await.unwrap();
        assert!(remove_from(&path, &book).await.unwrap());
        assert_eq!(read_from(&path).await.unwrap(), vec![other]);
        assert_eq!(tokio::fs::read_to_string(&book).await.unwrap(), "isi buku");
        let before = tokio::fs::read(&path).await.unwrap();
        assert!(!remove_from(&path, &book).await.unwrap());
        assert_eq!(tokio::fs::read(&path).await.unwrap(), before);
        tokio::fs::remove_dir_all(path.parent().unwrap()).await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn writes_reject_bookmark_symlink_without_altering_target() {
        use std::os::unix::fs::symlink;
        let path = temp_path();
        tokio::fs::create_dir_all(path.parent().unwrap()).await.unwrap();
        let target = path.parent().unwrap().join("target.json");
        tokio::fs::write(&target, br#"[{"path":"/book.txt","section":0,"scroll_percent":10,"markdown_source":false}]"#).await.unwrap();
        symlink(&target, &path).unwrap();
        let before = tokio::fs::read(&target).await.unwrap();
        assert!(remove_from(&path, Path::new("/book.txt")).await.is_err());
        assert!(save_to(&path, entry("/other.txt", 0, Some(20))).await.is_err());
        assert_eq!(tokio::fs::read(&target).await.unwrap(), before);
        assert!(tokio::fs::symlink_metadata(&path).await.unwrap().file_type().is_symlink());
        tokio::fs::remove_dir_all(path.parent().unwrap()).await.unwrap();
    }

    #[tokio::test]
    async fn rejects_external_changes_and_cleans_temporary_files() {
        let path = temp_path();
        tokio::fs::create_dir_all(path.parent().unwrap()).await.unwrap();
        let original = br#"[{"path":"/old.txt","section":0,"scroll_percent":10,"markdown_source":false}]"#;
        tokio::fs::write(&path, original).await.unwrap();
        let (_, snapshot) = read_for_write(&path).await.unwrap();
        let changed = b"external change, even if invalid";
        tokio::fs::write(&path, changed).await.unwrap();
        let replacement = [entry("/new.txt", 0, Some(50))];
        assert!(write_to(&path, &replacement, snapshot).await.is_err());
        assert_eq!(tokio::fs::read(&path).await.unwrap(), changed.to_vec());
        let mut files = tokio::fs::read_dir(path.parent().unwrap()).await.unwrap();
        assert!(files.next_entry().await.unwrap().is_some());
        assert!(files.next_entry().await.unwrap().is_none());
        tokio::fs::remove_dir_all(path.parent().unwrap()).await.unwrap();
    }

    #[tokio::test]
    async fn detects_created_and_removed_bookmark_files() {
        let path = temp_path();
        let (_, absent) = read_for_write(&path).await.unwrap();
        tokio::fs::create_dir_all(path.parent().unwrap()).await.unwrap();
        tokio::fs::write(&path, b"[]").await.unwrap();
        let replacement = [entry("/new.txt", 0, None)];
        assert!(write_to(&path, &replacement, absent).await.is_err());
        assert_eq!(tokio::fs::read(&path).await.unwrap(), b"[]".to_vec());
        let (_, present) = read_for_write(&path).await.unwrap();
        tokio::fs::remove_file(&path).await.unwrap();
        assert!(write_to(&path, &replacement, present).await.is_err());
        assert!(!path.exists());
        tokio::fs::remove_dir_all(path.parent().unwrap()).await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_symlink_introduced_after_snapshot() {
        use std::os::unix::fs::symlink;
        let path = temp_path();
        tokio::fs::create_dir_all(path.parent().unwrap()).await.unwrap();
        tokio::fs::write(&path, b"[]").await.unwrap();
        let (_, snapshot) = read_for_write(&path).await.unwrap();
        let target = path.parent().unwrap().join("target.json");
        tokio::fs::write(&target, b"[]").await.unwrap();
        tokio::fs::remove_file(&path).await.unwrap();
        symlink(&target, &path).unwrap();
        assert!(write_to(&path, &[entry("/new.txt", 0, None)], snapshot).await.is_err());
        assert!(tokio::fs::symlink_metadata(&path).await.unwrap().file_type().is_symlink());
        assert_eq!(tokio::fs::read(&target).await.unwrap(), b"[]".to_vec());
        tokio::fs::remove_dir_all(path.parent().unwrap()).await.unwrap();
    }

    #[tokio::test]
    async fn writes_reject_directory_as_bookmarks_file() {
        let path = temp_path();
        tokio::fs::create_dir_all(&path).await.unwrap();
        assert!(save_to(&path, entry("/book.txt", 0, None)).await.is_err());
        assert!(remove_from(&path, Path::new("/book.txt")).await.is_err());
        assert!(tokio::fs::metadata(&path).await.unwrap().is_dir());
        tokio::fs::remove_dir_all(path.parent().unwrap()).await.unwrap();
    }

    #[tokio::test]
    async fn removal_rejects_corrupt_data_without_changing_it() {
        let path = temp_path();
        tokio::fs::create_dir_all(path.parent().unwrap()).await.unwrap();
        tokio::fs::write(&path, b"not json").await.unwrap();
        assert!(remove_from(&path, Path::new("/book.epub")).await.is_err());
        assert_eq!(tokio::fs::read(&path).await.unwrap(), b"not json".to_vec());
        tokio::fs::remove_dir_all(path.parent().unwrap()).await.unwrap();
    }

    #[tokio::test]
    async fn old_bookmarks_load_and_are_preserved_when_saving_new_data() {
        let path = temp_path();
        tokio::fs::create_dir_all(path.parent().unwrap()).await.unwrap();
        tokio::fs::write(&path, br#"[{"path":"/old.epub","section":1,"scroll_percent":50,"markdown_source":false}]"#).await.unwrap();
        let old = read_from(&path).await.unwrap().pop().unwrap();
        assert_eq!(old.section_count, None);
        assert_eq!(old.scroll_percent, Some(50));
        let mut new = entry("/new.epub", 1, Some(50));
        new.section_count = Some(3);
        save_to(&path, new.clone()).await.unwrap();
        let saved = read_from(&path).await.unwrap();
        assert!(saved.contains(&old));
        assert!(saved.contains(&new));
        tokio::fs::remove_dir_all(path.parent().unwrap()).await.unwrap();
    }

    #[tokio::test]
    async fn corrupt_data_is_not_overwritten() {
        let path = temp_path();
        tokio::fs::create_dir_all(path.parent().unwrap()).await.unwrap();
        tokio::fs::write(&path, b"not json").await.unwrap();
        assert!(save_to(&path, entry("/book.epub", 0, Some(0))).await.is_err());
        assert_eq!(tokio::fs::read(&path).await.unwrap(), b"not json".to_vec());
        tokio::fs::remove_dir_all(path.parent().unwrap()).await.unwrap();
    }

    #[tokio::test]
    async fn rejects_out_of_range_percent() {
        let path = temp_path();
        tokio::fs::create_dir_all(path.parent().unwrap()).await.unwrap();
        tokio::fs::write(&path, br#"[{"path":"/x","section":0,"scroll_percent":101,"markdown_source":false}]"#).await.unwrap();
        assert!(read_from(&path).await.is_err());
        tokio::fs::remove_dir_all(path.parent().unwrap()).await.unwrap();
    }
}
