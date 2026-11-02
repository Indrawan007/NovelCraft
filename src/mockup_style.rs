//! Visual tokens adapted from mockup.html. Other saved themes keep their palettes.
use iced::widget::{button, container};
use iced::{Border, Color, Theme};
use std::sync::OnceLock;

fn rgb(r: u8, g: u8, b: u8) -> Color { Color::from_rgb8(r, g, b) }

pub fn mockup_theme() -> Theme {
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME.get_or_init(|| Theme::custom("NovelCraft".into(), iced::theme::Palette {
        background: rgb(12, 12, 12),
        text: rgb(240, 236, 228),
        primary: rgb(212, 168, 83),
        success: rgb(126, 184, 122),
        danger: rgb(196, 107, 107),
    })).clone()
}

fn is_mockup(theme: &Theme) -> bool { matches!(theme, Theme::Custom(_)) }

pub fn muted(theme: &Theme) -> Color {
    if is_mockup(theme) { rgb(160, 154, 142) } else { theme.palette().text.scale_alpha(0.70) }
}

pub fn subtle(theme: &Theme) -> Color {
    if is_mockup(theme) { rgb(115, 110, 101) } else { theme.palette().text.scale_alpha(0.58) }
}

pub fn chrome(theme: &Theme) -> container::Style {
    let color = if is_mockup(theme) { rgb(20, 20, 20) } else { theme.extended_palette().background.weak.color };
    container::Style::default().background(color)
}

pub fn card(theme: &Theme) -> container::Style {
    let color = if is_mockup(theme) { rgb(20, 20, 20) } else { theme.extended_palette().background.weak.color };
    container::Style::default().background(color).border(Border {
        color: theme.palette().text.scale_alpha(0.10), width: 1.0, radius: 10.0.into(),
    })
}

pub fn navigation(theme: &Theme, status: button::Status, selected: bool) -> button::Style {
    if !is_mockup(theme) {
        return if selected { button::secondary(theme, status) } else { button::text(theme, status) };
    }
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: (selected || hovered).then(|| rgb(26, 26, 26).into()),
        text_color: if selected || hovered { theme.palette().text } else { muted(theme) },
        border: Border { radius: 6.0.into(), ..Border::default() },
        ..button::Style::default()
    }
}
