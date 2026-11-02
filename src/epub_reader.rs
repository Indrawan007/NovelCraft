use epub::doc::{EpubDoc, NavPoint};
use std::path::{Component, Path, PathBuf};

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
    // EPUB 3 nav takes precedence; malformed or unmappable nav falls back to NCX.
    // Inspect the manifest directly so version strings newer than 3.0 work too.
    let nav = {
        let mut resources = book.resources.iter().filter(|(_, resource)| {
            resource.properties.as_deref().is_some_and(|properties| properties.split_ascii_whitespace().any(|value| value == "nav"))
        });
        let first = resources.next().map(|(id, resource)| (id.clone(), resource.path.clone()));
        if resources.next().is_some() { None } else { first }
    };
    let nav_toc = nav.and_then(|(id, nav_path)| {
        let (data, mime) = book.get_resource(&id)?;
        if mime != "application/xhtml+xml" || data.len() > 2 * 1024 * 1024 { return None; }
        let xml = std::str::from_utf8(&data).ok()?;
        parse_nav(xml, &nav_path, &section_paths)
    }).unwrap_or_default();
    let toc = if nav_toc.is_empty() { map_toc(&book.toc, &section_paths) } else { nav_toc };
    Ok(EpubContent { sections, toc })
}

const XHTML_NS: &str = "http://www.w3.org/1999/xhtml";
const EPUB_NS: &str = "http://www.idpf.org/2007/ops";

// Resolve only local archive-relative paths; never follow external links.
fn resolve_nav_path(nav_path: &Path, href: &str) -> Option<PathBuf> {
    let href = href.split('#').next()?;
    if href.contains('?') || href.contains(':') || href.contains('\\') || href.starts_with('/') {
        return None;
    }
    let path = if href.is_empty() { nav_path.to_path_buf() } else { nav_path.parent()?.join(href) };
    normalize_archive_path(&path)
}

fn normalize_archive_path(path: &Path) -> Option<PathBuf> {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(segment) => result.push(segment),
            Component::CurDir => {}
            Component::ParentDir if result.pop() => {}
            _ => return None,
        }
    }
    Some(result)
}

fn parse_nav(xml: &str, nav_path: &Path, section_paths: &[Option<PathBuf>]) -> Option<Vec<TocEntry>> {
    let options = roxmltree::ParsingOptions { nodes_limit: 100_000, ..Default::default() };
    let doc = roxmltree::Document::parse_with_options(xml, options).ok()?;
    let nav = doc.descendants().find(|node| {
        node.has_tag_name((XHTML_NS, "nav"))
            && node.attribute((EPUB_NS, "type")).is_some_and(|kind| kind.split_ascii_whitespace().any(|value| value == "toc"))
    })?;
    let list = nav.children().find(|node| node.has_tag_name((XHTML_NS, "ol")))?;
    let mut pending: Vec<_> = list.children().filter(|node| node.has_tag_name((XHTML_NS, "li")))
        .rev().map(|node| (node, 0_usize)).collect();
    let paths: Vec<_> = section_paths.iter().map(|path| path.as_deref().and_then(normalize_archive_path)).collect();
    let mut entries = Vec::new();
    while let Some((item, depth)) = pending.pop() {
        if let Some(nested) = item.children().find(|node| node.has_tag_name((XHTML_NS, "ol"))) {
            pending.extend(nested.children().filter(|node| node.has_tag_name((XHTML_NS, "li")))
                .rev().map(|node| (node, depth.saturating_add(1))));
        }
        let Some(link) = item.children().find(|node| node.has_tag_name((XHTML_NS, "a"))) else { continue };
        let Some(target) = link.attribute("href").and_then(|href| resolve_nav_path(nav_path, href)) else { continue };
        let Some(section) = paths.iter().position(|path| path.as_ref() == Some(&target)) else { continue };
        let raw_label: String = link.descendants().filter(|node| node.is_text())
            .filter_map(|node| node.text()).collect();
        let label = raw_label.split_whitespace().collect::<Vec<_>>().join(" ");
        if !label.is_empty() {
            entries.push(TocEntry { label, section, depth });
        }
    }
    Some(entries)
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
    fn maps_epub3_nav_relative_paths_and_nested_entries() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/nav.epub");
        let book = read_epub(&path).unwrap();
        assert_eq!(book.sections.len(), 2);
        assert_eq!(book.toc, vec![
            TocEntry { label: "Bab Satu".into(), section: 0, depth: 0 },
            TocEntry { label: "Subbab".into(), section: 0, depth: 1 },
            TocEntry { label: "Bab Dua".into(), section: 1, depth: 1 },
        ]);
    }

    #[test]
    fn epub3_nav_takes_precedence_over_ncx() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/nav-and-ncx.epub");
        let book = read_epub(&path).unwrap();
        assert_eq!(book.toc.first().unwrap().label, "Bab Satu");
        assert_eq!(book.toc.len(), 3);
    }

    #[test]
    fn malformed_epub3_nav_falls_back_to_ncx() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/nav-fallback.epub");
        let book = read_epub(&path).unwrap();
        assert_eq!(book.toc.first().unwrap().label, "Awal");
        assert_eq!(book.toc.len(), 3);
    }

    #[test]
    fn nav_path_resolution_rejects_external_or_escaping_paths() {
        let nav = Path::new("OEBPS/nav/toc.xhtml");
        assert_eq!(resolve_nav_path(nav, "../first.xhtml#start"), Some(PathBuf::from("OEBPS/first.xhtml")));
        assert_eq!(resolve_nav_path(nav, "#top"), Some(nav.to_path_buf()));
        for href in ["https://example.org/file", "//example.org/a", "/absolute", "../../../outside", "../first.xhtml?x=1", "..\\first.xhtml"] {
            assert_eq!(resolve_nav_path(nav, href), None, "{href}");
        }
    }

    #[test]
    fn invalid_or_missing_nav_is_safely_ignored() {
        let paths = [Some(PathBuf::from("OEBPS/first.xhtml"))];
        assert!(parse_nav("not xml", Path::new("OEBPS/nav.xhtml"), &paths).is_none());
        assert!(parse_nav("<html xmlns='http://www.w3.org/1999/xhtml'/>", Path::new("OEBPS/nav.xhtml"), &paths).is_none());
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
