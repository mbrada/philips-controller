//! Jazyk rozhraní: čeština, nebo angličtina.
//!
//! Rozhoduje `language` v konfiguraci, jinak locale systému (`LANGUAGE`, `LC_ALL`,
//! `LC_MESSAGES`, `LANG`): `cs…` znamená češtinu, cokoli jiného angličtinu.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    Cs,
    En,
}

static CURRENT: OnceLock<Lang> = OnceLock::new();

/// Nastaví jazyk rozhraní; volá se jednou při startu, než se zobrazí první text.
pub fn init(configured: Option<Lang>) {
    let _ = CURRENT.set(configured.unwrap_or_else(from_env));
}

pub fn current() -> Lang {
    *CURRENT.get_or_init(from_env)
}

/// Text v jazyce rozhraní.
pub fn tr(cs: &'static str, en: &'static str) -> &'static str {
    match current() {
        Lang::Cs => cs,
        Lang::En => en,
    }
}

/// `format!` se dvěma jazykovými variantami: `trf!("Česky {x}", "In English {x}")`.
macro_rules! trf {
    ($cs:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        match $crate::i18n::current() {
            $crate::i18n::Lang::Cs => format!($cs $(, $arg)*),
            $crate::i18n::Lang::En => format!($en $(, $arg)*),
        }
    };
}
pub(crate) use trf;

fn from_env() -> Lang {
    let locale = ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()));
    from_locale(locale.as_deref().unwrap_or_default())
}

fn from_locale(locale: &str) -> Lang {
    // `LANGUAGE` bývá seznam (`cs:en`) – rozhoduje první položka.
    if locale.starts_with("cs") {
        Lang::Cs
    } else {
        Lang::En
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn czech_locale_selects_czech() {
        assert_eq!(from_locale("cs_CZ.UTF-8"), Lang::Cs);
        assert_eq!(from_locale("cs:en"), Lang::Cs);
        assert_eq!(from_locale("en_US.UTF-8"), Lang::En);
        assert_eq!(from_locale("en_US:cs"), Lang::En);
        assert_eq!(from_locale("C"), Lang::En);
        assert_eq!(from_locale(""), Lang::En);
    }

    #[test]
    fn language_in_config_is_lowercase() {
        assert_eq!(serde_json::to_string(&Lang::Cs).unwrap(), "\"cs\"");
        assert_eq!(serde_json::from_str::<Lang>("\"en\"").unwrap(), Lang::En);
    }
}
