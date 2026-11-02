use epub::doc::EpubDoc;
use std::path::Path;

/// Each section is a spine item; a section does not necessarily match a chapter.
pub fn read_epub(path: &Path) -> Result<Vec<String>, String> {
    let mut book = EpubDoc::new(path).map_err(|error| format!("Gagal membuka EPUB: {error}"))?;
    if book.spine.is_empty() {
        return Err("EPUB tidak memiliki bagian untuk dibaca.".into());
    }

    let mut sections = Vec::with_capacity(book.spine.len());
    loop {
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
    Ok(sections)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_sections_in_spine_order() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/basic.epub");
        let sections = read_epub(&path).unwrap();
        assert_eq!(sections.len(), 2);
        assert!(sections[0].contains("Bagian pertama"));
        assert!(sections[0].contains("Halo & selamat datang"));
        assert!(sections[1].contains("Bagian kedua"));
    }

    #[test]
    fn invalid_epub_returns_error() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        assert!(read_epub(&path).is_err());
    }
}
