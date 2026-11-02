//! A project is an ordered index of ordinary Markdown files in one directory.
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

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
        let number = chapter.file.strip_prefix("chapter-").and_then(|s| s.strip_suffix(".md"));
        if !number.is_some_and(|s| !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit()))
            || !names.insert(&chapter.file) {
            return Err("Daftar file chapter tidak valid atau duplikat.".into());
        }
    }
    Ok(())
}

fn validate_title(title: &str) -> Result<(), String> {
    if title.trim().is_empty() || title.chars().count() > 150 || title.chars().any(char::is_control) {
        return Err("Judul chapter harus berisi 1–150 karakter tanpa karakter kontrol.".into());
    }
    Ok(())
}

async fn read_manifest(path: &Path) -> Result<Vec<u8>, String> {
    let meta = tokio::fs::symlink_metadata(path).await.map_err(|e| format!("Gagal membaca proyek: {e}"))?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err("Metadata proyek harus berupa file biasa, bukan symbolic link.".into());
    }
    tokio::fs::read(path).await.map_err(|e| format!("Gagal membaca proyek: {e}"))
}

pub async fn open(folder: PathBuf) -> Result<Project, String> {
    let folder = tokio::fs::canonicalize(folder).await.map_err(|e| format!("Gagal membuka folder proyek: {e}"))?;
    let snapshot = read_manifest(&folder.join(MANIFEST)).await?;
    let manifest: Manifest = serde_json::from_slice(&snapshot).map_err(|e| format!("Metadata proyek tidak valid: {e}"))?;
    validate(&manifest)?;
    Ok(Project { folder, manifest, snapshot })
}

pub async fn create(folder: PathBuf) -> Result<Project, String> {
    let folder = tokio::fs::canonicalize(folder).await.map_err(|e| format!("Gagal membuka folder proyek: {e}"))?;
    let manifest = Manifest { version: 1, chapters: Vec::new() };
    let snapshot = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
    let path = folder.join(MANIFEST);
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(&path).await.map_err(|e| format!("Gagal membuat proyek; metadata yang sudah ada tidak ditimpa: {e}"))?;
    file.write_all(&snapshot).await.map_err(|e| format!("Gagal menulis {}; file metadata mungkin belum lengkap: {e}", path.display()))?;
    file.sync_all().await.map_err(|e| format!("Gagal menyelesaikan {}; periksa file metadata: {e}", path.display()))?;
    Ok(Project { folder, manifest, snapshot })
}

pub async fn add_chapter(mut project: Project, title: String) -> Result<Project, String> {
    let title = title.trim().to_owned();
    validate_title(&title)?;
    let manifest_path = project.folder.join(MANIFEST);
    if read_manifest(&manifest_path).await? != project.snapshot {
        return Err("Metadata proyek berubah di luar aplikasi; buka ulang proyek.".into());
    }
    // Create exclusively: an existing file, even one outside the index, is never overwritten.
    let mut number = 1_u64;
    let (name, mut file) = loop {
        let name = format!("chapter-{number}.md");
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
    let bytes = serde_json::to_vec_pretty(&project.manifest).map_err(|e| e.to_string())?;
    // Reject symlinks and external changes before the atomic checked replacement.
    let current = read_manifest(&manifest_path).await
        .map_err(|e| format!("{e} File {name} dibuat tetapi belum masuk daftar."))?;
    if current != project.snapshot {
        return Err(format!("Metadata proyek berubah; chapter {name} dibuat tetapi belum masuk daftar. Buka ulang proyek."));
    }
    let expected = crate::writer::FileState::Present(project.snapshot.clone());
    crate::writer::atomic_write_regular_checked(&manifest_path, &bytes, &expected).await
        .map_err(|e| format!("Gagal menyimpan daftar proyek; file {name} tetap ada tetapi belum masuk daftar: {e}"))?;
    project.snapshot = bytes;
    Ok(project)
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

    #[test]
    fn paths_titles_and_version_are_validated() {
        for name in ["../chapter-1.md", "chapter-a.md", "chapter-1.md/../x", "chapter-.md"] {
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
}
