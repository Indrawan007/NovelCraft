use crate::ThemeChoice;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::AsyncWriteExt;

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
    let (_, settings, _) = read_from(&storage_path()?).await?;
    Ok(settings)
}

pub async fn save(theme: ThemeChoice) -> Result<ThemeChoice, String> {
    save_to(&storage_path()?, SettingUpdate::Theme(theme)).await?;
    Ok(theme)
}

pub async fn save_reader_font_size(size: u16) -> Result<u16, String> {
    save_to(&storage_path()?, SettingUpdate::ReaderFontSize(size)).await?;
    Ok(size)
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

async fn read_from(path: &Path) -> Result<(Map<String, Value>, Settings, Option<Vec<u8>>), String> {
    let snapshot = read_snapshot(path).await
        .map_err(|error| format!("Gagal membaca Settings; file asli tidak diubah: {error}"))?;
    let Some(bytes) = &snapshot else {
        return Ok((Map::new(), Settings::default(), None));
    };
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
    Ok((map, Settings { theme, reader_font_size }, snapshot))
}

async fn save_to(path: &Path, update: SettingUpdate) -> Result<(), String> {
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    // Re-read and validate the existing file immediately before modifying it.
    let (mut map, _, expected) = read_from(path).await?;
    match update {
        SettingUpdate::Theme(theme) => {
            map.insert("theme".into(), serde_json::to_value(theme)
                .map_err(|error| format!("Gagal menyiapkan Settings: {error}"))?);
        }
        SettingUpdate::ReaderFontSize(size) => {
            if !(MIN_READER_FONT_SIZE..=MAX_READER_FONT_SIZE).contains(&size) {
                return Err("Ukuran font Reader di luar batas.".into());
            }
            map.insert("reader_font_size".into(), serde_json::json!(size));
        }
    }
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
        assert_eq!(read_from(&path).await.unwrap().1.theme, ThemeChoice::NovelCraft);
        save_to(&path, SettingUpdate::Theme(ThemeChoice::Nord)).await.unwrap();
        assert_eq!(read_from(&path).await.unwrap().1.theme, ThemeChoice::Nord);
        tokio::fs::write(&path, br#"{"theme":"dark","future_setting":{"value":1}}"#).await.unwrap();
        save_to(&path, SettingUpdate::Theme(ThemeChoice::Dracula)).await.unwrap();
        let (map, settings, _) = read_from(&path).await.unwrap();
        assert_eq!(settings.theme, ThemeChoice::Dracula);
        assert_eq!(map.get("future_setting"), Some(&serde_json::json!({"value": 1})));
        tokio::fs::remove_dir_all(path.parent().unwrap()).await.unwrap();
    }

    #[tokio::test]
    async fn font_size_defaults_round_trips_and_preserves_theme_and_unknown_fields() {
        let path = temp_path();
        assert_eq!(read_from(&path).await.unwrap().1.reader_font_size, DEFAULT_READER_FONT_SIZE);
        tokio::fs::create_dir_all(path.parent().unwrap()).await.unwrap();
        tokio::fs::write(&path, br#"{"theme":"nord","future_setting":42}"#).await.unwrap();
        assert_eq!(read_from(&path).await.unwrap().1.reader_font_size, DEFAULT_READER_FONT_SIZE);
        save_to(&path, SettingUpdate::ReaderFontSize(24)).await.unwrap();
        let (map, settings, _) = read_from(&path).await.unwrap();
        assert_eq!(settings, Settings { theme: ThemeChoice::Nord, reader_font_size: 24 });
        assert_eq!(map.get("future_setting"), Some(&serde_json::json!(42)));
        save_to(&path, SettingUpdate::Theme(ThemeChoice::Dark)).await.unwrap();
        assert_eq!(read_from(&path).await.unwrap().1.reader_font_size, 24);
        tokio::fs::remove_dir_all(path.parent().unwrap()).await.unwrap();
    }

    #[tokio::test]
    async fn mockup_theme_round_trips_and_old_theme_is_preserved() {
        let path = temp_path();
        tokio::fs::create_dir_all(path.parent().unwrap()).await.unwrap();
        tokio::fs::write(&path, br#"{"theme":"tokyo_night"}"#).await.unwrap();
        assert_eq!(read_from(&path).await.unwrap().1.theme, ThemeChoice::TokyoNight);
        save_to(&path, SettingUpdate::Theme(ThemeChoice::NovelCraft)).await.unwrap();
        assert_eq!(read_from(&path).await.unwrap().1.theme, ThemeChoice::NovelCraft);
        tokio::fs::remove_dir_all(path.parent().unwrap()).await.unwrap();
    }

    #[tokio::test]
    async fn invalid_font_sizes_are_rejected_without_overwriting() {
        let path = temp_path();
        tokio::fs::create_dir_all(path.parent().unwrap()).await.unwrap();
        for bytes in [br#"{"reader_font_size":11}"#.as_slice(), br#"{"reader_font_size":33}"#.as_slice(), br#"{"reader_font_size":"20"}"#.as_slice(), br#"{"reader_font_size":null}"#.as_slice()] {
            tokio::fs::write(&path, bytes).await.unwrap();
            assert!(read_from(&path).await.is_err());
            assert!(save_to(&path, SettingUpdate::ReaderFontSize(20)).await.is_err());
            assert!(save_to(&path, SettingUpdate::Theme(ThemeChoice::Dark)).await.is_err());
            assert_eq!(tokio::fs::read(&path).await.unwrap(), bytes.to_vec());
        }
        tokio::fs::write(&path, b"{}").await.unwrap();
        for size in [0, 11, 33, u16::MAX] {
            assert!(save_to(&path, SettingUpdate::ReaderFontSize(size)).await.is_err());
            assert_eq!(tokio::fs::read(&path).await.unwrap(), b"{}".to_vec());
        }
        tokio::fs::remove_dir_all(path.parent().unwrap()).await.unwrap();
    }

    #[tokio::test]
    async fn rejects_invalid_and_unknown_settings_without_overwriting() {
        let path = temp_path();
        tokio::fs::create_dir_all(path.parent().unwrap()).await.unwrap();
        for bytes in [b"invalid".as_slice(), br#"{"theme":"unknown"}"#.as_slice(), b"[]".as_slice()] {
            tokio::fs::write(&path, bytes).await.unwrap();
            assert!(read_from(&path).await.is_err());
            assert!(save_to(&path, SettingUpdate::Theme(ThemeChoice::Nord)).await.is_err());
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
        assert!(save_to(&path, SettingUpdate::Theme(ThemeChoice::Dark)).await.is_err());
        assert!(save_to(&path, SettingUpdate::ReaderFontSize(24)).await.is_err());
        assert!(tokio::fs::symlink_metadata(&path).await.unwrap().file_type().is_symlink());
        assert_eq!(tokio::fs::read(&target).await.unwrap(), b"{}".to_vec());
        tokio::fs::remove_dir_all(path.parent().unwrap()).await.unwrap();
    }
}
