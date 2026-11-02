use iced::keyboard::{Key, Modifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    Library,
    Reader,
    Writer,
    Settings,
    NewDraft,
    Open,
    Save,
    SaveAs,
    PreviousSection,
    NextSection,
    ToggleFullscreen,
}

/// Linux shortcuts intentionally exclude Alt/AltGr and Super combinations.
pub fn from_key(key: Key<&str>, modifiers: Modifiers) -> Option<Shortcut> {
    if matches!(&key, Key::Named(iced::keyboard::key::Named::F11))
        && !modifiers.control() && !modifiers.shift() && !modifiers.alt() && !modifiers.logo()
    {
        return Some(Shortcut::ToggleFullscreen);
    }
    if !modifiers.control() || modifiers.alt() || modifiers.logo() {
        return None;
    }
    if !modifiers.shift() {
        match &key {
            Key::Named(iced::keyboard::key::Named::PageUp) => return Some(Shortcut::PreviousSection),
            Key::Named(iced::keyboard::key::Named::PageDown) => return Some(Shortcut::NextSection),
            _ => {}
        }
    }
    let Key::Character(key) = key else {
        return None;
    };
    if modifiers.shift() {
        return key.eq_ignore_ascii_case("s").then_some(Shortcut::SaveAs);
    }
    match key {
        "1" => Some(Shortcut::Library),
        "2" => Some(Shortcut::Reader),
        "3" => Some(Shortcut::Writer),
        "," => Some(Shortcut::Settings),
        _ if key.eq_ignore_ascii_case("n") => Some(Shortcut::NewDraft),
        _ if key.eq_ignore_ascii_case("o") => Some(Shortcut::Open),
        _ if key.eq_ignore_ascii_case("s") => Some(Shortcut::Save),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_navigation_and_writer_shortcuts() {
        let ctrl = Modifiers::CTRL;
        assert_eq!(from_key(Key::Character("1"), ctrl), Some(Shortcut::Library));
        assert_eq!(from_key(Key::Character("2"), ctrl), Some(Shortcut::Reader));
        assert_eq!(from_key(Key::Character("3"), ctrl), Some(Shortcut::Writer));
        assert_eq!(from_key(Key::Character(","), ctrl), Some(Shortcut::Settings));
        assert_eq!(from_key(Key::Character("n"), ctrl), Some(Shortcut::NewDraft));
        assert_eq!(from_key(Key::Character("o"), ctrl), Some(Shortcut::Open));
        assert_eq!(from_key(Key::Character("s"), ctrl), Some(Shortcut::Save));
        assert_eq!(from_key(Key::Character("S"), ctrl | Modifiers::SHIFT), Some(Shortcut::SaveAs));
    }

    #[test]
    fn recognizes_reader_section_shortcuts_only_without_other_modifiers() {
        use iced::keyboard::key::Named;
        assert_eq!(from_key(Key::Named(Named::PageUp), Modifiers::CTRL), Some(Shortcut::PreviousSection));
        assert_eq!(from_key(Key::Named(Named::PageDown), Modifiers::CTRL), Some(Shortcut::NextSection));
        assert_eq!(from_key(Key::Named(Named::PageUp), Modifiers::empty()), None);
        assert_eq!(from_key(Key::Named(Named::PageUp), Modifiers::CTRL | Modifiers::SHIFT), None);
        assert_eq!(from_key(Key::Named(Named::PageDown), Modifiers::CTRL | Modifiers::ALT), None);
        assert_eq!(from_key(Key::Named(Named::PageDown), Modifiers::CTRL | Modifiers::LOGO), None);
    }

    #[test]
    fn fullscreen_shortcut_requires_unmodified_f11() {
        use iced::keyboard::key::Named;
        assert_eq!(from_key(Key::Named(Named::F11), Modifiers::empty()), Some(Shortcut::ToggleFullscreen));
        assert_eq!(from_key(Key::Named(Named::F11), Modifiers::CTRL), None);
        assert_eq!(from_key(Key::Named(Named::F11), Modifiers::SHIFT), None);
        assert_eq!(from_key(Key::Named(Named::F11), Modifiers::ALT), None);
        assert_eq!(from_key(Key::Named(Named::F11), Modifiers::LOGO), None);
    }

    #[test]
    fn ignores_unrelated_modifiers_and_keys() {
        assert_eq!(from_key(Key::Character("s"), Modifiers::empty()), None);
        assert_eq!(from_key(Key::Character("s"), Modifiers::CTRL | Modifiers::ALT), None);
        assert_eq!(from_key(Key::Character("s"), Modifiers::CTRL | Modifiers::LOGO), None);
        assert_eq!(from_key(Key::Character("n"), Modifiers::CTRL | Modifiers::SHIFT), None);
        assert_eq!(from_key(Key::Character("c"), Modifiers::CTRL), None);
        assert_eq!(from_key(Key::Named(iced::keyboard::key::Named::Escape), Modifiers::CTRL), None);
    }
}
