//! Visual tokens adapted from mockup.html. Other saved themes keep their palettes.
use iced::widget::{button, container, pick_list, text_editor, text_input};
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

fn blend(base: Color, other: Color, amount: f32) -> Color {
    Color::from_rgb(
        base.r + (other.r - base.r) * amount,
        base.g + (other.g - base.g) * amount,
        base.b + (other.b - base.b) * amount,
    )
}

fn raised(theme: &Theme) -> Color {
    if is_mockup(theme) { rgb(20, 20, 20) }
    else { blend(theme.palette().background, theme.palette().text, 0.04) }
}

fn surface(theme: &Theme) -> Color {
    if is_mockup(theme) { rgb(26, 26, 26) }
    else { blend(theme.palette().background, theme.palette().text, 0.08) }
}

pub fn chrome(theme: &Theme) -> container::Style {
    container::Style::default().background(raised(theme))
}

pub fn sidebar(theme: &Theme) -> container::Style {
    container::Style::default().background(raised(theme)).border(Border {
        color: theme.palette().text.scale_alpha(0.08),
        width: 1.0,
        radius: 0.0.into(),
    })
}

pub fn card(theme: &Theme) -> container::Style {
    container::Style::default().background(raised(theme)).border(Border {
        color: theme.palette().text.scale_alpha(0.10), width: 1.0, radius: 10.0.into(),
    })
}

pub fn navigation(theme: &Theme, status: button::Status, selected: bool) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    button::Style {
        background: (selected || hovered).then(|| surface(theme).into()),
        text_color: if selected || hovered { theme.palette().text } else { muted(theme) },
        border: Border { radius: 6.0.into(), ..Border::default() },
        ..button::Style::default()
    }
}

pub fn navigation_menu(theme: &Theme, status: button::Status) -> button::Style {
    navigation(theme, status, false)
}

pub fn theme_select(theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let mut style = pick_list::default(theme, status);
    style.background = surface(theme).into();
    style.text_color = theme.palette().text;
    style.handle_color = muted(theme);
    style.border = Border {
        color: if matches!(status, pick_list::Status::Hovered | pick_list::Status::Opened) { theme.palette().primary } else { theme.palette().text.scale_alpha(0.12) },
        width: 1.0, radius: 6.0.into(),
    };
    style
}

pub fn search(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let palette = theme.palette();
    let disabled = status == text_input::Status::Disabled;
    text_input::Style {
        background: surface(theme).into(),
        border: Border {
            color: if status == text_input::Status::Focused { palette.primary }
                else { palette.text.scale_alpha(0.05) },
            width: 1.0,
            radius: 6.0.into(),
        },
        icon: muted(theme),
        placeholder: muted(theme),
        value: if disabled { muted(theme) } else { palette.text },
        selection: palette.primary.scale_alpha(0.3),
    }
}

// A quiet writing surface: no bright outline when focused or hovered.
pub fn writer_editor(theme: &Theme, status: text_editor::Status) -> text_editor::Style {
    let mut style = text_editor::default(theme, status);
    style.background = theme.palette().background.into();
    style.border = Border::default();
    style
}

pub fn secondary(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.palette();
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    let disabled = status == button::Status::Disabled;
    button::Style {
        background: Some(if hovered { blend(surface(theme), palette.text, 0.05) } else { surface(theme) }.into()),
        text_color: if disabled { muted(theme) } else { palette.text },
        border: Border { color: palette.text.scale_alpha(0.10), width: 1.0, radius: 6.0.into() },
        ..button::Style::default()
    }
}

pub fn primary(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.palette();
    let disabled = status == button::Status::Disabled;
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
    let accent = if hovered { blend(palette.primary, palette.text, 0.12) } else { palette.primary };
    button::Style {
        background: Some(if disabled { blend(surface(theme), accent, 0.25) } else { accent }.into()),
        text_color: if disabled { muted(theme) } else { palette.background },
        border: Border { radius: 6.0.into(), ..Border::default() },
        ..button::Style::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Background;

    #[test]
    fn mockup_search_uses_subtle_border_and_visible_focus() {
        let theme = mockup_theme();
        let normal = search(&theme, text_input::Status::Active);
        let focused = search(&theme, text_input::Status::Focused);
        assert_eq!(normal.background, Background::Color(rgb(26, 26, 26)));
        assert!(normal.border.color.a < 0.1);
        assert_eq!(focused.border.color, theme.palette().primary);
        assert_eq!(normal.border.radius, 6.0.into());
    }

    #[test]
    fn writer_editor_has_no_bright_border_in_any_state() {
        for theme in [mockup_theme(), Theme::Nord, Theme::Dark, Theme::Dracula] {
            for status in [text_editor::Status::Active, text_editor::Status::Hovered, text_editor::Status::Focused, text_editor::Status::Disabled] {
                let style = writer_editor(&theme, status);
                assert_eq!(style.border.width, 0.0);
                assert_eq!(style.background, Background::Color(theme.palette().background));
            }
        }
    }

    #[test]
    fn saved_dark_themes_keep_dark_chrome_and_disabled_controls() {
        for theme in [Theme::Nord, Theme::TokyoNight, Theme::Dark, Theme::Dracula] {
            let color = raised(&theme);
            assert!(color.r < 0.5 && color.g < 0.5 && color.b < 0.5);
            assert_eq!(primary(&theme, button::Status::Disabled).text_color, muted(&theme));
        }
    }
}
