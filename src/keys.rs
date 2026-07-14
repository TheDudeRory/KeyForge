use eframe::egui;

/// egui key names that global-hotkey's parser spells differently.
fn alias(name: &str) -> &str {
    match name {
        "Equals" => "Equal",
        "OpenBracket" => "BracketLeft",
        "CloseBracket" => "BracketRight",
        "Backtick" => "Backquote",
        other => other,
    }
}

/// Human-readable combo string ("Ctrl+Alt+K") that global-hotkey can parse back.
/// The Super/Win modifier is not capturable (egui doesn't report it) but parses
/// fine if typed into the JSON by hand.
pub fn combo_string(mods: egui::Modifiers, key: egui::Key) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if mods.ctrl {
        parts.push("Ctrl");
    }
    if mods.alt {
        parts.push("Alt");
    }
    if mods.shift {
        parts.push("Shift");
    }
    parts.push(alias(key.name()));
    parts.join("+")
}

/// Click-to-record hotkey field: click, press a combo (Esc cancels).
/// Returns true when a new combo was captured into `value`.
pub fn hotkey_capture(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    value: &mut String,
) -> bool {
    let my_id = ui.id().with(id_salt);
    // One global "currently capturing" slot so two fields can never record at once.
    let store = egui::Id::new("active_hotkey_capture");
    let active: Option<egui::Id> = ui.data(|d| d.get_temp(store)).unwrap_or(None);
    let capturing = active == Some(my_id);

    let mut changed = false;
    let mut stop = false;
    if capturing {
        let pressed = ui.input(|i| {
            i.events.iter().find_map(|ev| match ev {
                egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
                _ => None,
            })
        });
        match pressed {
            Some((egui::Key::Escape, _)) => stop = true,
            Some((key, mods)) => {
                *value = combo_string(mods, key);
                changed = true;
                stop = true;
            }
            None => {}
        }
    }

    let label = if capturing && !stop {
        "press keys… (Esc cancels)".to_owned()
    } else if value.is_empty() {
        "click to set".to_owned()
    } else {
        value.clone()
    };
    let response = ui.button(egui::RichText::new(label).monospace());

    // `!changed && !stop` keeps a captured Space/Enter from re-toggling via the
    // button's own keyboard activation.
    let toggled = response.clicked() && !changed && !stop;
    if toggled || stop {
        let new_active = if toggled && !capturing { Some(my_id) } else { None };
        ui.data_mut(|d| d.insert_temp(store, new_active));
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn combo_string_format() {
        let mods = egui::Modifiers { ctrl: true, alt: true, ..Default::default() };
        assert_eq!(combo_string(mods, egui::Key::K), "Ctrl+Alt+K");
        assert_eq!(combo_string(egui::Modifiers::NONE, egui::Key::F5), "F5");
        assert_eq!(combo_string(egui::Modifiers::SHIFT, egui::Key::Equals), "Shift+Equal");
    }

    /// Every combo the capture widget can emit for these keys must parse back
    /// with global-hotkey's parser — this pins the alias table.
    #[test]
    fn captured_names_parse() {
        use egui::Key::*;
        let keys = [
            A, K, Z, Num0, Num9, F1, F12, End, Home, PageUp, PageDown, Insert, Delete,
            Backspace, Enter, Space, Tab, ArrowUp, ArrowDown, ArrowLeft, ArrowRight,
            Minus, Period, Comma, Slash, Backslash, Semicolon, Quote, Equals,
            OpenBracket, CloseBracket, Backtick,
        ];
        for key in keys {
            let combo = combo_string(egui::Modifiers::CTRL | egui::Modifiers::ALT, key);
            assert!(
                global_hotkey::hotkey::HotKey::from_str(&combo).is_ok(),
                "unparseable capture: {combo}"
            );
        }
    }
}
