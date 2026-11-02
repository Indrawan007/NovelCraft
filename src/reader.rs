use crate::bookmarks::{self, Bookmark};
use iced::widget::{button, column, container, markdown, row, scrollable, text, text_input};
use iced::{Element, Length, Task, Theme};
use std::path::{Path, PathBuf};

use crate::settings::{DEFAULT_READER_FONT_SIZE, MAX_READER_FONT_SIZE as MAX_FONT_SIZE, MIN_READER_FONT_SIZE as MIN_FONT_SIZE};
const FONT_STEP: u16 = 2;
const SCROLL_ID: &str = "reader-content";

pub struct Reader {
    path: Option<PathBuf>,
    sections: Vec<String>,
    toc: Vec<crate::epub_reader::TocEntry>,
    show_toc: bool,
    section: usize,
    section_input: String,
    section_input_error: bool,
    search_input: String,
    search_notice: Option<String>,
    show_text_search: bool,
    text_search_line: Option<usize>,
    text_search_excerpt: Option<String>,
    is_epub: bool,
    is_markdown: bool,
    markdown_items: Vec<markdown::Item>,
    show_source: bool,
    link_notice: Option<String>,
    font_size: u16,
    scroll_percent: Option<u8>,
    scroll_revision: u64,
    pending_scroll: Option<u8>,
    // snap_to does not emit on_scroll; retain its approximate target for the
    // next page switch until a fresh measurement or layout change occurs.
    restored_scroll_fallback: Option<u8>,
    busy: bool,
    pending_remove_bookmark: bool,
    error: Option<String>,
    bookmark_notice: Option<String>,
}

impl Default for Reader {
    fn default() -> Self {
        Self {
            path: None,
            sections: Vec::new(),
            toc: Vec::new(),
            show_toc: false,
            section: 0,
            section_input: String::new(),
            section_input_error: false,
            search_input: String::new(),
            search_notice: None,
            show_text_search: false,
            text_search_line: None,
            text_search_excerpt: None,
            is_epub: false,
            is_markdown: false,
            markdown_items: Vec::new(),
            show_source: false,
            link_notice: None,
            font_size: DEFAULT_READER_FONT_SIZE,
            scroll_percent: None,
            scroll_revision: 0,
            pending_scroll: None,
            restored_scroll_fallback: None,
            busy: false,
            pending_remove_bookmark: false,
            error: None,
            bookmark_notice: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Document {
    sections: Vec<String>,
    toc: Vec<crate::epub_reader::TocEntry>,
    is_epub: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    Open,
    Opened(Result<Option<(PathBuf, Document)>, String>),
    OpenedWithBookmark(Result<(PathBuf, Document, Result<Option<Bookmark>, String>), String>),
    PreviousSection,
    NextSection,
    ToggleToc,
    SelectToc(usize),
    SectionInputChanged(String),
    JumpToSection,
    ToggleTextSearch,
    SearchInputChanged(String),
    SearchNext,
    SearchPrevious,
    ToggleSource,
    LinkClicked(markdown::Url),
    FontSmaller,
    FontLarger,
    Scrolled { revision: u64, percent: Option<u8> },
    SaveBookmark,
    BookmarkSaved(Result<bool, String>),
    RestoreBookmark,
    BookmarkLoaded(Result<Option<Bookmark>, String>),
    AskRemoveBookmark,
    CancelRemoveBookmark,
    ConfirmRemoveBookmark,
    BookmarkRemoved(Result<bool, String>),
}

impl Reader {
    pub fn is_busy(&self) -> bool {
        self.busy
    }

    fn controls_blocked(&self) -> bool {
        self.busy || self.pending_remove_bookmark
    }

    pub(crate) fn requested_font_size(&self, message: &Message) -> Option<u16> {
        if self.controls_blocked() { return None; }
        let size = match message {
            Message::FontSmaller => self.font_size.saturating_sub(FONT_STEP).max(MIN_FONT_SIZE),
            Message::FontLarger => self.font_size.saturating_add(FONT_STEP).min(MAX_FONT_SIZE),
            _ => return None,
        };
        (size != self.font_size).then_some(size)
    }

    pub(crate) fn apply_font_size(&mut self, size: u16) {
        if (MIN_FONT_SIZE..=MAX_FONT_SIZE).contains(&size) && size != self.font_size {
            self.font_size = size;
            self.invalidate_scroll_position();
        }
    }

    fn snapshot_bookmark(&self) -> Option<Bookmark> {
        Some(Bookmark {
            path: self.path.as_ref()?.clone(),
            section: self.section,
            section_count: self.is_epub.then_some(self.sections.len()),
            scroll_percent: self.scroll_percent,
            markdown_source: self.is_markdown && self.show_source,
        })
    }

    pub fn take_restore_task(&mut self) -> Task<Message> {
        match self.pending_scroll.take() {
            Some(percent) => {
                self.restored_scroll_fallback = Some(percent);
                scrollable::snap_to(
                    scrollable::Id::new(SCROLL_ID),
                    scrollable::RelativeOffset { x: 0.0, y: f32::from(percent) / 100.0 },
                )
            }
            None => Task::none(),
        }
    }

    pub fn invalidate_scroll_position(&mut self) {
        self.scroll_percent = None;
        self.restored_scroll_fallback = None;
        self.scroll_revision = self.scroll_revision.wrapping_add(1);
    }

    pub fn suspend_scroll_position(&mut self) {
        if self.path.is_some() && self.pending_scroll.is_none() {
            self.pending_scroll = self.scroll_percent.or(self.restored_scroll_fallback);
        }
        self.invalidate_scroll_position();
    }

    pub fn open_path(&mut self, path: PathBuf) -> Option<Task<Message>> {
        if self.controls_blocked() {
            return None;
        }
        self.busy = true;
        self.error = None;
        self.bookmark_notice = None;
        self.pending_scroll = None;
        self.invalidate_scroll_position();
        Some(Task::perform(
            async move {
                let document = read_document(&path).await?;
                Ok(Some((path, document)))
            },
            Message::Opened,
        ))
    }

    pub fn open_path_with_bookmark(&mut self, path: PathBuf) -> Option<Task<Message>> {
        if self.controls_blocked() {
            return None;
        }
        self.busy = true;
        self.error = None;
        self.bookmark_notice = None;
        self.pending_scroll = None;
        self.invalidate_scroll_position();
        Some(Task::perform(
            async move {
                let document = read_document(&path).await?;
                let bookmark = bookmarks::load(path.clone()).await;
                Ok((path, document, bookmark))
            },
            Message::OpenedWithBookmark,
        ))
    }

    fn set_document(&mut self, path: PathBuf, document: Document) {
        self.is_markdown = !document.is_epub && is_markdown_path(&path);
        self.markdown_items = if self.is_markdown {
            markdown::parse(document.sections.first().map(String::as_str).unwrap_or(""))
                .collect()
        } else {
            Vec::new()
        };
        self.show_source = false;
        self.link_notice = None;
        self.path = Some(path);
        self.pending_scroll = None;
        self.sections = document.sections;
        self.toc = document.toc;
        self.show_toc = false;
        self.section = 0;
        self.section_input.clear();
        self.section_input_error = false;
        self.search_input.clear();
        self.search_notice = None;
        self.show_text_search = false;
        self.text_search_line = None;
        self.text_search_excerpt = None;
        self.is_epub = document.is_epub;
        self.error = None;
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Open if !self.controls_blocked() => {
                self.busy = true;
                self.error = None;
                self.bookmark_notice = None;
                self.pending_scroll = None;
                self.invalidate_scroll_position();
                return Task::perform(open_file(), Message::Opened);
            }
            Message::Opened(result) => {
                self.busy = false;
                self.invalidate_scroll_position();
                match result {
                    Ok(Some((path, document))) => {
                        self.set_document(path, document);
                        return reset_scroll();
                    }
                    Ok(None) => {}
                    Err(error) => self.error = Some(error),
                }
            }
            Message::OpenedWithBookmark(result) => {
                self.busy = false;
                self.invalidate_scroll_position();
                match result {
                    Ok((path, document, bookmark)) => {
                        self.set_document(path, document);
                        let _ = self.update(Message::BookmarkLoaded(bookmark));
                        // If the bookmark is missing or invalid, start at the top.
                        // Keep this pending while Reader is hidden or a close dialog is open.
                        if self.pending_scroll.is_none() {
                            self.pending_scroll = Some(0);
                        }
                    }
                    Err(error) => self.error = Some(error),
                }
            }
            Message::PreviousSection if self.is_epub && !self.controls_blocked() && self.section > 0 => {
                self.pending_scroll = None;
                self.section -= 1;
                self.search_notice = None;
                self.invalidate_scroll_position();
                return reset_scroll();
            }
            Message::NextSection if self.is_epub && !self.controls_blocked() && self.section + 1 < self.sections.len() => {
                self.pending_scroll = None;
                self.section += 1;
                self.search_notice = None;
                self.invalidate_scroll_position();
                return reset_scroll();
            }
            Message::ToggleToc if self.is_epub && !self.toc.is_empty() && !self.controls_blocked() => {
                self.show_toc = !self.show_toc;
                self.invalidate_scroll_position();
            }
            Message::SelectToc(index) if self.is_epub && self.show_toc && !self.controls_blocked() => {
                if let Some(entry) = self.toc.get(index).filter(|entry| entry.section < self.sections.len()) {
                    self.section = entry.section;
                    self.pending_scroll = None;
                    self.show_toc = false;
                    self.search_notice = None;
                    self.invalidate_scroll_position();
                    return reset_scroll();
                }
            }
            Message::SectionInputChanged(value) if self.is_epub && !self.controls_blocked() => {
                self.section_input = value;
                self.section_input_error = false;
            }
            Message::JumpToSection if self.is_epub && !self.controls_blocked() => {
                if let Some(section) = parse_section_number(&self.section_input, self.sections.len()) {
                    self.pending_scroll = None;
                    self.section = section;
                    self.section_input_error = false;
                    self.search_notice = None;
                    self.invalidate_scroll_position();
                    return reset_scroll();
                }
                self.section_input_error = true;
            }
            Message::ToggleTextSearch if !self.is_epub && self.path.is_some() && !self.controls_blocked() => {
                self.show_text_search = !self.show_text_search;
                self.invalidate_scroll_position();
            }
            Message::SearchInputChanged(value) if (self.is_epub || self.path.is_some() && self.show_text_search) && !self.controls_blocked() => {
                self.search_input = value;
                self.search_notice = None;
                self.text_search_line = None;
                self.text_search_excerpt = None;
            }
            Message::SearchNext | Message::SearchPrevious if (self.is_epub || self.path.is_some() && self.show_text_search) && !self.controls_blocked() => {
                let query = self.search_input.trim();
                if query.is_empty() {
                    self.search_notice = Some("Masukkan teks yang ingin dicari.".into());
                } else if !self.is_epub {
                    if let Some(result) = find_text_line(
                        self.sections.first().map(String::as_str).unwrap_or(""),
                        query,
                        self.text_search_line,
                        matches!(message, Message::SearchNext),
                    ) {
                        self.text_search_line = Some(result.line);
                        self.search_notice = Some(format!("Baris {} · hasil {} dari {}", result.line + 1, result.rank, result.total));
                        self.text_search_excerpt = Some(result.excerpt);
                    } else {
                        self.text_search_line = None;
                        self.text_search_excerpt = None;
                        self.search_notice = Some("Teks tidak ditemukan di sumber file.".into());
                    }
                } else if let Some(section) = find_matching_section(
                    &self.sections, self.section, query, matches!(message, Message::SearchNext),
                ) {
                    self.pending_scroll = None;
                    self.section = section;
                    self.search_notice = Some(format!("Ditemukan di bagian {} (mulai dari atas).", section + 1));
                    self.invalidate_scroll_position();
                    return reset_scroll();
                } else {
                    self.search_notice = Some("Teks tidak ditemukan di bagian EPUB.".into());
                }
            }
            Message::ToggleSource if self.is_markdown && !self.controls_blocked() => {
                self.pending_scroll = None;
                self.show_source = !self.show_source;
                self.invalidate_scroll_position();
                self.link_notice = None;
                return reset_scroll();
            }
            Message::LinkClicked(url) if !self.controls_blocked() => {
                self.invalidate_scroll_position();
                self.link_notice = Some(format!("Tautan tidak dibuka otomatis: {url}"));
            }
            Message::SaveBookmark if !self.controls_blocked() && self.path.is_some() => {
                let bookmark = self.snapshot_bookmark().unwrap();
                self.busy = true;
                self.error = None;
                self.bookmark_notice = None;
                return Task::perform(bookmarks::save(bookmark), Message::BookmarkSaved);
            }
            Message::BookmarkSaved(result) => {
                self.busy = false;
                match result {
                    Ok(true) => self.bookmark_notice = Some("Posisi saat tombol ditekan telah disimpan.".into()),
                    Ok(false) => self.bookmark_notice = Some("Bagian disimpan, tetapi posisi gulir belum tersedia; pemulihan dimulai dari atas.".into()),
                    Err(error) => self.error = Some(error),
                }
            }
            Message::RestoreBookmark if !self.controls_blocked() && self.path.is_some() => {
                self.busy = true;
                self.error = None;
                self.bookmark_notice = None;
                let path = self.path.as_ref().unwrap().clone();
                return Task::perform(bookmarks::load(path), Message::BookmarkLoaded);
            }
            Message::BookmarkLoaded(result) => {
                self.busy = false;
                match result {
                    Ok(Some(bookmark)) if self.path.as_ref() == Some(&bookmark.path)
                        && bookmark.section < self.sections.len() => {
                        self.section = bookmark.section;
                        if self.is_epub { self.search_notice = None; }
                        self.show_source = self.is_markdown && bookmark.markdown_source;
                        self.pending_scroll = Some(bookmark.scroll_percent.unwrap_or(0));
                        self.invalidate_scroll_position();
                        self.bookmark_notice = Some(if bookmark.scroll_percent.is_some() {
                            "Posisi tersimpan dipulihkan secara perkiraan.".into()
                        } else {
                            "Bagian dipulihkan dari atas; posisi gulir sebelumnya tidak tersedia.".into()
                        });
                    }
                    Ok(Some(_)) => self.error = Some("Penanda tidak cocok dengan file yang sedang dibuka atau bagian sudah tidak ada.".into()),
                    Ok(None) => self.bookmark_notice = Some("Belum ada posisi tersimpan untuk file ini.".into()),
                    Err(error) => self.error = Some(error),
                }
            }
            Message::AskRemoveBookmark if !self.controls_blocked() && self.path.is_some() => {
                self.pending_remove_bookmark = true;
                self.error = None;
                self.bookmark_notice = None;
                self.invalidate_scroll_position();
            }
            Message::CancelRemoveBookmark if self.pending_remove_bookmark && !self.busy => {
                self.pending_remove_bookmark = false;
                self.invalidate_scroll_position();
            }
            Message::ConfirmRemoveBookmark if self.pending_remove_bookmark && !self.busy => {
                self.pending_remove_bookmark = false;
                if let Some(path) = self.path.clone() {
                    self.busy = true;
                    self.invalidate_scroll_position();
                    return Task::perform(bookmarks::remove(path), Message::BookmarkRemoved);
                }
            }
            Message::BookmarkRemoved(result) => {
                self.busy = false;
                self.invalidate_scroll_position();
                match result {
                    Ok(true) => self.bookmark_notice = Some("Penanda tersimpan telah dihapus; file buku tidak diubah.".into()),
                    Ok(false) => self.bookmark_notice = Some("Tidak ada penanda tersimpan untuk file ini.".into()),
                    Err(error) => self.error = Some(error),
                }
            }
            Message::Scrolled { revision, percent }
                if self.path.is_some() && !self.pending_remove_bookmark && revision == self.scroll_revision =>
            {
                self.scroll_percent = percent;
                self.restored_scroll_fallback = None;
            }
            _ => {}
        }
        Task::none()
    }

    pub fn view<'a>(&'a self, theme: &Theme, font_settings_ready: bool, font_status: &'static str) -> Element<'a, Message> {
        let mut open = button("Buka file");
        if !self.controls_blocked() {
            open = open.on_press(Message::Open);
        }
        let mut save_bookmark = button("Simpan posisi");
        let mut restore_bookmark = button("Pulihkan posisi");
        let mut remove_bookmark = button("Hapus penanda");
        if !self.controls_blocked() && self.path.is_some() {
            save_bookmark = save_bookmark.on_press(Message::SaveBookmark);
            restore_bookmark = restore_bookmark.on_press(Message::RestoreBookmark);
            remove_bookmark = remove_bookmark.on_press(Message::AskRemoveBookmark);
        }
        let mut smaller = button("A−");
        if font_settings_ready && !self.controls_blocked() && self.font_size > MIN_FONT_SIZE {
            smaller = smaller.on_press(Message::FontSmaller);
        }
        let mut larger = button("A+");
        if font_settings_ready && !self.controls_blocked() && self.font_size < MAX_FONT_SIZE {
            larger = larger.on_press(Message::FontLarger);
        }

        let filename = self
            .path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "Belum ada file yang dibuka".to_owned());
        let mut view = column![
            text("Reader").size(32),
            row![open, smaller, text(self.font_size.to_string()), larger].spacing(12),
            container(text(font_status).size(12)).height(24),
            text(filename).size(14),
            row![save_bookmark, restore_bookmark, remove_bookmark].spacing(12),
        ]
        .spacing(12)
        .height(Length::Fill);

        if self.pending_remove_bookmark {
            view = view.push(text("Hapus penanda tersimpan untuk file ini? File buku tidak akan dihapus."));
            view = view.push(row![
                button("Hapus penanda").on_press(Message::ConfirmRemoveBookmark),
                button("Batal").on_press(Message::CancelRemoveBookmark),
            ].spacing(12));
        }
        if self.busy {
            view = view.push(text("Memproses file atau penanda..."));
        }
        if let Some(error) = &self.error {
            view = view.push(text(error));
        }
        if let Some(notice) = &self.bookmark_notice {
            view = view.push(text(notice).size(14));
        }
        if self.path.is_none() {
            view = view.push(text("Pilih file EPUB, TXT, atau Markdown untuk mulai membaca."));
        }
        if self.is_markdown {
            let label = if self.show_source { "Lihat tampilan" } else { "Lihat sumber" };
            let mut toggle = button(label);
            if !self.controls_blocked() {
                toggle = toggle.on_press(Message::ToggleSource);
            }
            view = view.push(toggle);
        }
        if let Some(notice) = &self.link_notice {
            view = view.push(text(notice).size(14));
        }
        if self.is_epub {
            let mut previous = button("← Sebelumnya");
            let mut next = button("Berikutnya →");
            if !self.controls_blocked() && self.section > 0 {
                previous = previous.on_press(Message::PreviousSection);
            }
            if !self.controls_blocked() && self.section + 1 < self.sections.len() {
                next = next.on_press(Message::NextSection);
            }
            view = view.push(
                row![previous, text(format!("Bagian {} / {}", self.section + 1, self.sections.len())), next]
                    .spacing(12),
            );
            view = view.push(text("Ctrl+PageUp / Ctrl+PageDown: pindah bagian").size(12));
            if self.toc.is_empty() {
                view = view.push(text("Daftar isi tidak tersedia; gunakan nomor bagian.").size(12));
            } else {
                let label = if self.show_toc { "Tutup daftar isi" } else { "Daftar isi" };
                let mut toggle_toc = button(label);
                if !self.controls_blocked() {
                    toggle_toc = toggle_toc.on_press(Message::ToggleToc);
                }
                view = view.push(toggle_toc);
                if self.show_toc {
                    let mut entries = column![text("Tujuan dibuka dari awal bagian, bukan posisi jangkar.").size(12)].spacing(4);
                    for (index, entry) in self.toc.iter().enumerate() {
                        let label = format!("{}{} · bagian {}", "  ".repeat(entry.depth.min(4)), entry.label, entry.section + 1);
                        let mut item = button(text(label)).width(Length::Fill);
                        if !self.controls_blocked() {
                            item = item.on_press(Message::SelectToc(index));
                        }
                        entries = entries.push(item);
                    }
                    view = view.push(scrollable(entries).height(160).width(Length::Fill));
                }
            }
            let mut input = text_input("Nomor", &self.section_input).width(100);
            let mut jump = button("Pergi");
            if !self.controls_blocked() {
                input = input.on_input(Message::SectionInputChanged).on_submit(Message::JumpToSection);
                jump = jump.on_press(Message::JumpToSection);
            }
            view = view.push(row![text("Ke bagian:"), input, jump].spacing(12));
            // Reserve the message height so invalid input does not resize the reading viewport.
            let section_error = if self.section_input_error {
                format!("Masukkan nomor bagian dari 1 sampai {}.", self.sections.len())
            } else {
                String::new()
            };
            view = view.push(container(text(section_error).size(14)).height(24));
        }
        if !self.is_epub && self.path.is_some() {
            let mut toggle_search = button(if self.show_text_search { "Tutup pencarian" } else { "Cari dalam file" });
            if !self.controls_blocked() { toggle_search = toggle_search.on_press(Message::ToggleTextSearch); }
            view = view.push(toggle_search);
        }
        if self.is_epub || self.path.is_some() && self.show_text_search {
            let placeholder = if self.is_epub { "Cari teks di bagian EPUB" } else { "Cari di sumber TXT / Markdown" };
            let mut search = text_input(placeholder, &self.search_input).width(Length::Fill);
            let mut previous_match = button("← Cari");
            let mut next_match = button("Cari →");
            if !self.controls_blocked() {
                search = search.on_input(Message::SearchInputChanged).on_submit(Message::SearchNext);
                previous_match = previous_match.on_press(Message::SearchPrevious);
                next_match = next_match.on_press(Message::SearchNext);
            }
            view = view.push(row![search, previous_match, next_match].spacing(12));
            view = view.push(container(text(self.search_notice.as_deref().unwrap_or("")).size(14)).height(24));
            if !self.is_epub {
                let excerpt = self.text_search_excerpt.as_deref().unwrap_or("");
                view = view.push(scrollable(text(excerpt).size(14)).height(72).width(Length::Fill));
                view = view.push(text("Mencari per baris di sumber; cuplikan tidak menggeser atau menyorot dokumen.").size(12));
            }
        }

        let contents = self.sections.get(self.section).map(String::as_str).unwrap_or("");
        let content: Element<'_, Message> = if self.is_markdown && !self.show_source {
            markdown::view(
                &self.markdown_items,
                markdown::Settings::with_text_size(self.font_size),
                markdown::Style::from_palette(theme.palette()),
            )
            .map(Message::LinkClicked)
        } else {
            text(contents).size(self.font_size).into()
        };
        let revision = self.scroll_revision;
        view = view.push(
            scrollable(container(content).width(Length::Fill))
                .id(scrollable::Id::new(SCROLL_ID))
                .on_scroll(move |viewport| {
                    Message::Scrolled {
                        revision,
                        percent: scroll_percentage(
                            viewport.absolute_offset().y,
                            viewport.bounds().height,
                            viewport.content_bounds().height,
                        ),
                    }
                })
                .height(Length::Fill)
                .width(Length::Fill),
        );
        if self.path.is_some() {
            let position = self.scroll_percent
                .map(|percent| format!("{percent}%"))
                .unwrap_or_else(|| "—".to_owned());
            let label = if self.is_epub { "Posisi gulir bagian" } else { "Posisi gulir" };
            view = view.push(text(format!("{label}: {position}")).size(14));
        }
        view.into()
    }
}

// Percentage of the scrollable distance, not a measured reading completion.
// Iced does not emit viewport events when the content fits without scrolling.
fn scroll_percentage(offset: f32, viewport_height: f32, content_height: f32) -> Option<u8> {
    if !offset.is_finite() || !viewport_height.is_finite() || !content_height.is_finite()
        || viewport_height <= 0.0 || content_height <= viewport_height
    {
        return None;
    }
    let fraction = (offset / (content_height - viewport_height)).clamp(0.0, 1.0);
    Some((fraction * 100.0).round() as u8)
}

fn is_markdown_path(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| {
        ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown")
    })
}

// Searches the extracted text of whole spine sections, starting after the current one.
fn find_matching_section(sections: &[String], current: usize, query: &str, forward: bool) -> Option<usize> {
    if sections.is_empty() || current >= sections.len() || query.trim().is_empty() {
        return None;
    }
    let query = query.trim().to_lowercase();
    let mut index = current;
    for _ in 0..sections.len() {
        index = if forward {
            if index + 1 == sections.len() { 0 } else { index + 1 }
        } else if index == 0 {
            sections.len() - 1
        } else {
            index - 1
        };
        if sections[index].to_lowercase().contains(&query) {
            return Some(index);
        }
    }
    None
}

struct TextLineMatch {
    line: usize,
    rank: usize,
    total: usize,
    excerpt: String,
}

// Search lines without collecting a potentially huge list of matches. Unicode
// lowercase is applied per character so offsets can be mapped back to UTF-8.
fn lower_chars(text: &str) -> String {
    text.chars().flat_map(char::to_lowercase).collect()
}

fn find_text_line(text: &str, query: &str, current: Option<usize>, forward: bool) -> Option<TextLineMatch> {
    let query = lower_chars(query.trim());
    if query.is_empty() || query.contains(['\n', '\r']) { return None; }
    let mut total = 0;
    let mut first = None;
    let mut last = None;
    let mut before = None;
    let mut after = None;
    for (index, line) in text.lines().enumerate() {
        if lower_chars(line).contains(&query) {
            total += 1;
            let hit = (index, total);
            first.get_or_insert(hit);
            last = Some(hit);
            if current.is_some_and(|pos| index < pos) { before = Some(hit); }
            if current.is_some_and(|pos| index > pos) && after.is_none() { after = Some(hit); }
        }
    }
    let (line, rank) = if forward { after.or(first) } else { before.or(last) }?;
    let source = text.lines().nth(line)?;
    let lower_offset = lower_chars(source).find(&query)?;
    let mut lower_bytes = 0;
    let mut match_start = 0;
    for (index, ch) in source.char_indices() {
        let next = lower_bytes + ch.to_lowercase().map(char::len_utf8).sum::<usize>();
        if lower_offset < next { match_start = index; break; }
        lower_bytes = next;
    }
    let start = source[..match_start].char_indices().rev().nth(39).map(|(i, _)| i).unwrap_or(0);
    let end = source[match_start..].char_indices().nth(80).map(|(i, _)| match_start + i).unwrap_or(source.len());
    let excerpt: String = source[start..end].chars().map(|ch| if ch.is_control() { ' ' } else { ch }).collect();
    Some(TextLineMatch {
        line, rank, total,
        excerpt: format!("{}{}{}", if start > 0 { "…" } else { "" }, excerpt, if end < source.len() { "…" } else { "" }),
    })
}

fn parse_section_number(input: &str, section_count: usize) -> Option<usize> {
    if input.is_empty() || !input.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let index = input.parse::<usize>().ok()?.checked_sub(1)?;
    (index < section_count).then_some(index)
}

fn reset_scroll() -> Task<Message> {
    scrollable::snap_to(
        scrollable::Id::new(SCROLL_ID),
        scrollable::RelativeOffset { x: 0.0, y: 0.0 },
    )
}

async fn open_file() -> Result<Option<(PathBuf, Document)>, String> {
    let Some(file) = rfd::AsyncFileDialog::new()
        .add_filter("Buku dan teks", &["epub", "txt", "md", "markdown"])
        .pick_file()
        .await
    else {
        return Ok(None);
    };
    let path = file.path().to_owned();
    let document = read_document(&path).await?;
    Ok(Some((path, document)))
}

async fn read_document(path: &Path) -> Result<Document, String> {
    let is_epub = path.extension().and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("epub"));
    let (sections, toc) = if is_epub {
        let path = path.to_owned();
        let book = tokio::task::spawn_blocking(move || crate::epub_reader::read_epub(&path))
            .await
            .map_err(|error| format!("Gagal memproses EPUB: {error}"))??;
        (book.sections, book.toc)
    } else {
        (vec![read_text(path).await?], Vec::new())
    };
    Ok(Document { sections, toc, is_epub })
}

async fn read_text(path: &Path) -> Result<String, String> {
    tokio::fs::read_to_string(path)
        .await
        .map_err(|error| format!("Gagal membuka file sebagai teks UTF-8: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deleting_bookmark_requires_confirmation_and_cancel_preserves_document() {
        let mut reader = Reader { path: Some("book.epub".into()), sections: vec!["satu".into(), "dua".into()], section: 1, is_epub: true, ..Reader::default() };
        let _ = reader.update(Message::AskRemoveBookmark);
        assert!(reader.pending_remove_bookmark);
        let _ = reader.update(Message::PreviousSection);
        assert_eq!(reader.section, 1);
        assert!(reader.open_path("other.txt".into()).is_none());
        let _ = reader.update(Message::CancelRemoveBookmark);
        assert!(!reader.pending_remove_bookmark);
        assert_eq!(reader.path.as_deref(), Some(Path::new("book.epub")));
        assert_eq!(reader.section, 1);
        assert!(!reader.busy);
        let _ = reader.update(Message::AskRemoveBookmark);
        let _ = reader.update(Message::ConfirmRemoveBookmark);
        assert!(!reader.pending_remove_bookmark);
        assert!(reader.busy);
        let _ = reader.update(Message::BookmarkRemoved(Ok(true)));
        assert!(!reader.busy);
        assert_eq!(reader.section, 1);
        assert!(reader.bookmark_notice.as_deref().unwrap().contains("dihapus"));
        let _ = reader.update(Message::BookmarkRemoved(Err("rusak".into())));
        assert_eq!(reader.error.as_deref(), Some("rusak"));
        assert_eq!(reader.section, 1);
    }

    #[test]
    fn remove_confirmation_is_unavailable_without_document_or_while_busy() {
        let mut reader = Reader::default();
        let _ = reader.update(Message::AskRemoveBookmark);
        assert!(!reader.pending_remove_bookmark);
        reader.path = Some("book.txt".into());
        reader.busy = true;
        let _ = reader.update(Message::AskRemoveBookmark);
        assert!(!reader.pending_remove_bookmark);
        let _ = reader.update(Message::ConfirmRemoveBookmark);
        assert!(reader.busy);
    }

    #[test]
    fn open_with_bookmark_restores_section_and_markdown_mode() {
        let mut reader = Reader::default();
        let path = PathBuf::from("book.epub");
        let doc = Document { sections: vec!["Satu".into(), "Dua".into()], is_epub: true, toc: Vec::new() };
        let bookmark = Bookmark { path: path.clone(), section: 1, section_count: Some(2), scroll_percent: Some(65), markdown_source: false };
        let _ = reader.update(Message::OpenedWithBookmark(Ok((path.clone(), doc, Ok(Some(bookmark))))));
        assert_eq!(reader.path.as_ref(), Some(&path));
        assert_eq!(reader.section, 1);
        assert_eq!(reader.pending_scroll, Some(65));
        assert!(!reader.busy);
        let path = PathBuf::from("book.md");
        let doc = Document { sections: vec!["# Judul".into()], is_epub: false, toc: Vec::new() };
        let bookmark = Bookmark { path: path.clone(), section: 0, section_count: None, scroll_percent: None, markdown_source: true };
        let _ = reader.update(Message::OpenedWithBookmark(Ok((path, doc, Ok(Some(bookmark))))));
        assert!(reader.show_source);
        assert_eq!(reader.pending_scroll, Some(0));
    }

    #[test]
    fn open_with_missing_or_invalid_bookmark_starts_at_top_and_reports_problem() {
        let path = PathBuf::from("book.epub");
        let doc = Document { sections: vec!["Satu".into()], is_epub: true, toc: Vec::new() };
        for result in [Ok(None), Err("rusak".into()), Ok(Some(Bookmark {
            path: path.clone(), section: 4, section_count: Some(5), scroll_percent: Some(20), markdown_source: false,
        }))] {
            let mut reader = Reader::default();
            let _ = reader.update(Message::OpenedWithBookmark(Ok((path.clone(), doc.clone(), result))));
            assert_eq!(reader.section, 0);
            assert_eq!(reader.pending_scroll, Some(0));
            assert!(reader.error.is_some() || reader.bookmark_notice.is_some());
        }
    }

    #[test]
    fn failed_continuation_keeps_previous_document_and_busy_guard() {
        let mut reader = Reader { path: Some("old.txt".into()), sections: vec!["lama".into()], ..Reader::default() };
        let _task = reader.open_path_with_bookmark("missing.epub".into()).unwrap();
        assert!(reader.is_busy());
        assert!(reader.open_path_with_bookmark("other.epub".into()).is_none());
        let _ = reader.update(Message::OpenedWithBookmark(Err("gagal".into())));
        assert_eq!(reader.path.as_deref(), Some(Path::new("old.txt")));
        assert_eq!(reader.sections, vec!["lama".to_owned()]);
        assert_eq!(reader.error.as_deref(), Some("gagal"));
        assert!(!reader.busy);
    }

    #[test]
    fn continuation_defers_restore_when_reader_is_hidden() {
        let mut app = crate::NovelCraft::default();
        let path = PathBuf::from("book.epub");
        let doc = Document { sections: vec!["Satu".into()], is_epub: true, toc: Vec::new() };
        let bookmark = Bookmark { path: path.clone(), section: 0, section_count: Some(1), scroll_percent: Some(35), markdown_source: false };
        let _ = app.update(crate::Message::Reader(Message::OpenedWithBookmark(Ok((path, doc, Ok(Some(bookmark)))))));
        assert_eq!(app.reader.pending_scroll, Some(35));
        let _ = app.update(crate::Message::Navigate(crate::Page::Reader));
        assert_eq!(app.reader.pending_scroll, None);
    }

    #[test]
    fn page_navigation_remembers_measured_scroll_without_saving_bookmarks() {
        use crate::{Message as AppMessage, NovelCraft, Page};
        let mut app = NovelCraft::default();
        app.page = Page::Reader;
        app.reader = Reader { path: Some("book.epub".into()), sections: vec!["first".into(), "second".into()], section: 1, is_epub: true, scroll_percent: Some(63), ..Reader::default() };
        let old_revision = app.reader.scroll_revision;
        let _ = app.update(AppMessage::Navigate(Page::Library));
        assert_eq!(app.reader.pending_scroll, Some(63));
        assert_eq!(app.reader.section, 1);
        assert_eq!(app.reader.scroll_percent, None);
        let _ = app.update(AppMessage::Reader(Message::Scrolled { revision: old_revision, percent: Some(99) }));
        assert_eq!(app.reader.pending_scroll, Some(63));
        let _ = app.update(AppMessage::Navigate(Page::Reader));
        assert_eq!(app.reader.pending_scroll, None);
        assert_eq!(app.reader.restored_scroll_fallback, Some(63));
        // snap_to itself may not emit on_scroll, so a second page switch still works.
        let _ = app.update(AppMessage::Navigate(Page::Writer));
        assert_eq!(app.reader.pending_scroll, Some(63));
        let _ = app.update(AppMessage::Navigate(Page::Reader));
        let revision = app.reader.scroll_revision;
        let _ = app.update(AppMessage::Reader(Message::Scrolled { revision, percent: Some(80) }));
        assert_eq!(app.reader.restored_scroll_fallback, None);
        let _ = app.update(AppMessage::Navigate(Page::Settings));
        assert_eq!(app.reader.pending_scroll, Some(80));
    }

    #[test]
    fn close_dialog_defers_scroll_and_preserves_existing_bookmark_restore() {
        use crate::{Message as AppMessage, NovelCraft, Page};
        let mut app = NovelCraft::default();
        app.page = Page::Reader;
        app.reader = Reader { path: Some("book.txt".into()), sections: vec!["text".into()], scroll_percent: Some(44), busy: true, ..Reader::default() };
        let id = iced::window::Id::unique();
        let _ = app.update(AppMessage::CloseRequested(id));
        assert_eq!(app.reader.pending_scroll, Some(44));
        let _ = app.update(AppMessage::CancelClose);
        assert_eq!(app.reader.restored_scroll_fallback, Some(44));
        app.reader.pending_scroll = Some(20); // An explicit bookmark restore wins.
        app.reader.scroll_percent = Some(90);
        let _ = app.update(AppMessage::CloseRequested(id));
        assert_eq!(app.reader.pending_scroll, Some(20));
        let _ = app.update(AppMessage::CancelClose);
        assert_eq!(app.reader.restored_scroll_fallback, Some(20));
    }

    #[test]
    fn explicit_bookmark_restore_wins_over_temporary_scroll_when_hidden() {
        use crate::{Message as AppMessage, NovelCraft, Page};
        let mut app = NovelCraft::default();
        app.page = Page::Reader;
        let path = PathBuf::from("book.epub");
        app.reader = Reader { path: Some(path.clone()), sections: vec!["one".into(), "two".into()], is_epub: true, scroll_percent: Some(60), ..Reader::default() };
        let _ = app.update(AppMessage::Navigate(Page::Library));
        assert_eq!(app.reader.pending_scroll, Some(60));
        let bookmark = Bookmark { path, section: 1, section_count: Some(2), scroll_percent: Some(25), markdown_source: false };
        let _ = app.update(AppMessage::Reader(Message::BookmarkLoaded(Ok(Some(bookmark)))));
        assert_eq!(app.reader.section, 1);
        assert_eq!(app.reader.pending_scroll, Some(25));
        let _ = app.update(AppMessage::Navigate(Page::Reader));
        assert_eq!(app.reader.restored_scroll_fallback, Some(25));
        let _ = app.update(AppMessage::Reader(Message::PreviousSection));
        assert_eq!(app.reader.section, 0);
        assert_eq!(app.reader.restored_scroll_fallback, None);
    }

    #[test]
    fn no_scroll_is_restored_when_position_was_never_measured() {
        let mut reader = Reader { path: Some("book.txt".into()), sections: vec!["text".into()], ..Reader::default() };
        reader.suspend_scroll_position();
        assert_eq!(reader.pending_scroll, None);
        let _ = reader.take_restore_task();
        assert_eq!(reader.restored_scroll_fallback, None);
    }

    #[test]
    fn new_document_clears_old_scroll_while_cancel_keeps_pending_position() {
        use crate::{Message as AppMessage, NovelCraft, Page};
        let mut app = NovelCraft::default();
        app.page = Page::Reader;
        app.reader = Reader { path: Some("old.txt".into()), sections: vec!["old".into()], scroll_percent: Some(50), busy: true, ..Reader::default() };
        let _ = app.update(AppMessage::Navigate(Page::Library));
        assert_eq!(app.reader.pending_scroll, Some(50));
        let _ = app.update(AppMessage::Reader(Message::Opened(Ok(None))));
        assert_eq!(app.reader.pending_scroll, Some(50));
        let new_doc = Document { sections: vec!["new".into()], toc: Vec::new(), is_epub: false };
        let _ = app.update(AppMessage::Reader(Message::Opened(Ok(Some(("new.txt".into(), new_doc))))));
        assert_eq!(app.reader.pending_scroll, None);
        assert_eq!(app.reader.path.as_deref(), Some(Path::new("new.txt")));
    }

    #[test]
    fn continuation_waits_for_close_dialog_to_be_cancelled() {
        let mut app = crate::NovelCraft::default();
        app.page = crate::Page::Reader;
        app.pending_close = Some(iced::window::Id::unique());
        let path = PathBuf::from("book.epub");
        let doc = Document { sections: vec!["Satu".into()], is_epub: true, toc: Vec::new() };
        let bookmark = Bookmark { path: path.clone(), section: 0, section_count: Some(1), scroll_percent: Some(35), markdown_source: false };
        let _ = app.update(crate::Message::Reader(Message::OpenedWithBookmark(Ok((path, doc, Ok(Some(bookmark)))))));
        assert_eq!(app.reader.pending_scroll, Some(35));
        let _ = app.update(crate::Message::CancelClose);
        assert_eq!(app.reader.pending_scroll, None);
    }

    #[test]
    fn bookmark_snapshot_includes_epub_section_count() {
        let mut reader = Reader::default();
        assert!(reader.snapshot_bookmark().is_none());
        let path = PathBuf::from("book.epub");
        let doc = Document { sections: vec!["Satu".into(), "Dua".into()], is_epub: true, toc: Vec::new() };
        let _ = reader.update(Message::Opened(Ok(Some((path.clone(), doc)))));
        reader.section = 1;
        reader.scroll_percent = Some(50);
        let snapshot = reader.snapshot_bookmark().unwrap();
        assert_eq!(snapshot.path, path);
        assert_eq!(snapshot.section, 1);
        assert_eq!(snapshot.section_count, Some(2));
        assert_eq!(snapshot.scroll_percent, Some(50));
        reader.is_epub = false;
        assert_eq!(reader.snapshot_bookmark().unwrap().section_count, None);
    }

    #[test]
    fn restoring_bookmark_sets_section_and_defers_scroll_until_requested() {
        let mut reader = Reader::default();
        let path = PathBuf::from("book.epub");
        let doc = Document { sections: vec!["Satu".into(), "Dua".into()], is_epub: true, toc: Vec::new() };
        let _ = reader.update(Message::Opened(Ok(Some((path.clone(), doc)))));
        let revision = reader.scroll_revision;
        let bookmark = Bookmark { path, section: 1, section_count: None, scroll_percent: Some(65), markdown_source: false };
        let _ = reader.update(Message::BookmarkLoaded(Ok(Some(bookmark))));
        assert_eq!(reader.section, 1);
        assert_eq!(reader.pending_scroll, Some(65));
        assert!(reader.scroll_revision != revision);
        let _ = reader.take_restore_task();
        assert_eq!(reader.pending_scroll, None);
    }

    #[test]
    fn bookmark_restores_markdown_mode_and_handles_unmeasured_position() {
        let mut reader = Reader::default();
        let path = PathBuf::from("book.md");
        let doc = Document { sections: vec!["# Judul".into()], is_epub: false, toc: Vec::new() };
        let _ = reader.update(Message::Opened(Ok(Some((path.clone(), doc)))));
        let bookmark = Bookmark { path, section: 0, section_count: None, scroll_percent: None, markdown_source: true };
        let _ = reader.update(Message::BookmarkLoaded(Ok(Some(bookmark))));
        assert!(reader.show_source);
        assert_eq!(reader.pending_scroll, Some(0));
        assert!(reader.bookmark_notice.as_deref().unwrap().contains("tidak tersedia"));
    }

    #[test]
    fn invalid_bookmark_does_not_move_reader() {
        let mut reader = Reader::default();
        let path = PathBuf::from("book.epub");
        let doc = Document { sections: vec!["Satu".into()], is_epub: true, toc: Vec::new() };
        let _ = reader.update(Message::Opened(Ok(Some((path.clone(), doc)))));
        let bookmark = Bookmark { path, section: 4, section_count: None, scroll_percent: Some(50), markdown_source: false };
        let _ = reader.update(Message::BookmarkLoaded(Ok(Some(bookmark))));
        assert_eq!(reader.section, 0);
        assert!(reader.pending_scroll.is_none());
        assert!(reader.error.is_some());
    }

    #[test]
    fn missing_or_failed_bookmark_keeps_current_section() {
        let mut reader = Reader { section: 1, sections: vec!["Satu".into(), "Dua".into()], ..Reader::default() };
        let _ = reader.update(Message::BookmarkLoaded(Ok(None)));
        assert_eq!(reader.section, 1);
        assert!(reader.bookmark_notice.is_some());
        let _ = reader.update(Message::BookmarkSaved(Err("gagal".into())));
        assert_eq!(reader.section, 1);
        assert_eq!(reader.error.as_deref(), Some("gagal"));
    }

    #[test]
    fn scroll_position_is_bounded_and_handles_short_content() {
        assert_eq!(scroll_percentage(0.0, 100.0, 300.0), Some(0));
        assert_eq!(scroll_percentage(100.0, 100.0, 300.0), Some(50));
        assert_eq!(scroll_percentage(200.0, 100.0, 300.0), Some(100));
        assert_eq!(scroll_percentage(-10.0, 100.0, 300.0), Some(0));
        assert_eq!(scroll_percentage(500.0, 100.0, 300.0), Some(100));
        assert_eq!(scroll_percentage(0.0, 100.0, 100.0), None);
        assert_eq!(scroll_percentage(0.0, 100.0, 20.0), None);
        assert_eq!(scroll_percentage(f32::NAN, 100.0, 300.0), None);
        assert_eq!(scroll_percentage(0.0, 0.0, 300.0), None);
    }

    #[test]
    fn scroll_position_resets_for_new_section_and_ignores_stale_events() {
        let mut reader = Reader::default();
        let document = Document { sections: vec!["Satu".into(), "Dua".into()], is_epub: true, toc: Vec::new() };
        let _ = reader.update(Message::Opened(Ok(Some((PathBuf::from("book.epub"), document)))));
        let old_revision = reader.scroll_revision;
        let _ = reader.update(Message::Scrolled { revision: old_revision, percent: Some(75) });
        assert_eq!(reader.scroll_percent, Some(75));
        let _ = reader.update(Message::NextSection);
        assert_eq!(reader.scroll_percent, None);
        let _ = reader.update(Message::Scrolled { revision: old_revision, percent: Some(75) });
        assert_eq!(reader.scroll_percent, None);
        let _ = reader.update(Message::Scrolled { revision: reader.scroll_revision, percent: Some(15) });
        assert_eq!(reader.scroll_percent, Some(15));
        reader.invalidate_scroll_position();
        assert_eq!(reader.scroll_percent, None);
    }

    #[test]
    fn scroll_position_resets_when_changing_text_size_and_opening_a_file() {
        let mut reader = Reader::default();
        let document = Document { sections: vec!["Teks".into()], is_epub: false, toc: Vec::new() };
        let _ = reader.update(Message::Opened(Ok(Some((PathBuf::from("old.txt"), document)))));
        let _ = reader.update(Message::Scrolled { revision: reader.scroll_revision, percent: Some(60) });
        reader.apply_font_size(reader.requested_font_size(&Message::FontLarger).unwrap());
        assert_eq!(reader.scroll_percent, None);
        let _ = reader.update(Message::Scrolled { revision: reader.scroll_revision, percent: Some(30) });
        let document = Document { sections: vec!["Lain".into()], is_epub: false, toc: Vec::new() };
        let _ = reader.update(Message::Opened(Ok(Some((PathBuf::from("new.txt"), document)))));
        assert_eq!(reader.scroll_percent, None);
    }

    #[test]
    fn font_size_stays_within_bounds() {
        let mut reader = Reader::default();
        for _ in 0..100 {
            if let Some(size) = reader.requested_font_size(&Message::FontSmaller) { reader.apply_font_size(size); }
        }
        assert_eq!(reader.font_size, MIN_FONT_SIZE);
        for _ in 0..100 {
            if let Some(size) = reader.requested_font_size(&Message::FontLarger) { reader.apply_font_size(size); }
        }
        assert_eq!(reader.font_size, MAX_FONT_SIZE);
        assert_eq!(reader.requested_font_size(&Message::FontLarger), None);
        reader.apply_font_size(0);
        assert_eq!(reader.font_size, MAX_FONT_SIZE);
        reader.busy = true;
        assert_eq!(reader.requested_font_size(&Message::FontSmaller), None);
    }

    #[test]
    fn cancel_and_error_preserve_previous_document() {
        let path = PathBuf::from("old.txt");
        let mut reader = Reader {
            path: Some(path.clone()),
            sections: vec!["Teks lama".into()],
            busy: true,
            ..Reader::default()
        };
        let _ = reader.update(Message::Opened(Ok(None)));
        assert!(!reader.busy);
        let _ = reader.update(Message::Opened(Err("gagal".into())));
        assert_eq!(reader.path, Some(path));
        assert_eq!(reader.sections[0], "Teks lama");
        assert_eq!(reader.error.as_deref(), Some("gagal"));
    }

    #[test]
    fn opening_document_replaces_contents() {
        let mut reader = Reader::default();
        let path = PathBuf::from("novel.md");
        let document = Document { sections: vec!["Isi novel".into()], is_epub: false, toc: Vec::new() };
        let _ = reader.update(Message::Opened(Ok(Some((path.clone(), document)))));
        assert_eq!(reader.path, Some(path));
        assert_eq!(reader.sections[0], "Isi novel");
        assert!(!reader.busy);
    }

    #[test]
    fn section_search_wraps_and_ignores_case() {
        let sections = vec!["Bulan dan laut".into(), "Hujan turun".into(), "Malam BULAN".into()];
        assert_eq!(find_matching_section(&sections, 0, "bulAN", true), Some(2));
        assert_eq!(find_matching_section(&sections, 2, " bulan ", true), Some(0));
        assert_eq!(find_matching_section(&sections, 0, "BULAN", false), Some(2));
        assert_eq!(find_matching_section(&sections, 1, "hujan", true), Some(1));
        assert_eq!(find_matching_section(&sections, 0, "tak ada", true), None);
        assert_eq!(find_matching_section(&sections, 0, "  ", true), None);
        assert_eq!(find_matching_section(&[], 0, "bulan", true), None);
        assert_eq!(find_matching_section(&sections, 3, "bulan", true), None);
        assert_eq!(find_matching_section(&["Éclair".into()], 0, "éCLAIR", true), Some(0));
    }

    #[test]
    fn section_search_changes_section_only_for_a_match_and_resets_scroll() {
        let mut reader = Reader::default();
        let doc = Document { sections: vec!["Awal".into(), "Malam bulan".into(), "Akhir bulan".into()], is_epub: true, toc: Vec::new() };
        let _ = reader.update(Message::Opened(Ok(Some(("book.epub".into(), doc)))));
        let revision = reader.scroll_revision;
        let _ = reader.update(Message::Scrolled { revision, percent: Some(20) });
        let _ = reader.update(Message::SearchNext);
        assert_eq!(reader.section, 0);
        assert_eq!(reader.scroll_percent, Some(20));
        let _ = reader.update(Message::SearchInputChanged("tidak ada".into()));
        let _ = reader.update(Message::SearchNext);
        assert_eq!(reader.section, 0);
        assert_eq!(reader.scroll_revision, revision);
        assert_eq!(reader.scroll_percent, Some(20));
        assert!(reader.search_notice.as_deref().unwrap().contains("tidak ditemukan"));
        let _ = reader.update(Message::SearchInputChanged("BULAN".into()));
        let _ = reader.update(Message::SearchNext);
        assert_eq!(reader.section, 1);
        assert_eq!(reader.scroll_percent, None);
        let _ = reader.update(Message::Scrolled { revision, percent: Some(20) });
        assert_eq!(reader.scroll_percent, None);
        let _ = reader.update(Message::SearchPrevious);
        assert_eq!(reader.section, 2);
    }

    #[test]
    fn section_search_is_blocked_while_busy_and_cleared_on_open() {
        let mut reader = Reader { sections: vec!["satu".into(), "dua".into()], is_epub: true, busy: true, ..Reader::default() };
        let _ = reader.update(Message::SearchInputChanged("dua".into()));
        let _ = reader.update(Message::SearchNext);
        assert!(reader.search_input.is_empty());
        assert_eq!(reader.section, 0);
        reader.busy = false;
        let _ = reader.update(Message::SearchInputChanged("dua".into()));
        let doc = Document { sections: vec!["teks".into()], is_epub: false, toc: Vec::new() };
        let _ = reader.update(Message::Opened(Ok(Some(("book.txt".into(), doc)))));
        assert!(reader.search_input.is_empty());
        assert!(reader.search_notice.is_none());
        let _ = reader.update(Message::ToggleTextSearch);
        let _ = reader.update(Message::SearchInputChanged("teks".into()));
        let _ = reader.update(Message::SearchNext);
        assert_eq!(reader.search_input, "teks");
        assert_eq!(reader.text_search_line, Some(0));
    }

    #[test]
    fn text_search_finds_lines_wraps_and_builds_utf8_safe_excerpts() {
        let text = "Pertama BULAN dan bulan\r\nTidak ada\nÉCLAIR bulan\nTerakhir BULAN";
        let hit = find_text_line(text, " bulan ", None, true).unwrap();
        assert_eq!((hit.line, hit.rank, hit.total), (0, 1, 3));
        assert_eq!(hit.excerpt, "Pertama BULAN dan bulan");
        let hit = find_text_line(text, "BULAN", Some(hit.line), true).unwrap();
        assert_eq!((hit.line, hit.rank, hit.total), (2, 2, 3));
        assert!(hit.excerpt.contains("ÉCLAIR"));
        let hit = find_text_line(text, "bulan", Some(3), true).unwrap();
        assert_eq!((hit.line, hit.rank), (0, 1));
        let hit = find_text_line(text, "bulan", Some(0), false).unwrap();
        assert_eq!((hit.line, hit.rank), (3, 3));
        assert_eq!(find_text_line(text, "éclair", None, true).unwrap().line, 2);
        assert_eq!(find_text_line("İSTANBUL", "i\u{307}st", None, true).unwrap().excerpt, "İSTANBUL");
        assert!(find_text_line(text, "bulan\nTidak", None, true).is_none());
        assert!(find_text_line(text, "  ", None, true).is_none());
        assert!(find_text_line(text, "absen", None, true).is_none());
        let long_line = format!("{}Target{}", "🌙".repeat(100), "a".repeat(100));
        let excerpt = find_text_line(&long_line, "target", None, true).unwrap().excerpt;
        assert!(excerpt.starts_with('…') && excerpt.ends_with('…'));
        assert!(excerpt.contains("Target"));
        assert!(excerpt.chars().count() <= 122);
    }

    #[test]
    fn text_search_state_is_guarded_and_does_not_change_scroll_or_bookmark() {
        let mut reader = Reader::default();
        let doc = Document { sections: vec!["# Bulan\nSatu\nbulan dua".into()], is_epub: false, toc: Vec::new() };
        let _ = reader.update(Message::Opened(Ok(Some(("book.md".into(), doc)))));
        assert!(!reader.show_text_search);
        let _ = reader.update(Message::SearchInputChanged("bulan".into()));
        assert!(reader.search_input.is_empty());
        let _ = reader.update(Message::ToggleTextSearch);
        assert!(reader.show_text_search);
        let revision = reader.scroll_revision;
        let _ = reader.update(Message::Scrolled { revision, percent: Some(57) });
        let bookmark = reader.snapshot_bookmark().unwrap();
        let _ = reader.update(Message::SearchInputChanged("BULAN".into()));
        let _ = reader.update(Message::SearchNext);
        assert_eq!(reader.text_search_line, Some(0));
        assert_eq!(reader.text_search_excerpt.as_deref(), Some("# Bulan"));
        assert!(reader.search_notice.as_deref().unwrap().contains("1 dari 2"));
        let _ = reader.update(Message::SearchPrevious);
        assert_eq!(reader.text_search_line, Some(2));
        assert_eq!(reader.scroll_revision, revision);
        assert_eq!(reader.scroll_percent, Some(57));
        assert_eq!(reader.snapshot_bookmark().unwrap().scroll_percent, bookmark.scroll_percent);
        drop(reader.view(&Theme::TokyoNight, true, ""));
        let _ = reader.update(Message::SearchInputChanged("tidak ada".into()));
        assert!(reader.text_search_excerpt.is_none());
        let _ = reader.update(Message::SearchNext);
        assert!(reader.search_notice.as_deref().unwrap().contains("tidak ditemukan"));
        reader.pending_remove_bookmark = true;
        let _ = reader.update(Message::ToggleTextSearch);
        let _ = reader.update(Message::SearchInputChanged("lain".into()));
        assert!(reader.show_text_search);
        assert_eq!(reader.search_input, "tidak ada");
        reader.pending_remove_bookmark = false;
        let _ = reader.update(Message::ToggleTextSearch);
        assert!(!reader.show_text_search);
        assert_eq!(reader.scroll_percent, None);
        let doc = Document { sections: vec!["baru".into()], is_epub: false, toc: Vec::new() };
        let _ = reader.update(Message::Opened(Ok(Some(("other.txt".into(), doc)))));
        assert!(reader.search_input.is_empty());
        assert!(reader.text_search_line.is_none());
        assert!(reader.text_search_excerpt.is_none());
        assert!(!reader.show_text_search);
    }

    #[test]
    fn section_number_parser_checks_bounds_and_rejects_invalid_input() {
        assert_eq!(parse_section_number("1", 3), Some(0));
        assert_eq!(parse_section_number("3", 3), Some(2));
        assert_eq!(parse_section_number("0", 3), None);
        assert_eq!(parse_section_number("4", 3), None);
        assert_eq!(parse_section_number("1", 0), None);
        assert_eq!(parse_section_number("", 3), None);
        assert_eq!(parse_section_number("+1", 3), None);
        assert_eq!(parse_section_number(" 1", 3), None);
        assert_eq!(parse_section_number("999999999999999999999999999999", 3), None);
    }

    #[test]
    fn jumping_to_section_resets_scroll_and_invalid_input_preserves_position() {
        let mut reader = Reader::default();
        let doc = Document { sections: vec!["Satu".into(), "Dua".into(), "Tiga".into()], is_epub: true, toc: Vec::new() };
        let _ = reader.update(Message::Opened(Ok(Some(("book.epub".into(), doc)))));
        let old_revision = reader.scroll_revision;
        let _ = reader.update(Message::Scrolled { revision: old_revision, percent: Some(40) });
        let _ = reader.update(Message::SectionInputChanged("2".into()));
        let _ = reader.update(Message::JumpToSection);
        assert_eq!(reader.section, 1);
        assert_eq!(reader.scroll_percent, None);
        assert_ne!(reader.scroll_revision, old_revision);
        let _ = reader.update(Message::Scrolled { revision: old_revision, percent: Some(40) });
        assert_eq!(reader.scroll_percent, None);
        let new_revision = reader.scroll_revision;
        let _ = reader.update(Message::Scrolled { revision: new_revision, percent: Some(15) });
        let _ = reader.update(Message::SectionInputChanged("99".into()));
        let _ = reader.update(Message::JumpToSection);
        assert!(reader.section_input_error);
        assert_eq!(reader.section, 1);
        assert_eq!(reader.scroll_percent, Some(15));
        assert_eq!(reader.scroll_revision, new_revision);
        let _ = reader.update(Message::SectionInputChanged("3".into()));
        assert!(!reader.section_input_error);
        let _ = reader.update(Message::JumpToSection);
        assert_eq!(reader.section, 2);
    }

    #[test]
    fn section_jump_is_blocked_when_busy_or_not_epub_and_resets_on_open() {
        let mut reader = Reader { sections: vec!["Teks".into(), "Lain".into()], is_epub: true, busy: true, ..Reader::default() };
        let _ = reader.update(Message::SectionInputChanged("2".into()));
        let _ = reader.update(Message::JumpToSection);
        assert_eq!(reader.section, 0);
        assert!(reader.section_input.is_empty());
        reader.busy = false;
        let _ = reader.update(Message::SectionInputChanged("2".into()));
        let doc = Document { sections: vec!["teks".into()], is_epub: false, toc: Vec::new() };
        let _ = reader.update(Message::Opened(Ok(Some(("book.txt".into(), doc)))));
        assert!(reader.section_input.is_empty());
        let _ = reader.update(Message::SectionInputChanged("1".into()));
        let _ = reader.update(Message::JumpToSection);
        assert!(reader.section_input.is_empty());
        assert_eq!(reader.section, 0);
    }

    #[test]
    fn ncx_navigation_is_guarded_and_jumps_to_start_of_selected_section() {
        use crate::epub_reader::TocEntry;
        let mut reader = Reader::default();
        let doc = Document {
            sections: vec!["awal".into(), "akhir".into()],
            toc: vec![TocEntry { label: "Akhir".into(), section: 1, depth: 0 }],
            is_epub: true,
        };
        let _ = reader.update(Message::Opened(Ok(Some(("book.epub".into(), doc)))));
        let _ = reader.update(Message::SelectToc(0));
        assert_eq!(reader.section, 0); // Panel is closed.
        reader.scroll_percent = Some(50);
        let revision = reader.scroll_revision;
        let _ = reader.update(Message::ToggleToc);
        assert!(reader.show_toc);
        assert_eq!(reader.scroll_percent, None);
        let _ = reader.update(Message::Scrolled { revision, percent: Some(50) });
        assert_eq!(reader.scroll_percent, None);
        drop(reader.view(&Theme::TokyoNight, true, ""));
        reader.busy = true;
        let _ = reader.update(Message::SelectToc(0));
        assert_eq!(reader.section, 0);
        reader.busy = false;
        let _ = reader.update(Message::SelectToc(99));
        assert_eq!(reader.section, 0);
        let _ = reader.update(Message::SelectToc(0));
        assert_eq!(reader.section, 1);
        assert!(!reader.show_toc);
        assert_eq!(reader.scroll_percent, None);
        let text = Document { sections: vec!["catatan".into()], toc: Vec::new(), is_epub: false };
        let _ = reader.update(Message::Opened(Ok(Some(("note.txt".into(), text)))));
        assert!(reader.toc.is_empty());
        assert!(!reader.show_toc);
    }

    #[test]
    fn saved_font_size_applies_only_after_success_and_respects_guards() {
        use crate::{Message as AppMessage, NovelCraft, Page};
        let mut app = NovelCraft::default();
        app.page = Page::Reader;
        let _ = app.update(AppMessage::Reader(Message::FontLarger));
        assert!(!app.settings_busy); // Settings have not loaded.
        assert_eq!(app.reader.font_size, 18);
        let _ = app.update(AppMessage::SettingsLoaded(Ok(crate::settings::Settings {
            theme: crate::ThemeChoice::Nord, reader_font_size: 24,
        })));
        assert_eq!(app.reader.font_size, 24);
        app.reader.scroll_percent = Some(30);
        let _ = app.update(AppMessage::Reader(Message::FontLarger));
        assert!(app.settings_busy);
        assert_eq!(app.reader.font_size, 24); // Unchanged until save succeeds.
        let _ = app.update(AppMessage::Reader(Message::FontSmaller));
        assert_eq!(app.reader.font_size, 24); // No overlapping writes.
        let _ = app.update(AppMessage::SelectTheme(crate::ThemeChoice::Dark));
        assert_eq!(app.theme_choice, crate::ThemeChoice::Nord);
        let id = iced::window::Id::unique();
        let _ = app.update(AppMessage::CloseRequested(id));
        let _ = app.update(AppMessage::ConfirmClose);
        assert_eq!(app.pending_close, Some(id));
        let _ = app.update(AppMessage::ReaderFontSaved(Err("gagal".into())));
        assert_eq!(app.reader.font_size, 24);
        assert_eq!(app.settings_error.as_deref(), Some("gagal"));
        let _ = app.update(AppMessage::CancelClose);
        let _ = app.update(AppMessage::Reader(Message::FontSmaller));
        assert!(app.settings_busy);
        let _ = app.update(AppMessage::ReaderFontSaved(Ok(22)));
        assert_eq!(app.reader.font_size, 22);
        assert_eq!(app.reader.scroll_percent, None);
        assert!(app.settings_error.is_none());
        app.reader.busy = true;
        let _ = app.update(AppMessage::Reader(Message::FontLarger));
        assert!(!app.settings_busy);
        app.reader.busy = false;
        app.page = Page::Writer;
        let _ = app.update(AppMessage::Reader(Message::FontLarger));
        assert!(!app.settings_busy);
    }

    #[test]
    fn reader_focus_invalidates_stale_scroll_measurement_without_changing_document() {
        use crate::{Message as AppMessage, NovelCraft, Page};
        let mut app = NovelCraft::default();
        app.page = Page::Reader;
        app.reader = Reader { path: Some("book.epub".into()), sections: vec!["satu".into(), "dua".into()], section: 1, is_epub: true, scroll_percent: Some(75), ..Reader::default() };
        let revision = app.reader.scroll_revision;
        let _ = app.update(AppMessage::ToggleFocusMode);
        assert!(app.focus_mode);
        assert_eq!(app.reader.section, 1);
        assert_eq!(app.reader.path.as_deref(), Some(Path::new("book.epub")));
        assert_eq!(app.reader.scroll_percent, None);
        let _ = app.update(AppMessage::Reader(Message::Scrolled { revision, percent: Some(75) }));
        assert_eq!(app.reader.scroll_percent, None);
        let new_revision = app.reader.scroll_revision;
        let _ = app.update(AppMessage::Reader(Message::Scrolled { revision: new_revision, percent: Some(40) }));
        assert_eq!(app.reader.scroll_percent, Some(40));
        let _ = app.update(AppMessage::ToggleFocusMode);
        assert!(!app.focus_mode);
        assert_eq!(app.reader.scroll_percent, None);
        assert_eq!(app.reader.section, 1);
    }

    #[test]
    fn section_shortcuts_respect_page_busy_confirmation_and_document_type() {
        use crate::{Message as AppMessage, NovelCraft, Page};
        use crate::shortcuts::Shortcut;
        let mut app = NovelCraft::default();
        app.reader = Reader { path: Some("book.epub".into()), sections: vec!["satu".into(), "dua".into()], is_epub: true, ..Reader::default() };
        let _ = app.update(AppMessage::Shortcut(Shortcut::NextSection));
        assert_eq!(app.reader.section, 0); // Library, not Reader.
        app.page = Page::Reader;
        let _ = app.update(AppMessage::Shortcut(Shortcut::PreviousSection));
        assert_eq!(app.reader.section, 0);
        let _ = app.update(AppMessage::Shortcut(Shortcut::NextSection));
        let _ = app.update(AppMessage::Shortcut(Shortcut::NextSection));
        assert_eq!(app.reader.section, 1); // Stop at end.
        app.reader.busy = true;
        let _ = app.update(AppMessage::Shortcut(Shortcut::PreviousSection));
        assert_eq!(app.reader.section, 1);
        app.reader.busy = false;
        let _ = app.update(AppMessage::Reader(Message::AskRemoveBookmark));
        let _ = app.update(AppMessage::Shortcut(Shortcut::PreviousSection));
        assert_eq!(app.reader.section, 1);
        let _ = app.update(AppMessage::Reader(Message::CancelRemoveBookmark));
        let id = iced::window::Id::unique();
        app.pending_close = Some(id);
        let _ = app.update(AppMessage::Shortcut(Shortcut::PreviousSection));
        assert_eq!(app.reader.section, 1);
        app.pending_close = None;
        let _ = app.update(AppMessage::Shortcut(Shortcut::PreviousSection));
        assert_eq!(app.reader.section, 0);
        app.reader.is_epub = false;
        let _ = app.update(AppMessage::Shortcut(Shortcut::NextSection));
        assert_eq!(app.reader.section, 0);
    }

    #[test]
    fn section_navigation_is_bounded_and_resets_for_new_document() {
        let mut reader = Reader::default();
        let document = Document { sections: vec!["Satu".into(), "Dua".into()], is_epub: true, toc: Vec::new() };
        let _ = reader.update(Message::Opened(Ok(Some((PathBuf::from("book.epub"), document)))));
        let _ = reader.update(Message::PreviousSection);
        assert_eq!(reader.section, 0);
        let _ = reader.update(Message::NextSection);
        let _ = reader.update(Message::NextSection);
        assert_eq!(reader.section, 1);
        let document = Document { sections: vec!["Lain".into()], is_epub: false, toc: Vec::new() };
        let _ = reader.update(Message::Opened(Ok(Some((PathBuf::from("other.txt"), document)))));
        assert_eq!(reader.section, 0);
        assert!(!reader.is_epub);
    }

    #[test]
    fn markdown_is_parsed_and_source_mode_can_be_toggled() {
        let mut reader = Reader::default();
        let document = Document {
            sections: vec!["# Judul\n\nTeks **tebal**.\n\n- Satu\n- Dua".into()],
            is_epub: false,
            toc: Vec::new(),
        };
        let _ = reader.update(Message::Opened(Ok(Some((PathBuf::from("novel.MD"), document)))));
        assert!(reader.is_markdown);
        assert!(matches!(reader.markdown_items.first(), Some(markdown::Item::Heading(..))));
        assert!(reader.markdown_items.iter().any(|item| matches!(item, markdown::Item::List { .. })));
        for theme in [Theme::TokyoNight, Theme::Dark, Theme::Nord, Theme::Dracula] {
            drop(reader.view(&theme, true, ""));
        }
        assert!(!reader.show_source);
        let _ = reader.update(Message::ToggleSource);
        assert!(reader.show_source);
        let _ = reader.update(Message::ToggleSource);
        assert!(!reader.show_source);
    }

    #[test]
    fn opening_text_clears_markdown_state() {
        let mut reader = Reader::default();
        let doc = Document { sections: vec!["# Judul".into()], is_epub: false, toc: Vec::new() };
        let _ = reader.update(Message::Opened(Ok(Some((PathBuf::from("book.markdown"), doc)))));
        let _ = reader.update(Message::ToggleSource);
        let doc = Document { sections: vec!["# teks biasa".into()], is_epub: false, toc: Vec::new() };
        let _ = reader.update(Message::Opened(Ok(Some((PathBuf::from("book.txt"), doc)))));
        assert!(!reader.is_markdown);
        assert!(!reader.show_source);
        assert!(reader.markdown_items.is_empty());
        assert_eq!(reader.sections[0], "# teks biasa");
    }

    #[test]
    fn clicking_link_only_shows_its_address() {
        let mut reader = Reader::default();
        let url = markdown::Url::parse("https://example.com/").unwrap();
        let _ = reader.update(Message::LinkClicked(url));
        assert!(reader.link_notice.as_deref().unwrap().contains("https://example.com/"));
    }

    #[tokio::test]
    async fn reads_utf8_and_reports_invalid_encoding() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("novelcraft-reader-{}-{unique}.txt", std::process::id()));
        tokio::fs::write(&path, "Halo dunia").await.unwrap();
        assert_eq!(read_text(&path).await.unwrap(), "Halo dunia");
        tokio::fs::write(&path, [0xff]).await.unwrap();
        assert!(read_text(&path).await.is_err());
        tokio::fs::remove_file(path).await.unwrap();
    }
}
