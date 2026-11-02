use iced::widget::{button, column, container, markdown, progress_bar, row, scrollable, text, text_editor, text_input};
use iced::{highlighter, Element, Length, Task, Theme};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::AsyncWriteExt;

#[derive(Default)]
pub struct Writer {
    content: text_editor::Content,
    word_count: usize,
    path: Option<PathBuf>,
    saved_contents: Option<String>,
    target_input: String,
    show_preview: bool,
    preview_items: Vec<markdown::Item>,
    preview_notice: Option<String>,
    dirty: bool,
    busy: bool,
    pending_discard: Option<PendingAction>,
    pending_format_save: Option<bool>,
    pending_overwrite: Option<PreparedSave>,
    error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingAction {
    NewDraft,
    Open,
}

#[derive(Debug, Clone)]
pub struct PreparedSave {
    path: PathBuf,
    contents: String,
    expected: FileState,
    expected_target: Option<PathBuf>,
    requires_overwrite: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    Edit(text_editor::Action),
    TargetChanged(String),
    Shortcut(crate::shortcuts::Shortcut),
    TogglePreview,
    PreviewLink(markdown::Url),
    NewDraft,
    Open,
    ConfirmDiscard,
    CancelDiscard,
    Opened(Result<Option<(PathBuf, String)>, String>),
    Save,
    SaveAs,
    ConfirmFormatSave,
    CancelFormatSave,
    SavePrepared(Result<Option<PreparedSave>, String>),
    ConfirmOverwrite,
    CancelOverwrite,
    Saved(Result<Option<(PathBuf, String)>, String>),
}

impl Writer {
    pub fn has_unsaved_changes(&self) -> bool {
        self.dirty
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    fn controls_blocked(&self) -> bool {
        self.busy || self.pending_discard.is_some() || self.pending_format_save.is_some() || self.pending_overwrite.is_some()
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Edit(action) if !self.controls_blocked() => {
                let edited = action.is_edit();
                self.dirty |= edited;
                self.content.perform(action);
                if edited {
                    self.refresh_content_stats();
                }
            }
            Message::TargetChanged(value) if !self.controls_blocked() => {
                self.target_input = value;
            }
            Message::TogglePreview if !self.controls_blocked() => {
                self.show_preview = !self.show_preview;
                self.preview_notice = None;
                if self.show_preview {
                    self.refresh_preview();
                } else {
                    self.preview_items.clear();
                }
            }
            Message::PreviewLink(url) => {
                self.preview_notice = Some(format!("Tautan tidak dibuka otomatis: {url}"));
            }
            Message::NewDraft | Message::Open if !self.controls_blocked() => {
                let action = if matches!(message, Message::NewDraft) {
                    PendingAction::NewDraft
                } else {
                    PendingAction::Open
                };
                if self.dirty {
                    self.pending_discard = Some(action);
                } else {
                    return self.perform_action(action);
                }
            }
            Message::ConfirmDiscard if !self.busy => {
                if let Some(action) = self.pending_discard.take() {
                    return self.perform_action(action);
                }
            }
            Message::CancelDiscard if !self.busy => self.pending_discard = None,
            Message::Opened(result) => {
                self.busy = false;
                match result {
                    Ok(Some((path, contents))) => {
                        self.path = Some(path);
                        self.content = text_editor::Content::with_text(&contents);
                        self.saved_contents = Some(contents);
                        self.target_input.clear();
                        self.preview_notice = None;
                        self.refresh_content_stats();
                        self.dirty = false;
                        self.error = None;
                    }
                    Ok(None) => {}
                    Err(error) => self.error = Some(error),
                }
            }
            Message::Save | Message::SaveAs if !self.controls_blocked() => {
                let save_as = matches!(message, Message::SaveAs);
                self.error = None;
                if self.dirty {
                    self.pending_format_save = Some(save_as);
                } else {
                    return self.start_save(save_as);
                }
            }
            Message::ConfirmFormatSave if !self.busy => {
                if let Some(save_as) = self.pending_format_save.take() {
                    return self.start_save(save_as);
                }
            }
            Message::CancelFormatSave if !self.busy => self.pending_format_save = None,
            Message::SavePrepared(result) => {
                self.busy = false;
                match result {
                    Ok(Some(prepared)) if prepared.requires_overwrite => self.pending_overwrite = Some(prepared),
                    Ok(Some(prepared)) => return self.start_prepared_save(prepared),
                    Ok(None) => {}
                    Err(error) => self.error = Some(error),
                }
            }
            Message::ConfirmOverwrite if !self.busy => {
                if let Some(prepared) = self.pending_overwrite.take() {
                    return self.start_prepared_save(prepared);
                }
            }
            Message::CancelOverwrite if !self.busy => self.pending_overwrite = None,
            Message::Saved(result) => {
                self.busy = false;
                match result {
                    Ok(Some((path, contents))) => {
                        self.path = Some(path);
                        self.saved_contents = Some(contents);
                        self.dirty = false;
                        self.error = None;
                    }
                    Ok(None) => {}
                    Err(error) => self.error = Some(error),
                }
            }
            _ => {}
        }
        Task::none()
    }

    fn start_save(&mut self, save_as: bool) -> Task<Message> {
        self.busy = true;
        Task::perform(
            prepare_save(self.path.clone(), self.saved_contents.clone(), self.contents_to_save(), save_as),
            Message::SavePrepared,
        )
    }

    fn start_prepared_save(&mut self, prepared: PreparedSave) -> Task<Message> {
        self.busy = true;
        Task::perform(write_prepared(prepared), Message::Saved)
    }

    fn contents_to_save(&self) -> String {
        if !self.dirty {
            // Iced's Content::text() always adds a trailing newline and normalizes
            // line endings. Preserve the original bytes when no edit was made.
            return self.saved_contents.clone().unwrap_or_default();
        }
        self.content.text()
    }

    fn syntax_token(&self) -> &'static str {
        match self.path.as_deref() {
            None => "md",
            Some(path) if path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| {
                ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown")
            }) => "md",
            Some(_) => "txt",
        }
    }

    fn refresh_content_stats(&mut self) {
        // Iced joins lines with whitespace in Content::text(); counting each line
        // is equivalent without allocating a second copy of the entire draft.
        self.word_count = self.content.lines().map(|line| line.split_whitespace().count()).sum();
        if self.show_preview {
            self.refresh_preview();
        }
    }

    fn refresh_preview(&mut self) {
        self.preview_items = markdown::parse(&self.content.text()).collect();
    }

    fn perform_action(&mut self, action: PendingAction) -> Task<Message> {
        match action {
            PendingAction::NewDraft => {
                *self = Self::default();
                Task::none()
            }
            PendingAction::Open => {
                self.busy = true;
                self.error = None;
                Task::perform(open_file(), Message::Opened)
            }
        }
    }

    pub fn view<'a>(&'a self, theme: &Theme) -> Element<'a, Message> {
        let mut new_draft = button("Draft baru");
        let mut open = button("Buka");
        let mut save = button("Simpan");
        let mut save_as = button("Simpan sebagai");
        let mut preview = button(if self.show_preview { "Tutup preview" } else { "Preview" });
        if !self.controls_blocked() {
            new_draft = new_draft.on_press(Message::NewDraft);
            open = open.on_press(Message::Open);
            save = save.on_press(Message::Save);
            save_as = save_as.on_press(Message::SaveAs);
            preview = preview.on_press(Message::TogglePreview);
        }

        let filename = self
            .path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "Draft baru".to_owned());
        let status = if self.dirty { " • belum disimpan" } else { "" };
        let words = self.word_count;

        let mut editor = text_editor(&self.content)
            .height(Length::Fill)
            .highlight(self.syntax_token(), highlighter::Theme::Base16Ocean)
            .key_binding(editor_key_binding);
        if !self.controls_blocked() {
            editor = editor.on_action(Message::Edit);
        }

        let mut view = column![
            text("Writer").size(32),
            row![new_draft, open, save, save_as, preview].spacing(12),
            text("Ctrl+N: baru · Ctrl+O: buka · Ctrl+S: simpan · Ctrl+Shift+S: simpan sebagai").size(12),
            text(format!("{filename}{status}")),
        ]
        .spacing(12)
        .height(Length::Fill);

        if let Some(action) = self.pending_discard {
            let (question, confirm_label) = match action {
                PendingAction::NewDraft => (
                    "Perubahan belum disimpan. Buang perubahan dan buat draft baru?",
                    "Buang & buat baru",
                ),
                PendingAction::Open => (
                    "Perubahan belum disimpan. Buang perubahan dan buka file lain?",
                    "Buang & buka",
                ),
            };
            view = view.push(text(question));
            view = view.push(
                row![
                    button(confirm_label).on_press(Message::ConfirmDiscard),
                    button("Batal").on_press(Message::CancelDiscard),
                ]
                .spacing(12),
            );
        }
        if self.pending_format_save.is_some() {
            view = view.push(text(
                "Editor dapat mengubah format baris saat menyimpan: CRLF menjadi LF, newline akhir ditambahkan, atau baris kosong akhir berkurang. Periksa file hasil simpan. Lanjutkan?",
            ));
            let original = match &self.saved_contents {
                Some(contents) => format!("Versi tersimpan: {}", line_endings_summary(contents)),
                None => "Versi tersimpan: tidak ada (draft baru)".to_owned(),
            };
            view = view.push(text(original).size(14));
            view = view.push(text(format!("Hasil simpan: {}", line_endings_summary(&self.contents_to_save()))).size(14));
            view = view.push(text("Perbedaan juga dapat berasal dari edit Anda, bukan hanya normalisasi.").size(12));
            view = view.push(
                row![
                    button("Lanjutkan simpan").on_press(Message::ConfirmFormatSave),
                    button("Batal").on_press(Message::CancelFormatSave),
                ]
                .spacing(12),
            );
        }
        if let Some(prepared) = &self.pending_overwrite {
            view = view.push(text("File tujuan sudah ada. Ganti isi file ini dengan draft?"));
            view = view.push(text(prepared.path.display().to_string()).size(14));
            view = view.push(row![
                button("Ganti file").on_press(Message::ConfirmOverwrite),
                button("Batal").on_press(Message::CancelOverwrite),
            ].spacing(12));
        }
        if let Some(error) = &self.error {
            view = view.push(text(error));
        }
        if self.busy {
            view = view.push(text("Memproses file..."));
        }
        if let Some(notice) = &self.preview_notice {
            view = view.push(text(notice).size(14));
        }

        let editing_area: Element<'_, Message> = if self.show_preview {
            let preview_content = markdown::view(
                &self.preview_items,
                markdown::Settings::with_text_size(16),
                markdown::Style::from_palette(theme.palette()),
            )
            .map(Message::PreviewLink);
            let preview_pane = column![
                text("Preview Markdown").size(16),
                scrollable(container(preview_content).padding(12).width(Length::Fill))
                    .height(Length::Fill),
            ]
            .spacing(8)
            .height(Length::Fill)
            .width(Length::FillPortion(1));
            row![container(editor).width(Length::FillPortion(1)).height(Length::Fill), preview_pane]
                .spacing(12)
                .height(Length::Fill)
                .into()
        } else {
            editor.into()
        };
        let mut target_input = text_input("mis. 50000", &self.target_input).width(140);
        if !self.controls_blocked() {
            target_input = target_input.on_input(Message::TargetChanged);
        }
        view = view.push(editing_area).push(
            row![text(format!("{words} kata")), text("Target draft:"), target_input]
                .spacing(12),
        );
        if let Some(target) = word_target(&self.target_input) {
            let progress = target_progress(words, target);
            view = view.push(
                row![
                    progress_bar(0.0..=1.0, progress).width(Length::Fill),
                    text(format!("{words} / {target} kata")),
                ].spacing(12),
            );
        } else if !self.target_input.is_empty() {
            view = view.push(text("Target harus berupa angka 1–1.000.000.000.").size(14));
        }
        view.into()
    }
}

// Count exactly the line endings recognized by the editor, including mixed formats.
fn line_endings_summary(contents: &str) -> String {
    let bytes = contents.as_bytes();
    let (mut lf, mut crlf, mut cr, mut lfcr, mut trailing) = (0_usize, 0_usize, 0_usize, 0_usize, 0_usize);
    let mut index = 0;
    while index < bytes.len() {
        match (bytes[index], bytes.get(index + 1).copied()) {
            (b'\r', Some(b'\n')) => { crlf += 1; trailing += 1; index += 2; }
            (b'\n', Some(b'\r')) => { lfcr += 1; trailing += 1; index += 2; }
            (b'\n', _) => { lf += 1; trailing += 1; index += 1; }
            (b'\r', _) => { cr += 1; trailing += 1; index += 1; }
            _ => { trailing = 0; index += 1; }
        }
    }
    format!("LF {lf}, CRLF {crlf}, CR {cr}, LFCR {lfcr}; pemisah di akhir {trailing}")
}

fn word_target(input: &str) -> Option<usize> {
    if input.is_empty() || !input.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    input.parse::<usize>().ok().filter(|target| (1..=1_000_000_000).contains(target))
}

fn target_progress(words: usize, target: usize) -> f32 {
    // A target comes from word_target, so it is always positive.
    words.min(target) as f32 / target as f32
}

fn editor_key_binding(press: text_editor::KeyPress) -> Option<text_editor::Binding<Message>> {
    if press.status == text_editor::Status::Focused {
        if let Some(shortcut) = crate::shortcuts::from_key(press.key.as_ref(), press.modifiers) {
            // Reader-only keys must keep the editor's normal page movement.
            if !matches!(shortcut, crate::shortcuts::Shortcut::PreviousSection | crate::shortcuts::Shortcut::NextSection) {
                return Some(text_editor::Binding::Custom(Message::Shortcut(shortcut)));
            }
        }
    }
    text_editor::Binding::from_key_press(press)
}

async fn open_file() -> Result<Option<(PathBuf, String)>, String> {
    let Some(file) = rfd::AsyncFileDialog::new()
        .add_filter("Teks dan Markdown", &["txt", "md", "markdown"])
        .pick_file()
        .await
    else {
        return Ok(None);
    };
    let path = file.path().to_owned();
    let contents = tokio::fs::read_to_string(&path)
        .await
        .map_err(|error| format!("Gagal membuka file: {error}"))?;
    Ok(Some((path, contents)))
}

async fn prepare_save(
    current_path: Option<PathBuf>,
    saved_contents: Option<String>,
    contents: String,
    save_as: bool,
) -> Result<Option<PreparedSave>, String> {
    let path = if !save_as && current_path.is_some() {
        current_path.clone().unwrap()
    } else {
        let Some(file) = rfd::AsyncFileDialog::new()
            .add_filter("Markdown", &["md"])
            .add_filter("Teks", &["txt"])
            .set_file_name("draft.md")
            .save_file()
            .await
        else {
            return Ok(None);
        };
        file.path().to_owned()
    };

    prepare_destination(current_path, saved_contents, contents, path).await.map(Some)
}

async fn prepare_destination(
    current_path: Option<PathBuf>,
    saved_contents: Option<String>,
    contents: String,
    path: PathBuf,
) -> Result<PreparedSave, String> {
    let same_file = match &current_path {
        Some(current) => is_same_file(current, &path).await,
        None => false,
    };
    let expected = if same_file {
        let saved = saved_contents.ok_or_else(|| "Status file asli tidak diketahui; gunakan Simpan sebagai untuk memilih file lain.".to_owned())?;
        FileState::Present(saved.into_bytes())
    } else {
        read_file_state(&path).await.map_err(|error| format!("Gagal memeriksa file tujuan: {error}"))?
    };
    let requires_overwrite = !same_file && matches!(&expected, FileState::Present(_));
    let expected_target = if requires_overwrite {
        Some(tokio::fs::canonicalize(&path).await.map_err(|error| format!("Gagal memeriksa tujuan file: {error}"))?)
    } else {
        None
    };
    Ok(PreparedSave { path, contents, expected, expected_target, requires_overwrite })
}

async fn write_prepared(prepared: PreparedSave) -> Result<Option<(PathBuf, String)>, String> {
    if let Some(expected_target) = &prepared.expected_target {
        let actual_target = tokio::fs::canonicalize(&prepared.path).await
            .map_err(|error| format!("Gagal memeriksa tujuan file: {error}"))?;
        if &actual_target != expected_target {
            return Err("Tujuan file berubah setelah konfirmasi; penyimpanan dibatalkan.".into());
        }
    }
    atomic_write_checked(&prepared.path, prepared.contents.as_bytes(), Some(&prepared.expected))
        .await
        .map_err(|error| format!("Gagal menyimpan file: {error}"))?;
    Ok(Some((prepared.path, prepared.contents)))
}

async fn is_same_file(current: &Path, destination: &Path) -> bool {
    if current == destination {
        return true;
    }
    match (tokio::fs::canonicalize(current).await, tokio::fs::canonicalize(destination).await) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FileState {
    Missing,
    Present(Vec<u8>),
}

async fn read_file_state(path: &Path) -> std::io::Result<FileState> {
    match tokio::fs::read(path).await {
        Ok(contents) => Ok(FileState::Present(contents)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(FileState::Missing),
        Err(error) => Err(error),
    }
}

async fn check_file_state(path: &Path, expected: &FileState) -> std::io::Result<()> {
    if &read_file_state(path).await? != expected {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "File berubah atau terhapus di luar NovelCraft. Gunakan Simpan sebagai untuk menyimpan draft ke file lain.",
        ));
    }
    Ok(())
}

// Keep the old file intact until the complete replacement is ready. Follow an
// existing symlink rather than replacing the link itself.
async fn atomic_write_checked(path: &Path, contents: &[u8], expected: Option<&FileState>) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind};
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    if let Some(expected) = expected {
        check_file_state(path, expected).await?;
    }
    let (destination, followed_symlink) = match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) if metadata.file_type().is_symlink() => (tokio::fs::canonicalize(path).await?, true),
        Ok(_) => (path.to_owned(), false),
        Err(error) if error.kind() == ErrorKind::NotFound => (path.to_owned(), false),
        Err(error) => return Err(error),
    };
    let original = match tokio::fs::metadata(&destination).await {
        Ok(metadata) => {
            if !metadata.is_file() {
                return Err(Error::new(ErrorKind::InvalidInput, "Tujuan bukan file biasa"));
            }
            // Replacing an inode would silently break other hard links to it.
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if metadata.nlink() > 1 {
                    return Err(Error::new(ErrorKind::Unsupported, "File memiliki hard link; penyimpanan atomik tidak didukung"));
                }
            }
            // Atomic replacement should not bypass the original file's write permissions.
            tokio::fs::OpenOptions::new().write(true).open(&destination).await?;
            Some(metadata)
        }
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };

    let dir = destination.parent().filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let (temp, mut file) = loop {
        let temp = dir.join(format!(".novelcraft-{}-{}.tmp", std::process::id(), NEXT_TEMP.fetch_add(1, Ordering::Relaxed)));
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        match options.open(&temp).await {
            Ok(file) => break (temp, file),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let result = async {
        if let Some(metadata) = &original {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let new_metadata = file.metadata().await?;
                if metadata.uid() != new_metadata.uid() || metadata.gid() != new_metadata.gid() {
                    return Err(Error::new(ErrorKind::PermissionDenied, "Pemilik atau grup file akan berubah; penyimpanan dibatalkan"));
                }
            }
        }
        file.write_all(contents).await?;
        if let Some(metadata) = original {
            file.set_permissions(metadata.permissions()).await?;
        }
        file.sync_all().await?;
        drop(file);
        if let Some(expected) = expected {
            check_file_state(path, expected).await?;
        }
        if followed_symlink && tokio::fs::canonicalize(path).await? != destination {
            return Err(Error::new(ErrorKind::Other, "Tujuan symbolic link berubah; penyimpanan dibatalkan"));
        }
        tokio::fs::rename(&temp, &destination).await
    }.await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&temp).await;
    }
    result
}

#[cfg(test)]
async fn atomic_write(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    atomic_write_checked(path, contents, None).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarizes_mixed_line_endings_and_trailing_separators() {
        assert_eq!(line_endings_summary(""), "LF 0, CRLF 0, CR 0, LFCR 0; pemisah di akhir 0");
        assert_eq!(line_endings_summary("halo"), "LF 0, CRLF 0, CR 0, LFCR 0; pemisah di akhir 0");
        assert_eq!(line_endings_summary("é\nA\r\nB\rC\n\rD\n\n"),
            "LF 3, CRLF 1, CR 1, LFCR 1; pemisah di akhir 2");
        assert_eq!(line_endings_summary("A\r\n\r"), "LF 0, CRLF 1, CR 1, LFCR 0; pemisah di akhir 2");
        assert_eq!(line_endings_summary("A\n\r"), "LF 0, CRLF 0, CR 0, LFCR 1; pemisah di akhir 1");
    }

    #[test]
    fn format_summary_compares_saved_bytes_to_editor_output_without_modification() {
        let mut writer = Writer {
            content: text_editor::Content::with_text("satu\r\ndua\r\n\r\n"),
            saved_contents: Some("satu\r\ndua\r\n\r\n".into()),
            dirty: true,
            ..Writer::default()
        };
        let _ = writer.update(Message::Save);
        let original = writer.saved_contents.clone().unwrap();
        let output = writer.contents_to_save();
        assert_eq!(line_endings_summary(&original), "LF 0, CRLF 3, CR 0, LFCR 0; pemisah di akhir 2");
        assert!(line_endings_summary(&output).contains("CRLF 0"));
        drop(writer.view(&Theme::TokyoNight));
        assert_eq!(writer.contents_to_save(), output);
        assert_eq!(writer.saved_contents.as_deref(), Some(original.as_str()));
        assert!(writer.dirty);
        assert!(!writer.busy);
    }

    #[test]
    fn edited_save_requires_format_confirmation_and_cancel_keeps_draft() {
        let mut writer = Writer {
            content: text_editor::Content::with_text("satu\r\ndua\r\n\r\n"),
            path: Some("draft.txt".into()),
            saved_contents: Some("satu\r\ndua\r\n\r\n".into()),
            dirty: true,
            ..Writer::default()
        };
        let before = writer.content.text();
        let _ = writer.update(Message::Save);
        assert_eq!(writer.pending_format_save, Some(false));
        assert!(!writer.busy);
        let _ = writer.update(Message::NewDraft);
        let _ = writer.update(Message::SaveAs);
        let _ = writer.update(Message::Edit(text_editor::Action::Edit(text_editor::Edit::Insert('x'))));
        assert_eq!(writer.pending_format_save, Some(false));
        assert_eq!(writer.content.text(), before);
        assert_eq!(writer.pending_discard, None);
        let _ = writer.update(Message::CancelFormatSave);
        assert!(writer.pending_format_save.is_none());
        assert!(writer.dirty);
        assert_eq!(writer.saved_contents.as_deref(), Some("satu\r\ndua\r\n\r\n"));
        let _ = writer.update(Message::SaveAs);
        assert_eq!(writer.pending_format_save, Some(true));
        let _ = writer.update(Message::ConfirmFormatSave);
        assert!(writer.pending_format_save.is_none());
        assert!(writer.busy);
        assert!(writer.dirty);
        let _ = writer.update(Message::Saved(Ok(None)));
        assert!(!writer.busy);
        assert!(writer.dirty);
        assert_eq!(writer.content.text(), before);
    }

    #[test]
    fn clean_save_skips_format_confirmation() {
        let mut writer = Writer::default();
        let _ = writer.update(Message::SaveAs);
        assert!(writer.pending_format_save.is_none());
        assert!(writer.busy);
        let _ = writer.update(Message::Saved(Ok(None)));
        assert!(!writer.busy);
    }

    #[test]
    fn fullscreen_does_not_bypass_writer_save_confirmation() {
        let mut app = crate::NovelCraft::default();
        let _ = app.update(crate::Message::Navigate(crate::Page::Writer));
        let _ = app.update(crate::Message::Writer(Message::Edit(text_editor::Action::Edit(text_editor::Edit::Insert('x')))));
        let _ = app.update(crate::Message::Writer(Message::Save));
        assert_eq!(app.writer.pending_format_save, Some(false));
        let _ = app.update(crate::Message::ToggleFullscreen);
        let revision = app.fullscreen_revision;
        let id = iced::window::Id::unique();
        let _ = app.update(crate::Message::FullscreenWindowReady(revision, Some(id)));
        let _ = app.update(crate::Message::FullscreenModeReady(revision, id, iced::window::Mode::Windowed));
        assert_eq!(app.writer.pending_format_save, Some(false));
        assert!(app.writer.dirty);
        let _ = app.update(crate::Message::Writer(Message::CancelFormatSave));
        assert!(app.writer.pending_format_save.is_none());
        assert!(app.writer.dirty);
    }

    #[test]
    fn clean_save_preserves_original_line_endings_and_empty_new_draft() {
        let mut writer = Writer::default();
        assert_eq!(writer.contents_to_save(), "");
        for original in ["", "tanpa newline", "satu\ndua\n", "satu\r\ndua\r\n", "satu\rdua"] {
            let _ = writer.update(Message::Opened(Ok(Some(("/draft.txt".into(), original.into())))));
            assert_eq!(writer.contents_to_save(), original);
            assert!(!writer.dirty);
        }
        let _ = writer.update(Message::Edit(text_editor::Action::Edit(text_editor::Edit::Insert('x'))));
        assert!(writer.dirty);
        assert_eq!(writer.contents_to_save(), writer.content.text());
    }

    #[tokio::test]
    async fn saving_unchanged_files_keeps_exact_bytes_and_conflict_protection() {
        let dir = temporary_directory();
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("draft.txt");
        for original in ["", "tanpa newline", "satu\r\ndua\r\n"] {
            tokio::fs::write(&path, original).await.unwrap();
            let mut writer = Writer::default();
            let _ = writer.update(Message::Opened(Ok(Some((path.clone(), original.into())))));
            let contents = writer.contents_to_save();
            save_known_file(writer.path.clone(), writer.saved_contents.clone(), contents).await.unwrap();
            assert_eq!(tokio::fs::read(&path).await.unwrap(), original.as_bytes().to_vec());
            tokio::fs::write(&path, "perubahan eksternal").await.unwrap();
            let result = save_known_file(writer.path.clone(), writer.saved_contents.clone(), writer.contents_to_save()).await;
            assert!(result.is_err());
            assert_eq!(tokio::fs::read(&path).await.unwrap(), b"perubahan eksternal".to_vec());
        }
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[test]
    fn cached_word_count_matches_editor_text_through_document_lifecycle() {
        use text_editor::{Action, Edit, Motion};
        use std::sync::Arc;
        let mut writer = Writer::default();
        assert_eq!(writer.word_count, 0);
        for source in ["", "satu", "dua  kata\r\nbaris lain\r\n", "  α\u{2003}β\n\n 🌙 ", "a\rb\n\rc"] {
            let _ = writer.update(Message::Opened(Ok(Some(("/draft.md".into(), source.into())))));
            assert_eq!(writer.word_count, writer.content.text().split_whitespace().count());
            assert_eq!(writer.contents_to_save(), source);
            let _ = writer.update(Message::Edit(Action::Move(Motion::DocumentEnd)));
            assert_eq!(writer.word_count, writer.content.text().split_whitespace().count());
            assert!(!writer.dirty);
            drop(writer.view(&Theme::TokyoNight));
        }
        let _ = writer.update(Message::TogglePreview);
        for action in [
            Action::Edit(Edit::Paste(Arc::new(" 🌙 kata baru\nbaris kedua".into()))),
            Action::Edit(Edit::Enter),
            Action::Edit(Edit::Backspace),
            Action::SelectAll,
            Action::Edit(Edit::Paste(Arc::new("ganti semua  α\u{2003}β".into()))),
        ] {
            let _ = writer.update(Message::Edit(action));
            assert_eq!(writer.word_count, writer.content.text().split_whitespace().count());
            assert!(writer.show_preview);
        }
        assert!(writer.dirty);
        let count = writer.word_count;
        let output = writer.contents_to_save();
        let _ = writer.update(Message::Save);
        let _ = writer.update(Message::Edit(Action::Edit(Edit::Insert('x'))));
        assert_eq!(writer.word_count, count);
        assert_eq!(writer.contents_to_save(), output);
        let _ = writer.update(Message::CancelFormatSave);
        let _ = writer.update(Message::NewDraft);
        let _ = writer.update(Message::ConfirmDiscard);
        assert_eq!(writer.word_count, 0);
        assert!(writer.content.text().split_whitespace().next().is_none());
    }

    #[test]
    fn cached_word_count_handles_large_draft_and_reopening() {
        let mut writer = Writer::default();
        let contents = "kata α β\n".repeat(5000);
        let _ = writer.update(Message::Opened(Ok(Some(("/large.txt".into(), contents)))));
        assert_eq!(writer.word_count, 15_000);
        for _ in 0..5 { drop(writer.view(&Theme::TokyoNight)); }
        assert_eq!(writer.word_count, 15_000);
        let _ = writer.update(Message::Opened(Ok(Some(("/empty.txt".into(), String::new())))));
        assert_eq!(writer.word_count, 0);
    }

    #[test]
    fn word_targets_validate_and_clamp_progress() {
        assert_eq!(word_target(""), None);
        assert_eq!(word_target("0"), None);
        assert_eq!(word_target("-4"), None);
        assert_eq!(word_target("+4"), None);
        assert_eq!(word_target("1.5"), None);
        assert_eq!(word_target("1000000001"), None);
        assert_eq!(word_target("999999999999999999999999999999"), None);
        assert_eq!(word_target("500"), Some(500));
        assert_eq!(target_progress(0, 4), 0.0);
        assert_eq!(target_progress(2, 4), 0.5);
        assert_eq!(target_progress(4, 4), 1.0);
        assert_eq!(target_progress(8, 4), 1.0);
    }

    #[test]
    fn target_does_not_modify_draft_and_resets_only_when_document_changes() {
        let mut writer = Writer {
            content: text_editor::Content::with_text("dua kata"),
            path: Some("draft.md".into()),
            saved_contents: Some("dua kata".into()),
            ..Writer::default()
        };
        let before = writer.content.text();
        let _ = writer.update(Message::TargetChanged("100".into()));
        assert_eq!(writer.content.text(), before);
        assert_eq!(writer.saved_contents.as_deref(), Some("dua kata"));
        assert!(!writer.dirty);
        let _ = writer.update(Message::Open);
        let _ = writer.update(Message::TargetChanged("200".into()));
        assert_eq!(writer.target_input, "100");
        let _ = writer.update(Message::Opened(Ok(None)));
        assert_eq!(writer.target_input, "100");
        let _ = writer.update(Message::Opened(Err("gagal".into())));
        assert_eq!(writer.target_input, "100");
        let _ = writer.update(Message::Saved(Ok(Some(("lain.md".into(), "dua kata".into())))));
        assert_eq!(writer.target_input, "100");
        let _ = writer.update(Message::Opened(Ok(Some(("baru.md".into(), "baru".into())))));
        assert!(writer.target_input.is_empty());
        let _ = writer.update(Message::TargetChanged("20".into()));
        let _ = writer.update(Message::NewDraft);
        assert!(writer.target_input.is_empty());
    }

    #[test]
    fn target_changes_are_ignored_during_discard_confirmation() {
        let mut writer = Writer { dirty: true, target_input: "25".into(), ..Writer::default() };
        let _ = writer.update(Message::NewDraft);
        let _ = writer.update(Message::TargetChanged("40".into()));
        assert_eq!(writer.target_input, "25");
        let _ = writer.update(Message::CancelDiscard);
        assert_eq!(writer.target_input, "25");
        assert!(writer.dirty);
    }

    #[test]
    fn highlighting_tracks_draft_open_and_save_as_without_editing_content() {
        let mut writer = Writer::default();
        assert_eq!(writer.syntax_token(), "md");
        let _ = writer.update(Message::Opened(Ok(Some(("/novel.TXT".into(), "# Teks".into())))));
        assert_eq!(writer.syntax_token(), "txt");
        assert_eq!(writer.content.text().trim_end(), "# Teks");
        assert!(!writer.dirty);
        let _ = writer.update(Message::Saved(Ok(Some(("/novel.MARKDOWN".into(), "# Teks".into())))));
        assert_eq!(writer.syntax_token(), "md");
        assert!(!writer.dirty);
        let _ = writer.update(Message::Saved(Ok(Some(("/novel.other".into(), "# Teks".into())))));
        assert_eq!(writer.syntax_token(), "txt");
        let _ = writer.update(Message::NewDraft);
        assert_eq!(writer.syntax_token(), "md");
    }

    #[test]
    fn preview_toggles_without_changing_the_draft_or_save_state() {
        let mut writer = Writer {
            content: text_editor::Content::with_text("# Judul\n\nTeks **tebal**"),
            path: Some(PathBuf::from("draft.md")),
            saved_contents: Some("versi lama".into()),
            dirty: true,
            ..Writer::default()
        };
        let before = writer.content.text();
        let _ = writer.update(Message::TogglePreview);
        assert!(writer.show_preview);
        assert!(matches!(writer.preview_items.first(), Some(markdown::Item::Heading(..))));
        for theme in [Theme::TokyoNight, Theme::Dark, Theme::Nord, Theme::Dracula] {
            drop(writer.view(&theme));
        }
        assert_eq!(writer.content.text(), before);
        assert_eq!(writer.saved_contents.as_deref(), Some("versi lama"));
        assert!(writer.dirty);
        let _ = writer.update(Message::TogglePreview);
        assert!(!writer.show_preview);
        assert!(writer.preview_items.is_empty());
        assert_eq!(writer.content.text(), before);
    }

    #[test]
    fn preview_refreshes_when_editor_changes_and_when_a_file_is_opened() {
        use iced::widget::text_editor::{Action, Edit};
        use std::sync::Arc;
        let mut writer = Writer::default();
        let _ = writer.update(Message::TogglePreview);
        let _ = writer.update(Message::Edit(Action::Edit(Edit::Paste(Arc::new("# Judul\n\nIsi".into())))));
        assert!(writer.dirty);
        assert!(matches!(writer.preview_items.first(), Some(markdown::Item::Heading(..))));
        let _ = writer.update(Message::Opened(Ok(Some((PathBuf::from("lain.md"), "- satu\n- dua".into())))));
        assert!(writer.show_preview);
        assert!(matches!(writer.preview_items.first(), Some(markdown::Item::List { .. })));
        assert!(!writer.dirty);
    }

    #[test]
    fn preview_does_not_open_links_and_respects_confirmation() {
        let mut writer = Writer { dirty: true, ..Writer::default() };
        let _ = writer.update(Message::NewDraft);
        let _ = writer.update(Message::TogglePreview);
        assert!(!writer.show_preview);
        assert_eq!(writer.pending_discard, Some(PendingAction::NewDraft));
        let _ = writer.update(Message::CancelDiscard);
        let _ = writer.update(Message::TogglePreview);
        let url = markdown::Url::parse("https://example.com/").unwrap();
        let _ = writer.update(Message::PreviewLink(url));
        assert!(writer.preview_notice.as_deref().unwrap().contains("https://example.com/"));
        assert!(writer.dirty);
    }

    #[test]
    fn editor_shortcut_binding_handles_commands_and_preserves_copy() {
        use iced::keyboard::{Key, Modifiers};
        let press = |key: &str, modifiers| text_editor::KeyPress {
            key: Key::Character(key.into()),
            modifiers,
            text: None,
            status: text_editor::Status::Focused,
        };
        assert!(matches!(
            editor_key_binding(press("s", Modifiers::CTRL)),
            Some(text_editor::Binding::Custom(Message::Shortcut(crate::shortcuts::Shortcut::Save)))
        ));
        assert!(matches!(
            editor_key_binding(press("c", Modifiers::CTRL)),
            Some(text_editor::Binding::Copy)
        ));
        for key in [iced::keyboard::key::Named::PageUp, iced::keyboard::key::Named::PageDown] {
            let page_press = text_editor::KeyPress {
                key: Key::Named(key), modifiers: Modifiers::CTRL, text: None,
                status: text_editor::Status::Focused,
            };
            assert!(matches!(editor_key_binding(page_press), Some(text_editor::Binding::Move(_))));
        }
        let mut unfocused = press("s", Modifiers::CTRL);
        unfocused.status = text_editor::Status::Active;
        assert!(editor_key_binding(unfocused).is_none());
    }

    #[test]
    fn new_draft_clears_a_saved_document_without_touching_its_file() {
        let mut writer = Writer {
            content: text_editor::Content::with_text("sudah disimpan"),
            path: Some(PathBuf::from("existing.md")),
            saved_contents: Some("sudah disimpan".into()),
            error: Some("pesan lama".into()),
            ..Writer::default()
        };
        let _ = writer.update(Message::NewDraft);
        assert!(writer.content.text().trim().is_empty());
        assert_eq!(writer.path, None);
        assert_eq!(writer.saved_contents, None);
        assert_eq!(writer.pending_discard, None);
        assert_eq!(writer.error, None);
        assert!(!writer.dirty);
        assert!(!writer.busy);
    }

    #[test]
    fn new_draft_requires_confirmation_and_cancel_preserves_document() {
        let mut writer = Writer {
            content: text_editor::Content::with_text("draft belum disimpan"),
            path: Some(PathBuf::from("original.md")),
            saved_contents: Some("versi lama".into()),
            dirty: true,
            ..Writer::default()
        };
        let _ = writer.update(Message::NewDraft);
        assert_eq!(writer.pending_discard, Some(PendingAction::NewDraft));
        let _ = writer.update(Message::CancelDiscard);
        assert_eq!(writer.pending_discard, None);
        assert_eq!(writer.content.text().trim_end(), "draft belum disimpan");
        assert_eq!(writer.path.as_deref(), Some(Path::new("original.md")));
        assert_eq!(writer.saved_contents.as_deref(), Some("versi lama"));
        assert!(writer.dirty);

        let _ = writer.update(Message::NewDraft);
        let _ = writer.update(Message::ConfirmDiscard);
        assert!(writer.content.text().trim().is_empty());
        assert_eq!(writer.path, None);
        assert_eq!(writer.saved_contents, None);
        assert_eq!(writer.pending_discard, None);
        assert!(!writer.dirty);
    }

    #[test]
    fn new_draft_is_ignored_while_busy_or_awaiting_confirmation() {
        let mut writer = Writer { dirty: true, busy: true, ..Writer::default() };
        let _ = writer.update(Message::NewDraft);
        assert_eq!(writer.pending_discard, None);
        writer.busy = false;
        let _ = writer.update(Message::Open);
        let _ = writer.update(Message::NewDraft);
        assert_eq!(writer.pending_discard, Some(PendingAction::Open));
        assert!(writer.dirty);
    }

    #[test]
    fn cancel_file_picker_after_discard_confirmation_keeps_draft() {
        let mut writer = Writer {
            content: text_editor::Content::with_text("draft lama"),
            dirty: true,
            ..Writer::default()
        };
        let _ = writer.update(Message::Open);
        let _ = writer.update(Message::ConfirmDiscard);
        assert!(writer.busy);
        let _ = writer.update(Message::Opened(Ok(None)));
        assert!(!writer.busy);
        assert!(writer.dirty);
        assert_eq!(writer.content.text().trim_end(), "draft lama");
    }

    #[test]
    fn opening_dirty_document_requires_confirmation() {
        let mut writer = Writer { dirty: true, ..Writer::default() };
        let _ = writer.update(Message::Open);
        assert_eq!(writer.pending_discard, Some(PendingAction::Open));
        assert!(!writer.busy);
        let _ = writer.update(Message::CancelDiscard);
        assert!(writer.dirty);
        assert_eq!(writer.pending_discard, None);
    }

    #[test]
    fn cancelled_or_failed_open_preserves_document() {
        let mut writer = Writer {
            content: text_editor::Content::with_text("draft lama"),
            dirty: true,
            busy: true,
            ..Writer::default()
        };
        let _ = writer.update(Message::Opened(Ok(None)));
        assert_eq!(writer.content.text().trim_end(), "draft lama");
        assert!(writer.dirty);
        let _ = writer.update(Message::Opened(Err("gagal".to_owned())));
        assert_eq!(writer.content.text().trim_end(), "draft lama");
        assert!(writer.dirty);
        assert_eq!(writer.error.as_deref(), Some("gagal"));
    }

    #[test]
    fn successful_open_and_save_update_state() {
        let mut writer = Writer::default();
        let path = PathBuf::from("draft.md");
        let _ = writer.update(Message::Opened(Ok(Some((path.clone(), "dua kata".into())))));
        assert_eq!(writer.content.text().split_whitespace().count(), 2);
        assert_eq!(writer.path.as_ref(), Some(&path));
        assert_eq!(writer.saved_contents.as_deref(), Some("dua kata"));
        writer.dirty = true;
        let _ = writer.update(Message::Saved(Ok(Some((path, "dua kata".into())))));
        assert!(!writer.dirty);
        assert_eq!(writer.saved_contents.as_deref(), Some("dua kata"));
    }

    #[tokio::test]
    async fn saves_text_to_an_existing_path_without_a_dialog() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("novelcraft-{}-{unique}.md", std::process::id()));
        atomic_write(&path, b"Halo dunia").await.unwrap();
        assert_eq!(tokio::fs::read_to_string(&path).await.unwrap(), "Halo dunia");
        tokio::fs::remove_file(path).await.unwrap();
    }

    async fn save_known_file(current: Option<PathBuf>, saved: Option<String>, contents: String) -> Result<Option<(PathBuf, String)>, String> {
        let path = current.clone().unwrap();
        let prepared = prepare_destination(current, saved, contents, path).await?;
        assert!(!prepared.requires_overwrite);
        write_prepared(prepared).await
    }

    fn temporary_directory() -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("novelcraft-writer-{}-{unique}", std::process::id()))
    }

    #[test]
    fn cancel_or_successful_save_as_preserves_correct_state() {
        let mut writer = Writer {
            content: text_editor::Content::with_text("draft baru"),
            path: Some(PathBuf::from("original.md")),
            saved_contents: Some("versi awal".into()),
            dirty: true,
            busy: true,
            ..Writer::default()
        };
        let _ = writer.update(Message::Saved(Ok(None)));
        assert!(writer.dirty);
        assert_eq!(writer.path.as_deref(), Some(Path::new("original.md")));
        assert_eq!(writer.saved_contents.as_deref(), Some("versi awal"));
        let _ = writer.update(Message::Saved(Ok(Some((PathBuf::from("baru.md"), "draft baru".into())))));
        assert!(!writer.dirty);
        assert_eq!(writer.path.as_deref(), Some(Path::new("baru.md")));
        assert_eq!(writer.saved_contents.as_deref(), Some("draft baru"));
    }

    #[tokio::test]
    async fn save_as_existing_target_requires_confirmation_and_cancel_preserves_files() {
        let dir = temporary_directory();
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let original = dir.join("original.md");
        let target = dir.join("other.md");
        tokio::fs::write(&original, "lama").await.unwrap();
        tokio::fs::write(&target, "target awal").await.unwrap();
        let prepared = prepare_destination(Some(original.clone()), Some("lama".into()), "draft baru".into(), target.clone()).await.unwrap();
        assert!(prepared.requires_overwrite);
        let mut writer = Writer { path: Some(original.clone()), saved_contents: Some("lama".into()),
            content: text_editor::Content::with_text("draft baru"), dirty: true, busy: true, ..Writer::default() };
        let _ = writer.update(Message::SavePrepared(Ok(Some(prepared.clone()))));
        assert!(writer.pending_overwrite.is_some());
        assert!(!writer.busy);
        drop(writer.view(&Theme::TokyoNight));
        let _ = writer.update(Message::NewDraft);
        let _ = writer.update(Message::Save);
        let _ = writer.update(Message::Edit(text_editor::Action::Edit(text_editor::Edit::Insert('x'))));
        assert!(writer.pending_overwrite.is_some());
        assert!(writer.dirty);
        assert_eq!(tokio::fs::read_to_string(&target).await.unwrap(), "target awal");
        let _ = writer.update(Message::CancelOverwrite);
        assert!(writer.pending_overwrite.is_none());
        assert_eq!(writer.path.as_ref(), Some(&original));
        assert_eq!(writer.saved_contents.as_deref(), Some("lama"));
        assert!(writer.dirty);
        assert_eq!(writer.contents_to_save(), "draft baru\n");
        assert_eq!(tokio::fs::read_to_string(&target).await.unwrap(), "target awal");
        assert_eq!(tokio::fs::read_to_string(&original).await.unwrap(), "lama");
        // Confirm starts the asynchronous write; only a successful result changes Writer state.
        let _ = writer.update(Message::SavePrepared(Ok(Some(prepared.clone()))));
        let _ = writer.update(Message::ConfirmOverwrite);
        assert!(writer.busy);
        assert!(writer.pending_overwrite.is_none());
        assert!(writer.dirty);
        let result = write_prepared(prepared).await;
        let _ = writer.update(Message::Saved(result));
        assert!(!writer.busy);
        assert!(!writer.dirty);
        assert_eq!(writer.path.as_ref(), Some(&target));
        assert_eq!(tokio::fs::read_to_string(&target).await.unwrap(), "draft baru");
        assert_eq!(tokio::fs::read_to_string(&original).await.unwrap(), "lama");
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn save_as_rejects_target_changes_after_confirmation_prompt() {
        let dir = temporary_directory();
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let target = dir.join("other.md");
        tokio::fs::write(&target, "sebelum").await.unwrap();
        let prepared = prepare_destination(None, None, "draft".into(), target.clone()).await.unwrap();
        assert!(prepared.requires_overwrite);
        tokio::fs::write(&target, "perubahan luar").await.unwrap();
        let mut writer = Writer { content: text_editor::Content::with_text("draft"), dirty: true,
            busy: true, ..Writer::default() };
        let _ = writer.update(Message::Saved(write_prepared(prepared).await));
        assert!(writer.error.is_some());
        assert!(writer.dirty);
        assert_eq!(writer.contents_to_save(), "draft\n");
        assert!(writer.path.is_none());
        assert_eq!(tokio::fs::read_to_string(&target).await.unwrap(), "perubahan luar");
        let prepared = prepare_destination(None, None, "draft".into(), target.clone()).await.unwrap();
        tokio::fs::remove_file(&target).await.unwrap();
        assert!(write_prepared(prepared).await.is_err());
        assert!(!target.exists());
        let absent = dir.join("new.md");
        let prepared = prepare_destination(None, None, "draft".into(), absent.clone()).await.unwrap();
        assert!(!prepared.requires_overwrite);
        tokio::fs::write(&absent, "muncul belakangan").await.unwrap();
        assert!(write_prepared(prepared).await.is_err());
        assert_eq!(tokio::fs::read_to_string(&absent).await.unwrap(), "muncul belakangan");
        let new_target = dir.join("fresh.md");
        let prepared = prepare_destination(None, None, "draft".into(), new_target.clone()).await.unwrap();
        assert!(!prepared.requires_overwrite);
        write_prepared(prepared).await.unwrap();
        assert_eq!(tokio::fs::read_to_string(&new_target).await.unwrap(), "draft");
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn save_as_rejects_retargeted_symlink_even_when_contents_match() {
        use std::os::unix::fs::symlink;
        let dir = temporary_directory();
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let one = dir.join("one.md");
        let two = dir.join("two.md");
        let link = dir.join("target.md");
        tokio::fs::write(&one, "sama").await.unwrap();
        tokio::fs::write(&two, "sama").await.unwrap();
        symlink(&one, &link).unwrap();
        let prepared = prepare_destination(None, None, "draft".into(), link.clone()).await.unwrap();
        assert!(prepared.requires_overwrite);
        tokio::fs::remove_file(&link).await.unwrap();
        symlink(&two, &link).unwrap();
        assert!(write_prepared(prepared).await.is_err());
        assert_eq!(tokio::fs::read_to_string(&one).await.unwrap(), "sama");
        assert_eq!(tokio::fs::read_to_string(&two).await.unwrap(), "sama");
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn save_to_current_file_does_not_add_overwrite_confirmation() {
        let dir = temporary_directory();
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("draft.md");
        tokio::fs::write(&path, "lama").await.unwrap();
        let prepared = prepare_destination(Some(path.clone()), Some("lama".into()), "baru".into(), path.clone()).await.unwrap();
        assert!(!prepared.requires_overwrite);
        write_prepared(prepared).await.unwrap();
        assert_eq!(tokio::fs::read_to_string(&path).await.unwrap(), "baru");
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn refuses_external_changes_and_deletion_without_losing_draft() {
        let dir = temporary_directory();
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("draft.md");
        tokio::fs::write(&path, "versi awal").await.unwrap();
        tokio::fs::write(&path, "perubahan eksternal").await.unwrap();
        let result = save_known_file(Some(path.clone()), Some("versi awal".into()), "draft baru".into()).await;
        assert!(result.unwrap_err().contains("berubah atau terhapus"));
        assert_eq!(tokio::fs::read_to_string(&path).await.unwrap(), "perubahan eksternal");
        tokio::fs::remove_file(&path).await.unwrap();
        let result = save_known_file(Some(path.clone()), Some("versi awal".into()), "draft baru".into()).await;
        assert!(result.is_err());
        assert!(!path.exists());
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn saves_unchanged_file_and_updates_snapshot() {
        let dir = temporary_directory();
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("draft.md");
        tokio::fs::write(&path, "lama").await.unwrap();
        let saved = save_known_file(Some(path.clone()), Some("lama".into()), "baru".into()).await.unwrap().unwrap();
        assert_eq!(saved, (path.clone(), "baru".into()));
        let next = save_known_file(Some(path.clone()), Some(saved.1), "berikutnya".into()).await.unwrap();
        assert!(next.is_some());
        assert_eq!(tokio::fs::read_to_string(&path).await.unwrap(), "berikutnya");
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn newly_created_file_is_not_overwritten_if_it_appears() {
        let dir = temporary_directory();
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("draft.md");
        let expected = read_file_state(&path).await.unwrap();
        assert_eq!(expected, FileState::Missing);
        tokio::fs::write(&path, "file lain").await.unwrap();
        assert!(atomic_write_checked(&path, b"draft baru", Some(&expected)).await.is_err());
        assert_eq!(tokio::fs::read_to_string(&path).await.unwrap(), "file lain");
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlink_alias_is_the_same_save_target() {
        use std::os::unix::fs::symlink;
        let dir = temporary_directory();
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let target = dir.join("draft.md");
        let alias = dir.join("link.md");
        tokio::fs::write(&target, "lama").await.unwrap();
        symlink(&target, &alias).unwrap();
        assert!(is_same_file(&target, &alias).await);
        assert!(!is_same_file(&target, &dir.join("other.md")).await);
        let prepared = prepare_destination(Some(target.clone()), Some("lama".into()), "baru".into(), alias).await.unwrap();
        assert!(!prepared.requires_overwrite);
        write_prepared(prepared).await.unwrap();
        assert_eq!(tokio::fs::read_to_string(&target).await.unwrap(), "baru");
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn replaces_existing_file_and_cleans_up_temporary_file() {
        let dir = temporary_directory();
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("draft.md");
        tokio::fs::write(&path, "lama").await.unwrap();
        atomic_write(&path, b"baru").await.unwrap();
        assert_eq!(tokio::fs::read_to_string(&path).await.unwrap(), "baru");
        let mut entries = tokio::fs::read_dir(&dir).await.unwrap();
        assert_eq!(entries.next_entry().await.unwrap().unwrap().file_name(), "draft.md");
        assert!(entries.next_entry().await.unwrap().is_none());
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn invalid_destination_preserves_existing_file() {
        let dir = temporary_directory();
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let original = dir.join("draft.md");
        tokio::fs::write(&original, "teks lama").await.unwrap();
        assert!(atomic_write(&original.join("child"), b"baru").await.is_err());
        assert_eq!(tokio::fs::read_to_string(&original).await.unwrap(), "teks lama");
        assert!(atomic_write(&dir, b"baru").await.is_err());
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn new_file_is_not_readable_by_group_or_others() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temporary_directory();
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("baru.md");
        atomic_write(&path, b"rahasia").await.unwrap();
        assert_eq!(tokio::fs::metadata(&path).await.unwrap().permissions().mode() & 0o077, 0);
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn preserves_permissions_and_follows_symbolic_link() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = temporary_directory();
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let target = dir.join("draft.md");
        let link = dir.join("link.md");
        tokio::fs::write(&target, "lama").await.unwrap();
        tokio::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).await.unwrap();
        symlink(&target, &link).unwrap();
        atomic_write(&link, b"baru").await.unwrap();
        assert!(tokio::fs::symlink_metadata(&link).await.unwrap().file_type().is_symlink());
        assert_eq!(tokio::fs::read_to_string(&target).await.unwrap(), "baru");
        assert_eq!(tokio::fs::metadata(&target).await.unwrap().permissions().mode() & 0o777, 0o600);
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn refuses_hard_links_and_broken_symlinks() {
        use std::os::unix::fs::symlink;
        let dir = temporary_directory();
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let target = dir.join("draft.md");
        let hard_link = dir.join("hard.md");
        let broken_link = dir.join("broken.md");
        tokio::fs::write(&target, "lama").await.unwrap();
        tokio::fs::hard_link(&target, &hard_link).await.unwrap();
        symlink(dir.join("missing.md"), &broken_link).unwrap();
        assert!(atomic_write(&target, b"baru").await.is_err());
        assert!(atomic_write(&broken_link, b"baru").await.is_err());
        assert_eq!(tokio::fs::read_to_string(&target).await.unwrap(), "lama");
        assert_eq!(tokio::fs::read_to_string(&hard_link).await.unwrap(), "lama");
        assert!(tokio::fs::symlink_metadata(&broken_link).await.unwrap().file_type().is_symlink());
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[test]
    fn failed_save_keeps_unsaved_changes() {
        let mut writer = Writer {
            content: text_editor::Content::with_text("draft belum tersimpan"),
            dirty: true,
            busy: true,
            ..Writer::default()
        };
        let _ = writer.update(Message::Saved(Err("gagal".into())));
        assert_eq!(writer.content.text().trim_end(), "draft belum tersimpan");
        assert!(writer.dirty);
        assert!(!writer.busy);
        assert_eq!(writer.error.as_deref(), Some("gagal"));
    }
}
