//! Klávesy v okně ovladače, nastavitelné v sekci `window_keys` konfigurace.

use std::collections::BTreeMap;

use eframe::egui::Key;

use crate::action::{Action, TV_KEYS};
use crate::i18n::{tr, trf};

/// Klávesa v okně a akce, kterou vyvolá.
pub struct WindowKey {
    pub key: Key,
    pub action: Action,
}

/// Přečte sekci `window_keys`: název klávesy, jak ho zná egui (`Up`, `Enter`, `Space`, `M`,
/// `F1`, `+`…) → akce (viz `Action::parse`). Vrátí platné klávesy seřazené podle akcí
/// a popisy chyb pro nápovědu.
pub fn parse(config: &BTreeMap<String, String>) -> (Vec<WindowKey>, Vec<String>) {
    let mut keys: Vec<WindowKey> = Vec::new();
    let mut errors = Vec::new();
    for (name, action_name) in config {
        let Some(key) = Key::from_name(name.trim()) else {
            errors.push(if name.len() > 1 && name.contains('+') {
                trf!(
                    "„{name}“: v okně jdou jen samostatné klávesy bez modifikátorů",
                    "“{name}”: window keys cannot have modifiers"
                )
            } else {
                trf!("„{name}“: neznámá klávesa", "“{name}”: unknown key")
            });
            continue;
        };
        let Some(action) = Action::parse(action_name.trim()) else {
            errors.push(trf!(
                "„{name}“: neznámá akce „{action_name}“",
                "“{name}”: unknown action “{action_name}”"
            ));
            continue;
        };
        // Stejnou klávesu jde zapsat víc jmény (`Plus`, `+`).
        if keys.iter().any(|known| known.key == key) {
            errors.push(trf!(
                "„{name}“: klávesa je v seznamu víckrát",
                "“{name}”: the key is listed more than once"
            ));
            continue;
        }
        keys.push(WindowKey { key, action });
    }
    keys.sort_by_key(|window_key| rank(window_key.action));
    (keys, errors)
}

/// Klávesy seskupené podle akce pro nápovědu, např. („H / Home“, domů).
pub fn grouped(keys: &[WindowKey]) -> Vec<(String, Action)> {
    let mut groups: Vec<(Vec<&str>, Action)> = Vec::new();
    for window_key in keys {
        let label = label(window_key.key);
        match groups
            .iter_mut()
            .find(|(_, action)| *action == window_key.action)
        {
            Some((labels, _)) => labels.push(label),
            None => groups.push((vec![label], window_key.action)),
        }
    }
    groups
        .into_iter()
        .map(|(mut labels, action)| {
            labels.sort_unstable();
            (labels.join(" / "), action)
        })
        .collect()
}

fn label(key: Key) -> &'static str {
    match key {
        Key::Space => tr("Mezerník", "Space"),
        Key::Escape => "Esc",
        _ => key.symbol_or_name(),
    }
}

/// Pořadí v nápovědě: klávesy TV jako v `TV_KEYS`, pak ostatní akce.
fn rank(action: Action) -> usize {
    const OTHERS: [Action; 4] = [
        Action::PowerOn,
        Action::PowerOff,
        Action::ShowWindow,
        Action::ToggleWindow,
    ];
    match action {
        Action::Key(key) => TV_KEYS
            .iter()
            .position(|(known, _, _)| *known == key)
            .unwrap_or(TV_KEYS.len()),
        other => {
            TV_KEYS.len()
                + 1
                + OTHERS
                    .iter()
                    .position(|known| *known == other)
                    .unwrap_or(OTHERS.len())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, action)| (key.to_string(), action.to_string()))
            .collect()
    }

    #[test]
    fn default_window_keys_are_valid() {
        let (keys, errors) = parse(&Config::default().window_keys);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(keys.len(), Config::default().window_keys.len());
        assert_eq!(keys[0].key, Key::ArrowUp);
        assert_eq!(keys[0].action, Action::Key("CursorUp"));
    }

    #[test]
    fn reports_invalid_entries() {
        let config = map(&[
            ("Ctrl+M", "Mute"),
            ("Foo", "Mute"),
            ("F1", "Nothing"),
            ("Plus", "VolumeUp"),
            ("+", "VolumeDown"),
            ("P", "PowerOn"),
        ]);
        let (keys, errors) = parse(&config);
        assert_eq!(errors.len(), 4, "{errors:?}");
        // `+` se seřadí před `Plus`, takže platí jeho akce.
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].key, Key::Plus);
        assert_eq!(keys[0].action, Action::Key("VolumeDown"));
        assert_eq!(keys[1].action, Action::PowerOn);
    }

    #[test]
    fn groups_keys_by_action() {
        let config = map(&[("Home", "Home"), ("H", "Home"), ("Up", "CursorUp")]);
        let (keys, _) = parse(&config);
        let groups = grouped(&keys);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0], ("⏶".to_owned(), Action::Key("CursorUp")));
        assert_eq!(groups[1], ("H / Home".to_owned(), Action::Key("Home")));
    }
}
