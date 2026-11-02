use epub::doc::{EpubDoc, NavPoint};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocEntry {
    pub label: String,
    pub section: usize,
    pub depth: usize,
}

#[derive(Debug, Clone)]
pub struct EpubContent {
    pub sections: Vec<String>,
    pub toc: Vec<TocEntry>,
}

/// Each section is a spine item; a section does not necessarily match a chapter.
pub fn read_epub(path: &Path) -> Result<EpubContent, String> {
    let mut book = EpubDoc::new(path).map_err(|error| format!("Gagal membuka EPUB: {error}"))?;
    if book.spine.is_empty() {
        return Err("EPUB tidak memiliki bagian untuk dibaca.".into());
    }

    let mut sections = Vec::with_capacity(book.spine.len());
    let mut section_paths = Vec::with_capacity(book.spine.len());
    loop {
        section_paths.push(book.get_current_path());
        let (data, mime) = book
            .get_current()
            .ok_or_else(|| "Gagal membaca bagian EPUB; file mungkin rusak.".to_owned())?;
        let section = match mime.as_str() {
            "application/xhtml+xml" | "text/html" => {
                // Let Iced wrap the resulting text to the window width, rather than
                // inserting line breaks at a narrow terminal width.
                html2text::config::plain()
                    .string_from_read(data.as_slice(), 10_000)
                    .map_err(|error| format!("Gagal membaca isi EPUB: {error}"))?
            }
            _ => format!("[Bagian {mime} belum dapat ditampilkan sebagai teks.]"),
        };
        sections.push(section);
        if !book.go_next() {
            break;
        }
    }
    let toc = map_toc(&book.toc, &section_paths);
    Ok(EpubContent { sections, toc })
}

// NCX anchors point within an XHTML document. We can only navigate to the
// beginning of the matching spine item because the Reader displays plain text.
fn map_toc(points: &[NavPoint], section_paths: &[Option<PathBuf>]) -> Vec<TocEntry> {
    let mut entries = Vec::new();
    let mut pending: Vec<_> = points.iter().rev().map(|point| (point, 0_usize)).collect();
    while let Some((point, depth)) = pending.pop() {
        pending.extend(point.children.iter().rev().map(|child| (child, depth.saturating_add(1))));
        let Some(uri) = point.content.to_str() else { continue };
        // Ignore external and unsupported links instead of guessing a target.
        if uri.contains("://") || uri.contains('?') { continue; }
        let path = Path::new(uri.split('#').next().unwrap_or(""));
        if let Some(section) = section_paths.iter().position(|item| item.as_deref() == Some(path)) {
            let label = point.label.trim();
            if !label.is_empty() {
                entries.push(TocEntry { label: label.to_owned(), section, depth });
            }
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_sections_in_spine_order() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/basic.epub");
        let book = read_epub(&path).unwrap();
        assert!(book.toc.is_empty());
        let sections = book.sections;
        assert_eq!(sections.len(), 2);
        assert!(sections[0].contains("Bagian pertama"));
        assert!(sections[0].contains("Halo & selamat datang"));
        assert!(sections[1].contains("Bagian kedua"));
    }

    #[test]
    fn maps_nested_ncx_entries_to_spine_and_skips_unmatched_targets() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/toc.epub");
        let book = read_epub(&path).unwrap();
        assert_eq!(book.sections.len(), 2);
        assert_eq!(book.toc, vec![
            TocEntry { label: "Awal".into(), section: 0, depth: 0 },
            TocEntry { label: "Subbagian".into(), section: 0, depth: 1 },
            TocEntry { label: "Akhir".into(), section: 1, depth: 0 },
        ]);
    }

    #[test]
    fn maps_children_even_if_the_parent_is_unresolved() {
        let point = NavPoint {
            label: "Missing".into(), content: "OEBPS/missing.xhtml".into(),
            children: vec![NavPoint { label: "Child".into(), content: "OEBPS/first.xhtml#anchor".into(), children: vec![], play_order: Default::default() }],
            play_order: Default::default(),
        };
        assert_eq!(map_toc(&[point], &[Some("OEBPS/first.xhtml".into())]), vec![
            TocEntry { label: "Child".into(), section: 0, depth: 1 },
        ]);
    }

    #[test]
    fn invalid_epub_returns_error() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        assert!(read_epub(&path).is_err());
    }
}
