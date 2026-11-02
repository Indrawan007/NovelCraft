mod activity;
mod bookmarks;
mod epub_reader;
mod library;
mod reader;
mod project;
mod reference;
mod settings;
mod shortcuts;
#[path = "mockup_style.rs"]
mod mockup_style;
mod writer;

use iced::widget::{button, column, container, horizontal_space, pick_list, row, scrollable, stack, text, vertical_space, Column, Space};
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

fn fullscreen_target(mode: window::Mode) -> Option<window::Mode> {
    match mode {
        window::Mode::Windowed => Some(window::Mode::Fullscreen),
        window::Mode::Fullscreen => Some(window::Mode::Windowed),
        window::Mode::Hidden => None,
    }
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

// Always cancel a gesture released outside its drop target, or when the window
// loses pointer/focus. A target's own release message performs the move first;
// cancellation is harmless once the gesture has finished.
fn chapter_drag_event(event: iced::Event, _status: event::Status, _: window::Id) -> Option<Message> {
    match event {
        iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left) | iced::mouse::Event::CursorLeft)
        | iced::Event::Touch(iced::touch::Event::FingerLifted { .. } | iced::touch::Event::FingerLost { .. })
        | iced::Event::Window(window::Event::Unfocused) => Some(Message::Writer(writer::Message::CancelChapterDrag)),
        iced::Event::Keyboard(keyboard::Event::KeyPressed { key: keyboard::Key::Named(keyboard::key::Named::Escape), .. }) =>
            Some(Message::Writer(writer::Message::CancelChapterDrag)),
        _ => None,
    }
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
    NovelCraft,
    TokyoNight,
    Dark,
    Nord,
    Dracula,
}

impl ThemeChoice {
    const ALL: [Self; 5] = [Self::NovelCraft, Self::TokyoNight, Self::Dark, Self::Nord, Self::Dracula];

    fn label(self) -> &'static str {
        match self {
            Self::NovelCraft => "NovelCraft (mockup)",
            Self::TokyoNight => "Tokyo Night",
            Self::Dark => "Dark",
            Self::Nord => "Nord",
            Self::Dracula => "Dracula",
        }
    }

    fn iced_theme(self) -> Theme {
        match self {
            Self::NovelCraft => mockup_style::mockup_theme(),
            Self::TokyoNight => Theme::TokyoNight,
            Self::Dark => Theme::Dark,
            Self::Nord => Theme::Nord,
            Self::Dracula => Theme::Dracula,
        }
    }
}

impl std::fmt::Display for ThemeChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(self.label()) }
}

#[derive(Debug, Clone)]
enum Message {
    Navigate(Page),
    ToggleFocusMode,
    ToggleFullscreen,
    EnterZen,
    ExitZen,
    ZenWindowReady(u64, Option<window::Id>),
    ZenModeReady(u64, window::Id, window::Mode),
    FullscreenWindowReady(u64, Option<window::Id>),
    FullscreenModeReady(u64, window::Id, window::Mode),
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
    fullscreen_revision: u64,
    fullscreen_pending: bool,
    fullscreen_notice: Option<String>,
    zen_mode: bool,
    zen_restore_window: Option<window::Id>,
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
            event::listen_with(chapter_drag_event),
            if self.writer.autosave_enabled() {
                iced::time::every(std::time::Duration::from_secs(30)).map(|_| Message::Writer(writer::Message::AutosaveTick))
            } else { Subscription::none() },
        ])
    }

    fn file_operation_in_progress(&self) -> bool {
        self.writer.is_busy() || self.library.is_busy() || self.reader.is_busy() || self.settings_busy
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::CloseRequested(id) => {
                // A delayed window query must not change fullscreen after a close request.
                self.fullscreen_revision = self.fullscreen_revision.wrapping_add(1);
                self.fullscreen_pending = false;
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
                if page != Page::Writer { let _ = self.writer.update(writer::Message::CancelChapterDrag); }
                let exit_zen = if self.zen_mode && page != Page::Writer { self.exit_zen() } else { Task::none() };
                if self.page == Page::Reader && page != Page::Reader {
                    self.reader.suspend_scroll_position();
                }
                if page != self.page {
                    self.focus_mode = false;
                }
                self.page = page;
                if page == Page::Reader && self.pending_close.is_none() {
                    Task::batch([exit_zen, self.reader.take_restore_task().map(Message::Reader)])
                } else if page == Page::Library {
                    Task::batch([
                        exit_zen,
                        self.library.update(library::Message::RefreshBookmarks).map(Message::Library),
                        self.library.update(library::Message::RefreshAvailability).map(Message::Library),
                    ])
                } else {
                    exit_zen
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
            Message::ToggleFullscreen if self.pending_close.is_none() && !self.fullscreen_pending && !self.zen_mode => {
                self.fullscreen_revision = self.fullscreen_revision.wrapping_add(1);
                self.fullscreen_pending = true;
                self.fullscreen_notice = None;
                let revision = self.fullscreen_revision;
                window::get_latest().map(move |id| Message::FullscreenWindowReady(revision, id))
            }
            Message::FullscreenWindowReady(revision, id)
                if self.fullscreen_pending && self.pending_close.is_none() && revision == self.fullscreen_revision =>
            {
                if let Some(id) = id {
                    window::get_mode(id).map(move |mode| Message::FullscreenModeReady(revision, id, mode))
                } else {
                    self.fullscreen_pending = false;
                    self.fullscreen_notice = Some("Jendela tidak ditemukan; fullscreen tidak diubah.".into());
                    Task::none()
                }
            }
            Message::FullscreenModeReady(revision, id, mode)
                if self.fullscreen_pending && self.pending_close.is_none() && revision == self.fullscreen_revision =>
            {
                self.fullscreen_pending = false;
                let Some(next) = fullscreen_target(mode) else {
                    self.fullscreen_notice = Some("Jendela tersembunyi; fullscreen tidak diubah.".into());
                    return Task::none();
                };
                self.reader.invalidate_scroll_position();
                window::change_mode(id, next)
            }
            Message::ToggleFullscreen | Message::FullscreenWindowReady(..) | Message::FullscreenModeReady(..) => Task::none(),
            Message::EnterZen if self.page == Page::Writer && self.pending_close.is_none()
                && !self.zen_mode && !self.fullscreen_pending && self.writer.can_enter_zen() => {
                self.zen_mode = true;
                self.zen_restore_window = None;
                self.fullscreen_notice = None;
                self.fullscreen_revision = self.fullscreen_revision.wrapping_add(1);
                self.fullscreen_pending = true;
                let revision = self.fullscreen_revision;
                window::get_latest().map(move |id| Message::ZenWindowReady(revision, id))
            }
            Message::ExitZen if self.zen_mode && self.pending_close.is_none() => self.exit_zen(),
            Message::ZenWindowReady(revision, id)
                if self.zen_mode && self.fullscreen_pending && self.pending_close.is_none() && revision == self.fullscreen_revision => {
                if let Some(id) = id {
                    window::get_mode(id).map(move |mode| Message::ZenModeReady(revision, id, mode))
                } else {
                    self.fullscreen_pending = false;
                    self.fullscreen_notice = Some("Jendela tidak ditemukan; mode Zen tanpa fullscreen.".into());
                    Task::none()
                }
            }
            Message::ZenModeReady(revision, id, mode)
                if self.zen_mode && self.fullscreen_pending && self.pending_close.is_none() && revision == self.fullscreen_revision => {
                self.fullscreen_pending = false;
                match mode {
                    window::Mode::Windowed => {
                        self.zen_restore_window = Some(id);
                        window::change_mode(id, window::Mode::Fullscreen)
                    }
                    window::Mode::Fullscreen => Task::none(),
                    window::Mode::Hidden => {
                        self.fullscreen_notice = Some("Jendela tersembunyi; mode Zen tanpa fullscreen.".into());
                        Task::none()
                    }
                }
            }
            Message::EnterZen | Message::ExitZen | Message::ZenWindowReady(..) | Message::ZenModeReady(..) => Task::none(),
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
                    Shortcut::ToggleFullscreen => Message::ToggleFullscreen,
                    Shortcut::ToggleMenu if self.page == Page::Writer && !self.zen_mode => Message::Writer(writer::Message::ToggleTools),
                    Shortcut::ToggleMenu if self.page == Page::Reader => Message::Reader(reader::Message::ToggleTools),
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
            Message::Writer(writer::Message::AutosaveTick) if self.pending_close.is_some() => Task::none(),
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
            Message::Library(library::Message::NewDraft) if self.page == Page::Library && self.pending_close.is_none() && self.library.can_open_book() => {
                let task = self.writer.update(writer::Message::NewDraft).map(Message::Writer);
                self.page = Page::Writer;
                self.focus_mode = false;
                task
            }
            Message::Library(library::Message::NewDraft) => Task::none(),
            Message::Library(library::Message::Open(path)) => self.open_library_book(path, false),
            Message::Library(library::Message::Continue(path)) => self.open_library_book(path, true),
            Message::Library(message) => self.library.update(message).map(Message::Library),
        }
    }

    fn exit_zen(&mut self) -> Task<Message> {
        self.zen_mode = false;
        self.fullscreen_revision = self.fullscreen_revision.wrapping_add(1);
        self.fullscreen_pending = false;
        if let Some(id) = self.zen_restore_window.take() {
            window::change_mode(id, window::Mode::Windowed)
        } else {
            Task::none()
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
            content = content.push(text("Draft, catatan, atau target harian memiliki perubahan yang belum disimpan."));
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
        let theme = self.theme_choice.iced_theme();
        let selector: Element<'_, Message> = if self.settings_loaded && !self.settings_busy {
            pick_list(ThemeChoice::ALL.to_vec(), Some(self.theme_choice), Message::SelectTheme)
                .width(210).padding([8, 12]).style(mockup_style::theme_select).into()
        } else {
            container(text(self.theme_choice.label()).size(13).color(mockup_style::muted(&theme)))
                .width(210).padding([8, 12]).style(mockup_style::card).into()
        };
        let mut content = column![
            text("Settings").size(30),
            text("Pengaturan tersimpan secara lokal di perangkat ini.").size(13).color(mockup_style::muted(&theme)),
            container(text("APPEARANCE").size(11).color(mockup_style::subtle(&theme)))
                .padding(iced::Padding { top: 36.0, bottom: 12.0, ..iced::Padding::default() }),
            container(row![
                column![text("Tema").size(15), text("Warna tampilan aplikasi").size(12).color(mockup_style::muted(&theme))].spacing(4).width(Length::Fill),
                selector,
            ].spacing(20).align_y(iced::Alignment::Center).wrap())
                .padding([18, 0]).width(Length::Fill).style(mockup_style::setting_row),
            text("Tema diterapkan setelah berhasil disimpan.").size(12).color(mockup_style::muted(&theme)),
        ].spacing(8).width(Length::Fill);
        if self.settings_busy { content = content.push(text("Memproses Settings...").size(13)); }
        if let Some(error) = &self.settings_error {
            content = content.push(text(error).size(13));
            if !self.settings_loaded && !self.settings_busy {
                content = content.push(button("Coba muat ulang Settings").style(mockup_style::secondary).on_press(Message::LoadSettings));
            }
        }
        scrollable(container(container(content).max_width(640).width(Length::Fill))
            .width(Length::Fill).center_x(Length::Fill).padding([32, 16]))
            .height(Length::Fill).into()
    }

    fn reader_font_status(&self) -> &'static str {
        if self.settings_busy { "Memproses Settings..." }
        else if self.settings_error.is_some() && !self.settings_loaded { "Settings gagal dimuat; perbaiki dan muat ulang di Settings." }
        else if self.settings_error.is_some() { "Settings gagal disimpan; coba lagi atau lihat Settings." }
        else if !self.settings_loaded { "Settings belum dimuat." }
        else { "Ukuran font disimpan secara lokal." }
    }

    fn view(&self) -> Element<'_, Message> {
        if self.pending_close.is_some() {
            return self.close_confirmation();
        }

        let theme = self.theme_choice.iced_theme();
        if self.zen_mode && self.page == Page::Writer {
            let mut bar = row![button("Keluar Zen").on_press(Message::ExitZen), horizontal_space()].spacing(12);
            if let Some(notice) = &self.fullscreen_notice { bar = bar.push(text(notice).size(12)); }
            return container(column![bar, self.writer.zen_view(&theme).map(Message::Writer)].spacing(12))
                .width(Length::Fill).height(Length::Fill).padding(32).into();
        }
        let is_workspace = matches!(self.page, Page::Writer | Page::Reader);
        let compact = match self.page {
            Page::Writer => !self.writer.tools_visible(),
            Page::Reader => !self.reader.tools_visible(),
            _ => false,
        };
        // Writer/Reader use one unified menu; focus mode hides only page navigation.
        let sidebar_hidden = compact || (self.focus_mode && !is_workspace);
        let mut navigation = Column::new().spacing(4).width(Length::Fill);
        if !sidebar_hidden {
            if is_workspace {
                let mut close = button(text("Tutup").size(12)).style(mockup_style::navigation_menu);
                let action = match self.page {
                    Page::Writer if self.writer.can_toggle_tools() => Some(Message::Writer(writer::Message::ToggleTools)),
                    Page::Reader if self.reader.can_toggle_tools() => Some(Message::Reader(reader::Message::ToggleTools)),
                    _ => None,
                };
                if let Some(action) = action { close = close.on_press(action); }
                let status = if self.page == Page::Writer { self.writer.compact_status() } else { self.reader.compact_status() };
                navigation = navigation.push(row![text("novelcraft").size(16), horizontal_space(), close].align_y(iced::Alignment::Center));
                navigation = navigation.push(text(status).size(12).color(mockup_style::muted(&theme)));
                navigation = navigation.push(Space::new(1, 16));
            }
            if !self.focus_mode {
                navigation = navigation.push(text("NAVIGASI").size(10).color(mockup_style::subtle(&theme)));
                for page in Page::ALL {
                    let selected = self.page == page;
                    let marker = if selected { theme.palette().primary } else { iced::Color::TRANSPARENT };
                    let label = row![
                        container(Space::new(3, 16)).style(move |_| container::Style::default().background(marker).border(iced::Border { radius: 2.0.into(), ..iced::Border::default() })),
                        text(page.title()).size(13), horizontal_space(),
                        text(page.shortcut_hint()).size(10).color(mockup_style::subtle(&theme)),
                    ].align_y(iced::Alignment::Center).spacing(6);
                    navigation = navigation.push(button(label).width(Length::Fill).padding([9, 8])
                        .style(move |theme, status| mockup_style::navigation(theme, status, selected))
                        .on_press(Message::Navigate(page)));
                }
            }
            if is_workspace {
                navigation = navigation.push(container(text(if self.page == Page::Writer { "WRITER" } else { "READER" }).size(10).color(mockup_style::subtle(&theme)))
                    .padding(iced::Padding { top: 20.0, bottom: 6.0, ..iced::Padding::default() }));
                let tools: Element<'_, Message> = if self.page == Page::Writer {
                    self.writer.tools_view(&theme).map(Message::Writer)
                } else {
                    self.reader.tools_view(self.settings_loaded && !self.settings_busy, self.reader_font_status()).map(Message::Reader)
                };
                navigation = navigation.push(tools);
                navigation = navigation.push(container(text("TAMPILAN").size(10).color(mockup_style::subtle(&theme)))
                    .padding(iced::Padding { top: 20.0, bottom: 6.0, ..iced::Padding::default() }));
                let mut fullscreen = button(text("Fullscreen  (F11)").size(13)).width(Length::Fill).style(mockup_style::navigation_menu);
                if !self.fullscreen_pending { fullscreen = fullscreen.on_press(Message::ToggleFullscreen); }
                let focus = button(text(if self.focus_mode { "Keluar mode fokus" } else { "Mode fokus" }).size(13))
                    .width(Length::Fill).style(mockup_style::navigation_menu).on_press(Message::ToggleFocusMode);
                navigation = navigation.push(fullscreen).push(focus);
                if self.page == Page::Writer {
                    let mut zen = button(text("Mode Zen").size(13)).width(Length::Fill).style(mockup_style::navigation_menu);
                    if self.writer.can_enter_zen() && !self.fullscreen_pending { zen = zen.on_press(Message::EnterZen); }
                    navigation = navigation.push(zen);
                }
            } else {
                let draft = self.writer.draft_stats_label();
                navigation = navigation.push(vertical_space()).push(
                    container(column![text("DRAFT SAAT INI").size(10).color(mockup_style::subtle(&theme)),
                        text(draft).size(18).color(theme.palette().primary),
                        text("Target per draft · sesi ini").size(11).color(mockup_style::muted(&theme)),
                    ].spacing(5)).width(Length::Fill).padding(12).style(mockup_style::card));
                navigation = navigation.push(text("v0.1.0 · lokal").size(10).color(mockup_style::subtle(&theme)));
            }
        }
        let sidebar_content: Element<'_, Message> = if is_workspace && !sidebar_hidden {
            scrollable(navigation).height(Length::Fill).into()
        } else { navigation.into() };
        let sidebar = container(sidebar_content)
            .width(if sidebar_hidden { 0_u16 } else if is_workspace { 280_u16 } else { 200_u16 })
            .height(Length::Fill)
            .padding(if sidebar_hidden { iced::Padding::default() } else { iced::Padding { top: 20.0, right: 14.0, bottom: 20.0, left: 14.0 } })
            .style(mockup_style::chrome);
        let page: Element<'_, Message> = match self.page {
            Page::Library => self.library.view(&theme).map(Message::Library),
            Page::Writer => self.writer.view(&theme).map(Message::Writer),
            Page::Reader => self.reader.view(&theme, self.settings_loaded && !self.settings_busy, self.reader_font_status()).map(Message::Reader),
            Page::Settings => self.settings_view(),
        };
        let content = container(page).width(Length::Fill).height(Length::Fill)
            .padding(if self.page == Page::Library { [40, 48] } else if is_workspace { [8, 16] } else { [24, 32] });
        let mut fullscreen = button(text("Fullscreen (F11)").size(12)).style(mockup_style::secondary);
        if !self.fullscreen_pending { fullscreen = fullscreen.on_press(Message::ToggleFullscreen); }
        let mut bar = row![
            text("novelcraft").size(14).font(iced::Font::MONOSPACE),
            text(format!("/ {}", self.page.title().to_lowercase())).size(12).color(mockup_style::muted(&theme)),
            horizontal_space(), fullscreen,
        ].align_y(iced::Alignment::Center).spacing(12);
        if let Some(notice) = &self.fullscreen_notice { bar = bar.push(text(notice).size(12)); }
        let topbar: Element<'_, Message> = if is_workspace {
            if let Some(notice) = &self.fullscreen_notice {
                container(text(notice).size(12)).padding([8, 16]).width(Length::Fill).into()
            } else { container(Space::new(0, 0)).height(0).into() }
        } else {
            container(bar).width(Length::Fill).padding([0, 20]).center_y(48).style(mockup_style::chrome).into()
        };
        let base = column![topbar, row![sidebar, content].height(Length::Fill)].height(Length::Fill);
        let mut trigger = button(text("Menu").size(12)).width(76).height(36).style(mockup_style::menu_trigger);
        let action = match self.page {
            Page::Writer if compact && self.writer.can_toggle_tools() => Some(Message::Writer(writer::Message::ToggleTools)),
            Page::Reader if compact && self.reader.can_toggle_tools() => Some(Message::Reader(reader::Message::ToggleTools)),
            _ => None,
        };
        let overlay: Element<'_, Message> = if let Some(action) = action {
            trigger = trigger.on_press(action);
            container(trigger).padding([4, 4]).into()
        } else { Space::new(0, 0).into() };
        stack![base, overlay].width(Length::Fill).height(Length::Fill).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zen_restores_only_fullscreen_it_started_and_ignores_late_results() {
        let mut app = NovelCraft::default();
        let _ = app.update(Message::Navigate(Page::Writer));
        let _ = app.update(Message::EnterZen);
        assert!(app.zen_mode);
        let revision = app.fullscreen_revision;
        let id = window::Id::unique();
        let _ = app.update(Message::ZenModeReady(revision, id, window::Mode::Windowed));
        assert_eq!(app.zen_restore_window, Some(id));
        let _ = app.update(Message::ExitZen);
        assert!(!app.zen_mode);
        assert_eq!(app.zen_restore_window, None);
        let _ = app.update(Message::ZenModeReady(revision, id, window::Mode::Windowed));
        assert!(!app.zen_mode);
        assert_eq!(app.zen_restore_window, None);
        let _ = app.update(Message::EnterZen);
        let revision = app.fullscreen_revision;
        let _ = app.update(Message::ZenModeReady(revision, id, window::Mode::Fullscreen));
        assert_eq!(app.zen_restore_window, None);
        drop(app.view());
        let _ = app.update(Message::Navigate(Page::Library));
        assert!(!app.zen_mode);
        assert_eq!(app.zen_restore_window, None);
    }

    #[test]
    fn zen_respects_writer_confirmation_and_close_guard() {
        let mut app = NovelCraft::default();
        let _ = app.update(Message::Navigate(Page::Writer));
        make_draft_dirty(&mut app);
        let _ = app.update(Message::Writer(writer::Message::NewDraft));
        let _ = app.update(Message::EnterZen);
        assert!(!app.zen_mode);
        assert!(app.writer.has_unsaved_changes());
        let _ = app.update(Message::Writer(writer::Message::CancelDiscard));
        let _ = app.update(Message::EnterZen);
        assert!(app.zen_mode);
        let id = window::Id::unique();
        let revision = app.fullscreen_revision;
        let _ = app.update(Message::CloseRequested(id));
        let _ = app.update(Message::ZenModeReady(revision, id, window::Mode::Windowed));
        assert_eq!(app.zen_restore_window, None);
        assert_eq!(app.pending_close, Some(id));
        assert!(app.writer.has_unsaved_changes());
    }

    #[test]
    fn fullscreen_target_uses_current_window_mode() {
        assert_eq!(fullscreen_target(window::Mode::Windowed), Some(window::Mode::Fullscreen));
        assert_eq!(fullscreen_target(window::Mode::Fullscreen), Some(window::Mode::Windowed));
        assert_eq!(fullscreen_target(window::Mode::Hidden), None);
    }

    #[test]
    fn fullscreen_queries_one_at_a_time_and_handles_missing_or_hidden_window() {
        let mut app = NovelCraft::default();
        let _ = app.update(Message::Shortcut(shortcuts::Shortcut::ToggleFullscreen));
        assert!(app.fullscreen_pending);
        let revision = app.fullscreen_revision;
        let _ = app.update(Message::ToggleFullscreen);
        assert_eq!(app.fullscreen_revision, revision);
        let _ = app.update(Message::FullscreenWindowReady(revision, None));
        assert!(!app.fullscreen_pending);
        assert!(app.fullscreen_notice.as_deref().unwrap().contains("tidak ditemukan"));
        let _ = app.update(Message::ToggleFullscreen);
        assert!(app.fullscreen_notice.is_none());
        let revision = app.fullscreen_revision;
        let id = window::Id::unique();
        let _ = app.update(Message::FullscreenWindowReady(revision, Some(id)));
        assert!(app.fullscreen_pending);
        let _ = app.update(Message::FullscreenModeReady(revision, id, window::Mode::Hidden));
        assert!(!app.fullscreen_pending);
        assert!(app.fullscreen_notice.as_deref().unwrap().contains("tersembunyi"));
        let _ = app.update(Message::ToggleFullscreen);
        let revision = app.fullscreen_revision;
        let _ = app.update(Message::FullscreenWindowReady(revision, Some(id)));
        let _ = app.update(Message::FullscreenModeReady(revision, id, window::Mode::Windowed));
        assert!(!app.fullscreen_pending);
        assert!(app.fullscreen_notice.is_none());
        drop(app.view());
    }

    #[test]
    fn fullscreen_cannot_bypass_close_guard_or_apply_stale_queries() {
        let mut app = NovelCraft::default();
        let _ = app.update(Message::Navigate(Page::Writer));
        make_draft_dirty(&mut app);
        let _ = app.update(Message::ToggleFocusMode);
        let _ = app.update(Message::ToggleFullscreen);
        let first = app.fullscreen_revision;
        let id = window::Id::unique();
        let _ = app.update(Message::FullscreenWindowReady(first, Some(id)));
        let _ = app.update(Message::CloseRequested(id));
        assert_eq!(app.pending_close, Some(id));
        assert!(!app.fullscreen_pending);
        let _ = app.update(Message::Shortcut(shortcuts::Shortcut::ToggleFullscreen));
        let _ = app.update(Message::ToggleFullscreen);
        assert!(!app.fullscreen_pending);
        let _ = app.update(Message::CancelClose);
        assert!(app.writer.has_unsaved_changes());
        assert!(app.focus_mode);
        let _ = app.update(Message::ToggleFullscreen);
        let second = app.fullscreen_revision;
        assert_ne!(first, second);
        let _ = app.update(Message::FullscreenModeReady(first, id, window::Mode::Windowed));
        assert!(app.fullscreen_pending);
        let _ = app.update(Message::FullscreenWindowReady(second, Some(id)));
        let _ = app.update(Message::FullscreenModeReady(second, id, window::Mode::Fullscreen));
        assert!(!app.fullscreen_pending);
        assert!(app.writer.has_unsaved_changes());
        assert!(app.focus_mode);
    }

    #[test]
    fn menu_shortcut_opens_workspace_panel_and_respects_confirmations() {
        use shortcuts::Shortcut;
        let mut app = NovelCraft::default();
        let _ = app.update(Message::Shortcut(Shortcut::ToggleMenu));
        assert!(!app.writer.tools_visible()); // Library has no workspace menu.
        let _ = app.update(Message::Navigate(Page::Writer));
        make_draft_dirty(&mut app);
        let _ = app.update(Message::Shortcut(Shortcut::ToggleMenu));
        assert!(app.writer.tools_visible());
        assert!(app.writer.has_unsaved_changes());
        drop(app.view());
        let _ = app.update(Message::Shortcut(Shortcut::ToggleMenu));
        assert!(!app.writer.tools_visible());
        let _ = app.update(Message::Writer(writer::Message::NewDraft));
        let _ = app.update(Message::Shortcut(Shortcut::ToggleMenu));
        assert!(!app.writer.tools_visible()); // The discard confirmation remains in view.
        assert!(app.writer.has_unsaved_changes());
        drop(app.view());
        let _ = app.update(Message::Writer(writer::Message::CancelDiscard));
        let _ = app.update(Message::Navigate(Page::Reader));
        let _ = app.update(Message::Shortcut(Shortcut::ToggleMenu));
        assert!(app.reader.tools_visible());
        drop(app.view());
        let _ = app.update(Message::Shortcut(Shortcut::ToggleMenu));
        assert!(!app.reader.tools_visible());
    }

    #[test]
    fn compact_writer_menu_retains_draft_and_keeps_navigation_accessible() {
        let mut app = NovelCraft::default();
        let _ = app.update(Message::Navigate(Page::Writer));
        assert!(!app.writer.tools_visible());
        make_draft_dirty(&mut app); drop(app.view());
        let _ = app.update(Message::Writer(writer::Message::ToggleTools));
        assert!(app.writer.tools_visible()); assert!(app.writer.has_unsaved_changes()); drop(app.view());
        let _ = app.update(Message::Writer(writer::Message::ToggleTools));
        assert!(!app.writer.tools_visible()); assert!(app.writer.has_unsaved_changes());
        let _ = app.update(Message::Shortcut(shortcuts::Shortcut::Library));
        assert_eq!(app.page, Page::Library); assert!(app.writer.has_unsaved_changes());
        let _ = app.update(Message::Shortcut(shortcuts::Shortcut::Writer));
        assert!(!app.writer.tools_visible());
        let id = window::Id::unique(); let _ = app.update(Message::CloseRequested(id));
        assert_eq!(app.pending_close, Some(id));
    }

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
    fn library_new_draft_respects_writer_and_close_guards() {
        let mut app = NovelCraft::default();
        make_draft_dirty(&mut app);
        let id = window::Id::unique();
        let _ = app.update(Message::CloseRequested(id));
        let _ = app.update(Message::Library(library::Message::NewDraft));
        assert_eq!(app.page, Page::Library);
        assert!(app.writer.has_unsaved_changes());
        let _ = app.update(Message::CancelClose);
        let _ = app.update(Message::Library(library::Message::NewDraft));
        assert_eq!(app.page, Page::Writer);
        assert!(app.writer.has_unsaved_changes());
        let _ = app.update(Message::Writer(writer::Message::CancelDiscard));
        assert!(app.writer.has_unsaved_changes());
        let _ = app.update(Message::Navigate(Page::Library));
        let _ = app.update(Message::Library(library::Message::NewDraft));
        let _ = app.update(Message::Writer(writer::Message::ConfirmDiscard));
        assert!(!app.writer.has_unsaved_changes());
    }

    #[test]
    fn application_defaults_to_mockup_theme() {
        assert_eq!(NovelCraft::default().theme_choice, ThemeChoice::NovelCraft);
        assert_eq!(NovelCraft::default().theme_choice.iced_theme().palette(), mockup_style::mockup_theme().palette());
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
        assert_eq!(app.theme_choice, ThemeChoice::NovelCraft);
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
