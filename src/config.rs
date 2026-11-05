//! Uložené nastavení: poslední televize a přihlašovací údaje z párování.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::i18n::{Lang, tr, trf};
use crate::tv::Credentials;

/// Výchozí globální zkratky: zkratka (jako v xbindkeys) → akce (viz `Action::parse`).
const DEFAULT_HOTKEYS: [(&str, &str); 14] = [
    ("Super+Alt+t", "ShowWindow"),
    ("Super+Alt+Up", "CursorUp"),
    ("Super+Alt+Down", "CursorDown"),
    ("Super+Alt+Left", "CursorLeft"),
    ("Super+Alt+Right", "CursorRight"),
    ("Super+Alt+Return", "Confirm"),
    ("Super+Alt+BackSpace", "Back"),
    ("Super+Alt+Home", "Home"),
    ("Super+Alt+space", "PlayPause"),
    ("Super+Alt+Page_Up", "VolumeUp"),
    ("Super+Alt+Page_Down", "VolumeDown"),
    ("Super+Alt+m", "Mute"),
    ("Super+Alt+Insert", "PowerOn"),
    ("Super+Alt+Delete", "PowerOff"),
];

/// Výchozí klávesy v okně ovladače: klávesa (název podle egui) → akce (viz `Action::parse`).
const DEFAULT_WINDOW_KEYS: [(&str, &str); 14] = [
    ("Up", "CursorUp"),
    ("Down", "CursorDown"),
    ("Left", "CursorLeft"),
    ("Right", "CursorRight"),
    ("Enter", "Confirm"),
    ("Backspace", "Back"),
    ("Escape", "Back"),
    ("H", "Home"),
    ("Home", "Home"),
    ("Space", "PlayPause"),
    ("Plus", "VolumeUp"),
    ("Equals", "VolumeUp"),
    ("Minus", "VolumeDown"),
    ("M", "Mute"),
];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub host: Option<String>,
    pub name: Option<String>,
    /// MAC adresa pro Wake-on-LAN, zjištěná z ARP tabulky po prvním připojení.
    pub mac: Option<String>,
    pub credentials: Option<Credentials>,
    #[serde(default = "default_hotkeys")]
    pub hotkeys: BTreeMap<String, String>,
    /// Klávesy v okně ovladače (viz `window_keys`).
    #[serde(default = "default_window_keys")]
    pub window_keys: BTreeMap<String, String>,
    /// Jazyk rozhraní (`cs`, `en`); bez něj podle locale systému.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<Lang>,
    /// Rozložení tlačítek ovladače.
    #[serde(default)]
    pub layout: Layout,
    /// Soubor při startu nešel přečíst – aplikace běží s výchozím nastavením a nic neukládá.
    #[serde(skip)]
    load_error: Option<String>,
}

/// Rozložení tlačítek: jen nejčastější klávesy, nebo skoro celý dálkový ovladač.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    #[default]
    Minimal,
    Full,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: None,
            name: None,
            mac: None,
            credentials: None,
            hotkeys: default_hotkeys(),
            window_keys: default_window_keys(),
            language: None,
            layout: Layout::Minimal,
            load_error: None,
        }
    }
}

fn default_hotkeys() -> BTreeMap<String, String> {
    to_map(&DEFAULT_HOTKEYS)
}

fn default_window_keys() -> BTreeMap<String, String> {
    to_map(&DEFAULT_WINDOW_KEYS)
}

fn to_map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(key, action)| (key.to_string(), action.to_string()))
        .collect()
}

impl Config {
    /// Načte nastavení. Když soubor nejde přečíst (třeba chyba v JSON po ruční úpravě),
    /// vrátí výchozí nastavení s chybou (`load_error`) a takové se nikdy neuloží.
    pub fn load() -> Self {
        match path() {
            Some(path) => Self::load_from(&path),
            None => Self::default(),
        }
    }

    fn load_from(path: &Path) -> Self {
        match read(path) {
            Ok(config) => config.unwrap_or_default(),
            Err(err) => {
                let error = format!("{err:#}");
                eprintln!("{error}");
                Self {
                    load_error: Some(error),
                    ..Self::default()
                }
            }
        }
    }

    /// Proč soubor při startu nešel přečíst.
    pub fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    /// Uloží údaje, které mění aplikace: televizi, párování a rozložení. Zkratky, klávesy
    /// a jazyk zůstanou v souboru tak, jak je uživatel napsal, i když je upravil za běhu.
    /// Soubor, který nejde přečíst, nepřepíše.
    pub fn save(&self) -> Result<()> {
        let path = path().context(tr(
            "nelze zjistit adresář s nastavením",
            "cannot determine the configuration directory",
        ))?;
        self.save_to(&path)
    }

    fn save_to(&self, path: &Path) -> Result<()> {
        // Soubor nešel přečíst už při startu – okno na to upozorňuje, uživatel ho opraví.
        if self.load_error.is_some() {
            return Ok(());
        }
        let stored = match read(path)? {
            Some(mut stored) => {
                stored.host = self.host.clone();
                stored.name = self.name.clone();
                stored.mac = self.mac.clone();
                stored.credentials = self.credentials.clone();
                stored.layout = self.layout;
                stored
            }
            None => self.clone(),
        };
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        write_private(path, &serde_json::to_string_pretty(&stored)?)
            .with_context(|| trf!("nelze uložit {}", "cannot save {}", short_path(path)))
    }
}

/// Zapíše soubor tak, aby ho pád uprostřed zápisu nepoškodil (dočasný soubor + přejmenování)
/// a aby ho nikdy nešlo číst cizím uživatelům – obsahuje klíč k ovládání TV.
fn write_private(path: &Path, text: &str) -> io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp)?;
    // `mode` platí jen pro nově vytvořený soubor.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    fs::rename(&tmp, path)
}

/// Přečte soubor s nastavením; `None`, když ještě neexistuje.
fn read(path: &Path) -> Result<Option<Config>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err)
                .with_context(|| trf!("nelze přečíst {}", "cannot read {}", short_path(path)));
        }
    };
    let config = serde_json::from_str(&text)
        .with_context(|| trf!("chyba v {}", "error in {}", short_path(path)))?;
    Ok(Some(config))
}

/// Cesta pro hlášky, domovský adresář zkrácený na `~`.
fn short_path(path: &Path) -> String {
    match dirs::home_dir().and_then(|home| path.strip_prefix(home).ok().map(Path::to_owned)) {
        Some(relative) => format!("~/{}", relative.display()),
        None => path.display().to_string(),
    }
}

fn path() -> Option<PathBuf> {
    Some(
        dirs::config_dir()?
            .join("philips-controller")
            .join("config.json"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Testovací soubor s nastavením v prázdném adresáři, který po testu zmizí.
    struct TestFile(PathBuf);

    impl TestFile {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "philips-controller-test-{}-{name}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir.join("config.json"))
        }
    }

    impl Drop for TestFile {
        fn drop(&mut self) {
            if let Some(dir) = self.0.parent() {
                let _ = fs::remove_dir_all(dir);
            }
        }
    }

    #[test]
    fn broken_file_is_never_overwritten() {
        let file = TestFile::new("broken");
        let path = &file.0;
        let broken = "{ \"host\": \"192.0.2.10\",, }";
        fs::write(path, broken).unwrap();

        let mut config = Config::load_from(path);
        assert!(config.load_error().unwrap().contains("line 1"));
        assert_eq!(config.host, None);
        config.host = Some("192.0.2.20".into());
        config.save_to(path).unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), broken);
    }

    #[test]
    fn file_broken_while_running_is_not_overwritten() {
        let file = TestFile::new("broken-later");
        let path = &file.0;
        fs::write(path, r#"{ "host": "192.0.2.10" }"#).unwrap();
        let config = Config::load_from(path);
        assert!(config.load_error().is_none());

        let broken = "{ \"host\": ";
        fs::write(path, broken).unwrap();
        assert!(config.save_to(path).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), broken);
    }

    #[test]
    fn save_keeps_hotkeys_edited_while_running() {
        let file = TestFile::new("edited");
        let path = &file.0;
        fs::write(
            path,
            r#"{ "host": "192.0.2.10", "hotkeys": { "F1": "Mute" } }"#,
        )
        .unwrap();
        let mut config = Config::load_from(path);

        fs::write(
            path,
            r#"{ "host": "192.0.2.10", "hotkeys": { "F2": "Mute" }, "language": "cs" }"#,
        )
        .unwrap();
        config.host = Some("192.0.2.20".into());
        config.save_to(path).unwrap();

        let saved = Config::load_from(path);
        assert_eq!(saved.host.as_deref(), Some("192.0.2.20"));
        assert_eq!(saved.hotkeys.keys().collect::<Vec<_>>(), ["F2"]);
        assert_eq!(saved.language, Some(Lang::Cs));
    }

    #[test]
    fn missing_file_gets_created_with_defaults() {
        let file = TestFile::new("missing");
        let path = &file.0;
        let config = Config::load_from(path);
        assert!(config.load_error().is_none());
        config.save_to(path).unwrap();
        let saved = Config::load_from(path);
        assert_eq!(saved.hotkeys, default_hotkeys());
        assert_eq!(saved.window_keys, default_window_keys());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn layout_defaults_to_minimal() {
        let config: Config = serde_json::from_str(r#"{ "host": "192.0.2.10" }"#).unwrap();
        assert_eq!(config.layout, Layout::Minimal);
        let config: Config = serde_json::from_str(r#"{ "layout": "full" }"#).unwrap();
        assert_eq!(config.layout, Layout::Full);
    }
}
