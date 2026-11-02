use crate::ThemeChoice;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::AsyncWriteExt;

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

pub async fn load() -> Result<ThemeChoice, String> {
    let (_, theme, _) = read_from(&storage_path()?).await?;
    Ok(theme)
}

pub async fn save(theme: ThemeChoice) -> Result<ThemeChoice, String> {
    save_to(&storage_path()?, theme).await?;
    Ok(theme)
}

async fn read_snapshot(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput,
                "Settings harus berupa file biasa, bukan symbolic link"));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    }
    tokio::fs::read(path).await.map(Some)
}

async fn read_from(path: &Path) -> Result<(Map<String, Value>, ThemeChoice, Option<Vec<u8>>), String> {
    let snapshot = read_snapshot(path).await
        .map_err(|error| format!("Gagal membaca Settings; file asli tidak diubah: {error}"))?;
    let Some(bytes) = &snapshot else {
        return Ok((Map::new(), ThemeChoice::default(), None));
    };
    let map: Map<String, Value> = serde_json::from_slice(bytes)
        .map_err(|error| format!("Settings tidak valid; file asli tidak diubah: {error}"))?;
    let theme = match map.get("theme") {
        Some(value) => serde_json::from_value(value.clone())
            .map_err(|error| format!("Tema Settings tidak dikenal; file asli tidak diubah: {error}"))?,
        None => ThemeChoice::default(),
    };
    Ok((map, theme, snapshot))
}

async fn save_to(path: &Path, theme: ThemeChoice) -> Result<(), String> {
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    // Re-read and validate the existing file immediately before modifying it.
    let (mut map, _, expected) = read_from(path).await?;
    map.insert("theme".into(), serde_json::to_value(theme)
        .map_err(|error| format!("Gagal menyiapkan Settings: {error}"))?);
    let bytes = serde_json::to_vec_pretty(&map)
        .map_err(|error| format!("Gagal menyiapkan Settings: {error}"))?;
    let dir = path.parent().ok_or_else(|| "Lokasi Settings tidak valid.".to_owned())?;
    tokio::fs::create_dir_all(dir).await
        .map_err(|error| format!("Gagal membuat direktori Settings: {error}"))?;
    let (temp, mut file) = loop {
        let temp = dir.join(format!(".settings-{}-{}.tmp", std::process::id(), NEXT_TEMP.fetch_add(1, Ordering::Relaxed)));
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        match options.open(&temp).await {
            Ok(file) => break (temp, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("Gagal membuat file sementara Settings: {error}")),
        }
    };
    let result = async {
        file.write_all(&bytes).await?;
        file.sync_all().await?;
        drop(file);
        if read_snapshot(path).await? != expected {
            return Err(std::io::Error::new(std::io::ErrorKind::Other,
                "Settings berubah saat disimpan; penyimpanan dibatalkan"));
        }
        tokio::fs::rename(&temp, path).await
    }.await;
    if let Err(error) = result {
        let _ = tokio::fs::remove_file(&temp).await;
        return Err(format!("Gagal menyimpan Settings: {error}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path() -> PathBuf {
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("novelcraft-settings-{}-{unique}", std::process::id())).join("settings.json")
    }

    #[tokio::test]
    async fn saves_and_reloads_theme_and_preserves_unknown_fields() {
        let path = temp_path();
        assert_eq!(read_from(&path).await.unwrap().1, ThemeChoice::TokyoNight);
        save_to(&path, ThemeChoice::Nord).await.unwrap();
        assert_eq!(read_from(&path).await.unwrap().1, ThemeChoice::Nord);
        tokio::fs::write(&path, br#"{"theme":"dark","future_setting":{"value":1}}"#).await.unwrap();
        save_to(&path, ThemeChoice::Dracula).await.unwrap();
        let (map, theme, _) = read_from(&path).await.unwrap();
        assert_eq!(theme, ThemeChoice::Dracula);
        assert_eq!(map.get("future_setting"), Some(&serde_json::json!({"value": 1})));
        tokio::fs::remove_dir_all(path.parent().unwrap()).await.unwrap();
    }

    #[tokio::test]
    async fn rejects_invalid_and_unknown_settings_without_overwriting() {
        let path = temp_path();
        tokio::fs::create_dir_all(path.parent().unwrap()).await.unwrap();
        for bytes in [b"invalid".as_slice(), br#"{"theme":"unknown"}"#.as_slice(), b"[]".as_slice()] {
            tokio::fs::write(&path, bytes).await.unwrap();
            assert!(read_from(&path).await.is_err());
            assert!(save_to(&path, ThemeChoice::Nord).await.is_err());
            assert_eq!(tokio::fs::read(&path).await.unwrap(), bytes.to_vec());
        }
        tokio::fs::remove_dir_all(path.parent().unwrap()).await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_symlink_without_replacing_it_or_its_target() {
        use std::os::unix::fs::symlink;
        let path = temp_path();
        tokio::fs::create_dir_all(path.parent().unwrap()).await.unwrap();
        let target = path.parent().unwrap().join("other.json");
        tokio::fs::write(&target, b"{}").await.unwrap();
        symlink(&target, &path).unwrap();
        assert!(save_to(&path, ThemeChoice::Dark).await.is_err());
        assert!(tokio::fs::symlink_metadata(&path).await.unwrap().file_type().is_symlink());
        assert_eq!(tokio::fs::read(&target).await.unwrap(), b"{}".to_vec());
        tokio::fs::remove_dir_all(path.parent().unwrap()).await.unwrap();
    }
}
