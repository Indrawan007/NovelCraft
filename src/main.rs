mod bookmarks;
mod epub_reader;
mod library;
mod reader;
mod settings;
mod shortcuts;
mod writer;

use iced::widget::{button, column, container, row, text, Column};
use iced::{event, keyboard, window, Element, Length, Subscription, Task, Theme};
use std::ffi::OsString;
use std::path::PathBuf;

fn main() -> iced::Result {
    iced::application("NovelCraft", NovelCraft::update, NovelCraft::view)
        .theme(|app: &NovelCraft| app.theme_choice.iced_theme())
        .window(window::Settings {
            size: (1100.0, 750.0).into(),
            exit_on_close_request: false,
            ..window::Settings::default()
        })
        .subscription(NovelCraft::subscription)
        .run_with(|| {
            let mut app = NovelCraft::default();
            let startup = startup_file(std::env::args_os().skip(1), std::env::current_dir());
            let open = app.open_startup_file(startup);
            let task = Task::batch([
                open,
                app.library.update(library::Message::Load).map(Message::Library),
                app.library.update(library::Message::RefreshBookmarks).map(Message::Library),
                app.update(Message::LoadSettings),
            ]);
            (app, task)
        })
}

// Accept one local file path. Keep OS-native bytes and preserve the selected
// path rather than canonicalizing it, as bookmarks are keyed by path.
fn startup_file(args: impl IntoIterator<Item = OsString>, cwd: std::io::Result<PathBuf>) -> Result<Option<PathBuf>, String> {
    let mut args = args.into_iter();
    let Some(arg) = args.next() else { return Ok(None); };
    if args.next().is_some() {
        return Err("Pilih hanya satu file EPUB, TXT, atau Markdown saat membuka aplikasi.".into());
    }
    let path = PathBuf::from(arg);
    let supported = path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| {
        ["epub", "txt", "md", "markdown"].iter().any(|format| ext.eq_ignore_ascii_case(format))
    });
    if !supported {
        return Err("Argumen harus berupa satu path file EPUB, TXT, atau Markdown lokal.".into());
    }
    let absolute = if path.is_absolute() {
        path
    } else {
        cwd.map_err(|error| format!("Tidak dapat menentukan direktori kerja untuk path relatif: {error}"))?.join(path)
    };
    Ok(Some(absolute))
}

fn shortcut_event(event: iced::Event, status: event::Status, _: window::Id) -> Option<Message> {
    if status != event::Status::Ignored {
        return None;
    }
    if let iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event {
        return shortcuts::from_key(key.as_ref(), modifiers).map(Message::Shortcut);
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Page {
    #[default]
    Library,
    Reader,
    Writer,
    Settings,
}

impl Page {
    const ALL: [Self; 4] = [Self::Library, Self::Reader, Self::Writer, Self::Settings];

    fn title(self) -> &'static str {
        match self {
            Self::Library => "Library",
            Self::Reader => "Reader",
            Self::Writer => "Writer",
            Self::Settings => "Settings",
        }
    }

    fn shortcut_hint(self) -> &'static str {
        match self {
            Self::Library => "Ctrl+1",
            Self::Reader => "Ctrl+2",
            Self::Writer => "Ctrl+3",
            Self::Settings => "Ctrl+,",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum ThemeChoice {
    #[default]
    TokyoNight,
    Dark,
    Nord,
    Dracula,
}

impl ThemeChoice {
    const ALL: [Self; 4] = [Self::TokyoNight, Self::Dark, Self::Nord, Self::Dracula];

    fn label(self) -> &'static str {
        match self {
            Self::TokyoNight => "Tokyo Night",
            Self::Dark => "Dark",
            Self::Nord => "Nord",
            Self::Dracula => "Dracula",
        }
    }

    fn iced_theme(self) -> Theme {
        match self {
            Self::TokyoNight => Theme::TokyoNight,
            Self::Dark => Theme::Dark,
            Self::Nord => Theme::Nord,
            Self::Dracula => Theme::Dracula,
        }
    }
}

#[derive(Debug, Clone)]
enum Message {
    Navigate(Page),
    ToggleFocusMode,
    LoadSettings,
    SettingsLoaded(Result<settings::Settings, String>),
    SelectTheme(ThemeChoice),
    SettingsSaved(Result<ThemeChoice, String>),
    ReaderFontSaved(Result<u16, String>),
    Writer(writer::Message),
    Reader(reader::Message),
    Library(library::Message),
    CloseRequested(window::Id),
    ConfirmClose,
    CancelClose,
    ReturnToWriter,
    WindowResized,
    Shortcut(shortcuts::Shortcut),
}

#[derive(Default)]
struct NovelCraft {
    page: Page,
    focus_mode: bool,
    theme_choice: ThemeChoice,
    settings_loaded: bool,
    settings_busy: bool,
    settings_error: Option<String>,
    writer: writer::Writer,
    reader: reader::Reader,
    library: library::Library,
    pending_close: Option<window::Id>,
}

impl NovelCraft {
    fn open_startup_file(&mut self, file: Result<Option<PathBuf>, String>) -> Task<Message> {
        match file {
            Ok(None) => Task::none(),
            Ok(Some(path)) => {
                self.page = Page::Reader;
                self.reader.open_path(path).map(|task| task.map(Message::Reader)).unwrap_or_else(Task::none)
            }
            Err(error) => {
                self.page = Page::Reader;
                self.reader.update(reader::Message::Opened(Err(error))).map(Message::Reader)
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            window::close_requests().map(Message::CloseRequested),
            window::resize_events().map(|_| Message::WindowResized),
            event::listen_with(shortcut_event),
        ])
    }

    fn file_operation_in_progress(&self) -> bool {
        self.writer.is_busy() || self.library.is_busy() || self.reader.is_busy() || self.settings_busy
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::CloseRequested(id) => {
                if self.file_operation_in_progress() || self.writer.has_unsaved_changes() {
                    if self.page == Page::Reader {
                        self.reader.suspend_scroll_position();
                    }
                    self.pending_close = Some(id);
                    Task::none()
                } else {
                    window::close(id)
                }
            }
            Message::ConfirmClose if !self.file_operation_in_progress() => {
                if let Some(id) = self.pending_close.take() {
                    window::close(id)
                } else {
                    Task::none()
                }
            }
            Message::ConfirmClose => Task::none(),
            Message::CancelClose => {
                self.pending_close = None;
                if self.page == Page::Reader {
                    self.reader.take_restore_task().map(Message::Reader)
                } else {
                    Task::none()
                }
            }
            Message::ReturnToWriter => {
                self.pending_close = None;
                if self.page != Page::Writer {
                    self.focus_mode = false;
                }
                self.page = Page::Writer;
                Task::none()
            }
            Message::WindowResized => {
                self.reader.invalidate_scroll_position();
                Task::none()
            }
            Message::Navigate(page) => {
                if self.page == Page::Reader && page != Page::Reader {
                    self.reader.suspend_scroll_position();
                }
                if page != self.page {
                    self.focus_mode = false;
                }
                self.page = page;
                if page == Page::Reader && self.pending_close.is_none() {
                    self.reader.take_restore_task().map(Message::Reader)
                } else if page == Page::Library {
                    Task::batch([
                        self.library.update(library::Message::RefreshBookmarks).map(Message::Library),
                        self.library.update(library::Message::RefreshAvailability).map(Message::Library),
                    ])
                } else {
                    Task::none()
                }
            }
            Message::ToggleFocusMode if matches!(self.page, Page::Reader | Page::Writer) && self.pending_close.is_none() => {
                self.focus_mode = !self.focus_mode;
                // Changing the reading width changes wrapping and scroll measurements.
                if self.page == Page::Reader {
                    self.reader.invalidate_scroll_position();
                }
                Task::none()
            }
            Message::ToggleFocusMode => Task::none(),
            Message::LoadSettings if !self.settings_busy && !self.settings_loaded => {
                self.settings_busy = true;
                self.settings_error = None;
                Task::perform(settings::load(), Message::SettingsLoaded)
            }
            Message::LoadSettings => Task::none(),
            Message::SettingsLoaded(result) => {
                self.settings_busy = false;
                match result {
                    Ok(settings) => {
                        self.theme_choice = settings.theme;
                        self.reader.apply_font_size(settings.reader_font_size);
                        self.settings_loaded = true;
                        self.settings_error = None;
                    }
                    Err(error) => self.settings_error = Some(error),
                }
                Task::none()
            }
            Message::SelectTheme(choice) if self.page == Page::Settings && self.pending_close.is_none()
                && self.settings_loaded && !self.settings_busy && choice != self.theme_choice => {
                self.settings_busy = true;
                self.settings_error = None;
                Task::perform(settings::save(choice), Message::SettingsSaved)
            }
            Message::SelectTheme(_) => Task::none(),
            Message::SettingsSaved(result) => {
                self.settings_busy = false;
                match result {
                    Ok(choice) => {
                        self.theme_choice = choice;
                        self.settings_error = None;
                    }
                    Err(error) => self.settings_error = Some(error),
                }
                Task::none()
            }
            Message::ReaderFontSaved(result) => {
                self.settings_busy = false;
                match result {
                    Ok(size) => {
                        self.reader.apply_font_size(size);
                        self.settings_error = None;
                    }
                    Err(error) => self.settings_error = Some(error),
                }
                Task::none()
            }
            Message::Reader(message @ (reader::Message::FontSmaller | reader::Message::FontLarger)) => {
                if self.page != Page::Reader || self.pending_close.is_some() || !self.settings_loaded || self.settings_busy {
                    return Task::none();
                }
                let Some(size) = self.reader.requested_font_size(&message) else { return Task::none() };
                self.settings_busy = true;
                self.settings_error = None;
                Task::perform(settings::save_reader_font_size(size), Message::ReaderFontSaved)
            }
            Message::Shortcut(shortcut) if self.pending_close.is_none() => {
                use shortcuts::Shortcut;
                let message = match shortcut {
                    Shortcut::Library => Message::Navigate(Page::Library),
                    Shortcut::Reader => Message::Navigate(Page::Reader),
                    Shortcut::Writer => Message::Navigate(Page::Writer),
                    Shortcut::Settings => Message::Navigate(Page::Settings),
                    Shortcut::NewDraft if self.page == Page::Writer => Message::Writer(writer::Message::NewDraft),
                    Shortcut::Open if self.page == Page::Writer => Message::Writer(writer::Message::Open),
                    Shortcut::Save if self.page == Page::Writer => Message::Writer(writer::Message::Save),
                    Shortcut::SaveAs if self.page == Page::Writer => Message::Writer(writer::Message::SaveAs),
                    Shortcut::PreviousSection if self.page == Page::Reader => Message::Reader(reader::Message::PreviousSection),
                    Shortcut::NextSection if self.page == Page::Reader => Message::Reader(reader::Message::NextSection),
                    _ => return Task::none(),
                };
                self.update(message)
            }
            Message::Shortcut(_) => Task::none(),
            Message::Writer(writer::Message::Shortcut(shortcut)) => self.update(Message::Shortcut(shortcut)),
            Message::Writer(message) => self.writer.update(message).map(Message::Writer),
            Message::Reader(message) => {
                let bookmark_saved = matches!(&message, reader::Message::BookmarkSaved(Ok(_)) | reader::Message::BookmarkRemoved(Ok(_)));
                let task = self.reader.update(message).map(Message::Reader);
                let restore = if self.page == Page::Reader && self.pending_close.is_none() {
                    self.reader.take_restore_task().map(Message::Reader)
                } else {
                    Task::none()
                };
                let refresh = if bookmark_saved {
                    self.library.update(library::Message::RefreshBookmarks).map(Message::Library)
                } else {
                    Task::none()
                };
                Task::batch([task, restore, refresh])
            }
            Message::Library(library::Message::Open(path)) => self.open_library_book(path, false),
            Message::Library(library::Message::Continue(path)) => self.open_library_book(path, true),
            Message::Library(message) => self.library.update(message).map(Message::Library),
        }
    }

    fn open_library_book(&mut self, path: std::path::PathBuf, resume: bool) -> Task<Message> {
        if !self.library.can_open_book() {
            return Task::none();
        }
        let task = if resume {
            self.reader.open_path_with_bookmark(path)
        } else {
            self.reader.open_path(path)
        };
        if let Some(task) = task {
            self.page = Page::Reader;
            self.focus_mode = false;
            task.map(Message::Reader)
        } else {
            self.library.set_error("Reader masih sibuk atau menunggu konfirmasi. Selesaikan dahulu di Reader.");
            Task::none()
        }
    }

    fn close_confirmation(&self) -> Element<'_, Message> {
        let mut content = column![text("Tutup NovelCraft?").size(28)].spacing(16);
        if self.file_operation_in_progress() {
            content = content.push(text(
                "Operasi file Reader, Writer, Library, atau Settings masih berlangsung. Selesaikan dialog atau tunggu proses selesai sebelum menutup aplikasi.",
            ));
        } else if self.writer.has_unsaved_changes() {
            content = content.push(text("Draft Writer memiliki perubahan yang belum disimpan."));
            content = content.push(
                row![
                    button("Kembali ke Writer").on_press(Message::ReturnToWriter),
                    button("Buang perubahan & tutup").on_press(Message::ConfirmClose),
                ].spacing(12),
            );
        } else {
            content = content.push(text("Operasi file selesai. Tutup aplikasi sekarang?"));
            content = content.push(button("Tutup aplikasi").on_press(Message::ConfirmClose));
        }
        content = content.push(button("Batal").on_press(Message::CancelClose));
        container(content).width(Length::Fill).height(Length::Fill).padding(32).into()
    }

    fn settings_view(&self) -> Element<'_, Message> {
        let mut content = column![
            text("Settings").size(32),
            text("Tema gelap").size(20),
            text("Pilihan disimpan secara lokal; tema berubah setelah penyimpanan berhasil.").size(14),
        ]
        .spacing(12);
        if self.settings_busy {
            content = content.push(text("Memproses Settings...").size(14));
        }
        if let Some(error) = &self.settings_error {
            content = content.push(text(error).size(14));
            if !self.settings_loaded && !self.settings_busy {
                content = content.push(button("Coba muat ulang Settings").on_press(Message::LoadSettings));
            }
        }
        for choice in ThemeChoice::ALL {
            let label = if choice == self.theme_choice {
                format!("✓ {} (aktif)", choice.label())
            } else {
                choice.label().to_owned()
            };
            let mut option = button(text(label));
            if self.settings_loaded && !self.settings_busy && choice != self.theme_choice {
                option = option.on_press(Message::SelectTheme(choice));
            }
            content = content.push(option);
        }
        content.into()
    }

    fn view(&self) -> Element<'_, Message> {
        if self.pending_close.is_some() {
            return self.close_confirmation();
        }

        let mut navigation = Column::new().spacing(8);
        if !self.focus_mode {
            navigation = navigation.push(text("NAVIGATE").size(12));
            for page in Page::ALL {
                let label = if self.page == page {
                    format!("›  {}  {}", page.title(), page.shortcut_hint())
                } else {
                    format!("   {}  {}", page.title(), page.shortcut_hint())
                };
                navigation = navigation.push(
                    button(text(label))
                        .width(Length::Fill)
                        .on_press(Message::Navigate(page)),
                );
            }
        }

        // Keep the sidebar's place in the widget tree so the editor state is not replaced.
        let sidebar = container(navigation)
            .width(if self.focus_mode { 0_u16 } else { 200_u16 })
            .height(Length::Fill)
            .padding(if self.focus_mode { 0_u16 } else { 20_u16 });

        let theme = self.theme_choice.iced_theme();
        let page: Element<'_, Message> = if self.page == Page::Library {
            self.library.view().map(Message::Library)
        } else if self.page == Page::Writer {
            self.writer.view(&theme).map(Message::Writer)
        } else if self.page == Page::Reader {
            let status = if self.settings_busy {
                "Memproses Settings..."
            } else if self.settings_error.is_some() && !self.settings_loaded {
                "Settings gagal dimuat; perbaiki dan muat ulang di Settings."
            } else if self.settings_error.is_some() {
                "Settings gagal disimpan; coba lagi atau lihat Settings."
            } else if !self.settings_loaded {
                "Settings belum dimuat."
            } else {
                "Ukuran font disimpan secara lokal."
            };
            self.reader.view(&theme, self.settings_loaded && !self.settings_busy, status).map(Message::Reader)
        } else {
            self.settings_view()
        };

        let content = container(page)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(32);

        let mut topbar_content = row![text("novelcraft").size(18)].spacing(16);
        if matches!(self.page, Page::Reader | Page::Writer) {
            let label = if self.focus_mode { "Keluar mode fokus" } else { "Mode fokus" };
            topbar_content = topbar_content.push(button(label).on_press(Message::ToggleFocusMode));
        }
        let topbar = container(topbar_content)
            .width(Length::Fill)
            .padding([12, 20]);

        column![topbar, row![sidebar, content].height(Length::Fill)]
            .height(Length::Fill)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writer_focus_mode_keeps_unsaved_draft_and_is_unavailable_in_library() {
        let mut app = NovelCraft::default();
        let _ = app.update(Message::ToggleFocusMode);
        assert!(!app.focus_mode);
        let _ = app.update(Message::Navigate(Page::Writer));
        make_draft_dirty(&mut app);
        let _ = app.update(Message::ToggleFocusMode);
        assert!(app.focus_mode);
        assert!(app.writer.has_unsaved_changes());
        drop(app.view());
        let _ = app.update(Message::ToggleFocusMode);
        assert!(!app.focus_mode);
        assert!(app.writer.has_unsaved_changes());
    }

    #[test]
    fn navigating_away_exits_focus_and_keyboard_navigation_still_works() {
        let mut app = NovelCraft::default();
        let _ = app.update(Message::Navigate(Page::Writer));
        let _ = app.update(Message::ToggleFocusMode);
        let _ = app.update(Message::Shortcut(shortcuts::Shortcut::Library));
        assert_eq!(app.page, Page::Library);
        assert!(!app.focus_mode);
        let _ = app.update(Message::Shortcut(shortcuts::Shortcut::Writer));
        assert_eq!(app.page, Page::Writer);
        assert!(!app.focus_mode);
    }

    #[test]
    fn reader_focus_mode_resets_on_page_change_but_not_same_page_navigation() {
        let mut app = NovelCraft::default();
        let _ = app.update(Message::Navigate(Page::Reader));
        let _ = app.update(Message::ToggleFocusMode);
        assert!(app.focus_mode);
        drop(app.view());
        let _ = app.update(Message::Navigate(Page::Reader));
        assert!(app.focus_mode);
        app.pending_close = Some(window::Id::unique());
        let _ = app.update(Message::ToggleFocusMode);
        assert!(app.focus_mode);
        let _ = app.update(Message::CancelClose);
        assert!(app.focus_mode);
        let _ = app.update(Message::Shortcut(shortcuts::Shortcut::Writer));
        assert_eq!(app.page, Page::Writer);
        assert!(!app.focus_mode);
        let _ = app.update(Message::Navigate(Page::Reader));
        let _ = app.update(Message::ToggleFocusMode);
        assert!(app.focus_mode);
        let _ = app.update(Message::ReturnToWriter);
        assert_eq!(app.page, Page::Writer);
        assert!(!app.focus_mode);
        let _ = app.update(Message::Navigate(Page::Settings));
        let _ = app.update(Message::ToggleFocusMode);
        assert!(!app.focus_mode);
    }

    #[test]
    fn focus_mode_cannot_bypass_close_confirmation() {
        let mut app = NovelCraft::default();
        let _ = app.update(Message::Navigate(Page::Writer));
        make_draft_dirty(&mut app);
        let _ = app.update(Message::ToggleFocusMode);
        let id = window::Id::unique();
        let _ = app.update(Message::CloseRequested(id));
        let _ = app.update(Message::ToggleFocusMode);
        assert!(app.focus_mode);
        assert_eq!(app.pending_close, Some(id));
        let _ = app.update(Message::CancelClose);
        assert!(app.focus_mode);
        assert!(app.writer.has_unsaved_changes());
    }

    #[test]
    fn global_shortcuts_ignore_events_already_handled_by_widgets() {
        use keyboard::key::{NativeCode, Physical};
        let pressed = iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Character("s".into()),
            modified_key: keyboard::Key::Character("s".into()),
            physical_key: Physical::Unidentified(NativeCode::Unidentified),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::CTRL,
            text: None,
        });
        let id = window::Id::unique();
        assert!(matches!(shortcut_event(pressed.clone(), event::Status::Ignored, id), Some(Message::Shortcut(shortcuts::Shortcut::Save))));
        assert!(shortcut_event(pressed, event::Status::Captured, id).is_none());
    }

    #[test]
    fn writer_shortcuts_only_run_on_writer_and_respect_confirmation() {
        use shortcuts::Shortcut;
        let mut app = NovelCraft::default();
        let _ = app.update(Message::Shortcut(Shortcut::Save));
        assert!(!app.writer.is_busy());
        let _ = app.update(Message::Shortcut(Shortcut::Writer));
        assert_eq!(app.page, Page::Writer);
        make_draft_dirty(&mut app);
        let _ = app.update(Message::Writer(writer::Message::Shortcut(Shortcut::NewDraft)));
        let _ = app.update(Message::Shortcut(Shortcut::Save));
        assert!(!app.writer.is_busy());
        assert!(app.writer.has_unsaved_changes());
        let _ = app.update(Message::Writer(writer::Message::CancelDiscard));
        let _ = app.update(Message::Shortcut(Shortcut::Save));
        assert!(!app.writer.is_busy());
        let _ = app.update(Message::Writer(writer::Message::ConfirmFormatSave));
        assert!(app.writer.is_busy());
    }

    #[test]
    fn shortcuts_cannot_bypass_close_confirmation() {
        let mut app = NovelCraft::default();
        make_draft_dirty(&mut app);
        let id = window::Id::unique();
        let _ = app.update(Message::CloseRequested(id));
        let _ = app.update(Message::Shortcut(shortcuts::Shortcut::Writer));
        assert_eq!(app.page, Page::Library);
        assert_eq!(app.pending_close, Some(id));
        assert!(app.writer.has_unsaved_changes());
    }

    #[test]
    fn application_defaults_to_tokyo_night() {
        assert_eq!(NovelCraft::default().theme_choice.iced_theme(), Theme::TokyoNight);
    }

    #[test]
    fn settings_saves_theme_without_discarding_writer_state() {
        let mut app = NovelCraft::default();
        make_draft_dirty(&mut app);
        let _ = app.update(Message::SettingsLoaded(Ok(settings::Settings::default())));
        let _ = app.update(Message::Navigate(Page::Settings));
        for (choice, expected) in [
            (ThemeChoice::Dark, Theme::Dark),
            (ThemeChoice::Nord, Theme::Nord),
            (ThemeChoice::Dracula, Theme::Dracula),
            (ThemeChoice::TokyoNight, Theme::TokyoNight),
        ] {
            let _ = app.update(Message::SelectTheme(choice));
            assert!(app.settings_busy);
            let _ = app.update(Message::SettingsSaved(Ok(choice)));
            assert_eq!(app.theme_choice.iced_theme(), expected);
            assert!(app.writer.has_unsaved_changes());
            drop(app.view());
        }
        let _ = app.update(Message::SelectTheme(ThemeChoice::Nord));
        let _ = app.update(Message::SettingsSaved(Ok(ThemeChoice::Nord)));
        let _ = app.update(Message::Navigate(Page::Writer));
        assert_eq!(app.theme_choice, ThemeChoice::Nord);
        drop(app.view());
        let _ = app.update(Message::SelectTheme(ThemeChoice::Dark));
        assert_eq!(app.theme_choice, ThemeChoice::Nord);
        assert!(app.writer.has_unsaved_changes());
    }

    #[test]
    fn settings_errors_do_not_change_theme_or_allow_overlapping_saves() {
        let mut app = NovelCraft::default();
        let _ = app.update(Message::Navigate(Page::Settings));
        let _ = app.update(Message::SettingsLoaded(Err("rusak".into())));
        let _ = app.update(Message::SelectTheme(ThemeChoice::Nord));
        assert!(!app.settings_loaded);
        assert!(!app.settings_busy);
        let _ = app.update(Message::LoadSettings);
        assert!(app.settings_busy);
        let _ = app.update(Message::SettingsLoaded(Ok(settings::Settings { theme: ThemeChoice::Dark, ..settings::Settings::default() })));
        let _ = app.update(Message::SelectTheme(ThemeChoice::Nord));
        let _ = app.update(Message::SelectTheme(ThemeChoice::Dracula));
        assert!(app.settings_busy);
        assert_eq!(app.theme_choice, ThemeChoice::Dark);
        let _ = app.update(Message::SettingsSaved(Err("gagal".into())));
        assert_eq!(app.theme_choice, ThemeChoice::Dark);
        assert_eq!(app.settings_error.as_deref(), Some("gagal"));
        assert!(!app.settings_busy);
    }

    #[test]
    fn close_waits_for_settings_write() {
        let mut app = NovelCraft::default();
        let _ = app.update(Message::SettingsLoaded(Ok(settings::Settings::default())));
        let _ = app.update(Message::Navigate(Page::Settings));
        let _ = app.update(Message::SelectTheme(ThemeChoice::Nord));
        let id = window::Id::unique();
        let _ = app.update(Message::CloseRequested(id));
        let _ = app.update(Message::ConfirmClose);
        assert_eq!(app.pending_close, Some(id));
        let _ = app.update(Message::SettingsSaved(Ok(ThemeChoice::Nord)));
        assert!(!app.settings_busy);
        let _ = app.update(Message::ConfirmClose);
        assert_eq!(app.pending_close, None);
    }

    #[test]
    fn theme_selection_cannot_bypass_close_confirmation() {
        let mut app = NovelCraft::default();
        make_draft_dirty(&mut app);
        let _ = app.update(Message::Navigate(Page::Settings));
        let id = window::Id::unique();
        let _ = app.update(Message::CloseRequested(id));
        let _ = app.update(Message::SelectTheme(ThemeChoice::Dark));
        assert_eq!(app.theme_choice, ThemeChoice::TokyoNight);
        assert_eq!(app.pending_close, Some(id));
    }

    #[test]
    fn startup_accepts_one_supported_local_path_and_resolves_relative_paths() {
        let cwd = PathBuf::from("/workspace");
        assert_eq!(startup_file(Vec::<OsString>::new(), Err(std::io::ErrorKind::NotFound.into())).unwrap(), None);
        assert_eq!(startup_file(vec![OsString::from("books/My Novel.EPUB")], Ok(cwd.clone())).unwrap(), Some(cwd.join("books/My Novel.EPUB")));
        for name in ["a.TXT", "a.md", "a.MARKDOWN"] {
            let path = PathBuf::from("/books").join(name);
            assert_eq!(startup_file(vec![path.clone().into_os_string()], Err(std::io::ErrorKind::NotFound.into())).unwrap(), Some(path));
        }
        assert!(startup_file(vec![OsString::from("draft.pdf")], Ok(cwd.clone())).is_err());
        assert!(startup_file(vec![OsString::from("")], Ok(cwd.clone())).is_err());
        assert!(startup_file(vec![OsString::from("first.txt"), OsString::from("second.txt")], Ok(cwd)).is_err());
        assert!(startup_file(vec![OsString::from("relative.txt")], Err(std::io::ErrorKind::NotFound.into())).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn startup_preserves_non_utf8_path_bytes() {
        use std::os::unix::ffi::OsStringExt;
        let file = OsString::from_vec(b"novel-\xff.txt".to_vec());
        let expected = PathBuf::from("/books").join(&file);
        assert_eq!(startup_file(vec![file], Ok(PathBuf::from("/books"))).unwrap(), Some(expected));
    }

    #[test]
    fn startup_file_opens_reader_and_errors_do_not_exit_app() {
        let mut app = NovelCraft::default();
        let _ = app.open_startup_file(Ok(None));
        assert_eq!(app.page, Page::Library);
        assert!(!app.reader.is_busy());
        let _ = app.open_startup_file(Err("Argumen tidak valid".into()));
        assert_eq!(app.page, Page::Reader);
        assert!(!app.reader.is_busy());
        drop(app.view());
        let mut app = NovelCraft::default();
        let _ = app.open_startup_file(Ok(Some(PathBuf::from("/novel.txt"))));
        assert_eq!(app.page, Page::Reader);
        assert!(app.reader.is_busy());
        assert!(app.file_operation_in_progress());
    }

    #[test]
    fn starts_in_library() {
        assert_eq!(NovelCraft::default().page, Page::Library);
    }

    #[test]
    fn continuing_library_book_opens_reader_and_respects_busy_state() {
        let mut app = NovelCraft::default();
        let _ = app.update(Message::Library(library::Message::Continue("/book.epub".into())));
        assert_eq!(app.page, Page::Reader);
        assert!(app.reader.is_busy());
        let _ = app.update(Message::Navigate(Page::Library));
        let _ = app.update(Message::Library(library::Message::Continue("/other.epub".into())));
        assert_eq!(app.page, Page::Library);
        assert!(app.reader.is_busy());
    }

    #[test]
    fn opening_library_book_navigates_to_reader() {
        let mut app = NovelCraft::default();
        let _ = app.update(Message::Library(library::Message::Open("/book.txt".into())));
        assert_eq!(app.page, Page::Reader);
    }

    fn make_draft_dirty(app: &mut NovelCraft) {
        use iced::widget::text_editor::{Action, Edit};
        let _ = app.update(Message::Writer(writer::Message::Edit(Action::Edit(Edit::Insert('a')))));
        assert!(app.writer.has_unsaved_changes());
    }

    #[test]
    fn unsaved_draft_requires_confirmation_even_outside_writer() {
        let mut app = NovelCraft::default();
        make_draft_dirty(&mut app);
        let id = window::Id::unique();
        let _ = app.update(Message::CloseRequested(id));
        assert_eq!(app.pending_close, Some(id));
        let _ = app.update(Message::CancelClose);
        assert_eq!(app.pending_close, None);
        assert!(app.writer.has_unsaved_changes());
        let _ = app.update(Message::CloseRequested(id));
        let _ = app.update(Message::ReturnToWriter);
        assert_eq!(app.page, Page::Writer);
        assert_eq!(app.pending_close, None);
        assert!(app.writer.has_unsaved_changes());
    }

    #[test]
    fn close_waits_for_writer_save_and_retains_dirty_on_cancel() {
        let mut app = NovelCraft::default();
        make_draft_dirty(&mut app);
        let _ = app.update(Message::Writer(writer::Message::Save));
        let _ = app.update(Message::Writer(writer::Message::ConfirmFormatSave));
        let id = window::Id::unique();
        let _ = app.update(Message::CloseRequested(id));
        let _ = app.update(Message::ConfirmClose);
        assert_eq!(app.pending_close, Some(id));
        let _ = app.update(Message::Writer(writer::Message::Saved(Ok(None))));
        assert!(!app.file_operation_in_progress());
        assert!(app.writer.has_unsaved_changes());
        assert_eq!(app.pending_close, Some(id));
    }

    #[test]
    fn completed_save_keeps_close_confirmation_until_user_decides() {
        let mut app = NovelCraft::default();
        make_draft_dirty(&mut app);
        let _ = app.update(Message::Writer(writer::Message::Save));
        let _ = app.update(Message::Writer(writer::Message::ConfirmFormatSave));
        let id = window::Id::unique();
        let _ = app.update(Message::CloseRequested(id));
        let _ = app.update(Message::Writer(writer::Message::Saved(Ok(Some(("/draft.md".into(), "a".into()))))));
        assert!(!app.writer.has_unsaved_changes());
        assert_eq!(app.pending_close, Some(id));
        let _ = app.update(Message::ConfirmClose);
        assert_eq!(app.pending_close, None);
    }

    #[test]
    fn failed_save_does_not_discard_draft_on_close() {
        let mut app = NovelCraft::default();
        make_draft_dirty(&mut app);
        let _ = app.update(Message::Writer(writer::Message::Save));
        let _ = app.update(Message::Writer(writer::Message::ConfirmFormatSave));
        let id = window::Id::unique();
        let _ = app.update(Message::CloseRequested(id));
        let _ = app.update(Message::Writer(writer::Message::Saved(Err("gagal".into()))));
        assert!(app.writer.has_unsaved_changes());
        assert_eq!(app.pending_close, Some(id));
        let _ = app.update(Message::ReturnToWriter);
        assert_eq!(app.page, Page::Writer);
        assert!(app.writer.has_unsaved_changes());
    }

    #[test]
    fn close_waits_for_reader_file_operations() {
        let mut app = NovelCraft::default();
        let _ = app.reader.open_path("/book.txt".into());
        let id = window::Id::unique();
        let _ = app.update(Message::CloseRequested(id));
        let _ = app.update(Message::ConfirmClose);
        assert_eq!(app.pending_close, Some(id));
        let _ = app.update(Message::Reader(reader::Message::Opened(Err("gagal".into()))));
        assert!(!app.file_operation_in_progress());
        let _ = app.update(Message::ConfirmClose);
        assert_eq!(app.pending_close, None);
    }

    #[test]
    fn close_waits_for_library_operations() {
        let mut app = NovelCraft::default();
        let _ = app.update(Message::Library(library::Message::Load));
        let id = window::Id::unique();
        let _ = app.update(Message::CloseRequested(id));
        let _ = app.update(Message::ConfirmClose);
        assert_eq!(app.pending_close, Some(id));
        let _ = app.update(Message::Library(library::Message::Loaded(Ok((vec![], None)))));
        assert!(!app.file_operation_in_progress());
        let _ = app.update(Message::ConfirmClose);
        assert_eq!(app.pending_close, None);
    }

    #[test]
    fn clean_idle_close_needs_no_confirmation() {
        let mut app = NovelCraft::default();
        let _ = app.update(Message::CloseRequested(window::Id::unique()));
        assert_eq!(app.pending_close, None);
    }

    #[test]
    fn navigation_changes_the_current_page() {
        let mut app = NovelCraft::default();
        for page in Page::ALL {
            let _ = app.update(Message::Navigate(page));
            assert_eq!(app.page, page);
        }
    }
}
