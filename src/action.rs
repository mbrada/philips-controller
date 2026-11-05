//! Akce vyvolané mimo okno: z ikony v liště, globální zkratkou nebo dalším spuštěním aplikace.

use crate::i18n::tr;

/// Klávesy JointSpace API, které lze přiřadit zkratkám, s popisem pro nápovědu (česky, anglicky).
pub const TV_KEYS: [(&str, &str, &str); 44] = [
    ("CursorUp", "šipka nahoru", "arrow up"),
    ("CursorDown", "šipka dolů", "arrow down"),
    ("CursorLeft", "šipka doleva", "arrow left"),
    ("CursorRight", "šipka doprava", "arrow right"),
    ("Confirm", "OK", "OK"),
    ("Back", "zpět", "back"),
    ("Home", "domů", "home"),
    ("Options", "možnosti", "options"),
    ("Info", "info", "info"),
    ("Find", "hledat", "search"),
    ("Source", "zdroj", "source"),
    ("WatchTV", "sledovat TV", "watch TV"),
    ("PlayPause", "pauza / pokračovat", "play / pause"),
    ("Play", "pokračovat", "play"),
    ("Pause", "pauza", "pause"),
    ("Stop", "stop", "stop"),
    ("Rewind", "přetočit zpět", "rewind"),
    ("FastForward", "přetočit vpřed", "fast forward"),
    ("Previous", "předchozí", "previous"),
    ("Next", "další", "next"),
    ("Record", "nahrávat", "record"),
    ("VolumeUp", "hlasitost +", "volume +"),
    ("VolumeDown", "hlasitost −", "volume −"),
    ("Mute", "ztlumit", "mute"),
    ("ChannelStepUp", "kanál +", "channel +"),
    ("ChannelStepDown", "kanál −", "channel −"),
    ("Standby", "standby (přepnout)", "standby (toggle)"),
    ("AmbilightOnOff", "Ambilight", "Ambilight"),
    ("Subtitle", "titulky", "subtitles"),
    ("Teletext", "teletext", "teletext"),
    ("RedColour", "červené tlačítko", "red button"),
    ("GreenColour", "zelené tlačítko", "green button"),
    ("YellowColour", "žluté tlačítko", "yellow button"),
    ("BlueColour", "modré tlačítko", "blue button"),
    ("Digit0", "0", "0"),
    ("Digit1", "1", "1"),
    ("Digit2", "2", "2"),
    ("Digit3", "3", "3"),
    ("Digit4", "4", "4"),
    ("Digit5", "5", "5"),
    ("Digit6", "6", "6"),
    ("Digit7", "7", "7"),
    ("Digit8", "8", "8"),
    ("Digit9", "9", "9"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Key(&'static str),
    PowerOn,
    PowerOff,
    ShowWindow,
    HideWindow,
    ToggleWindow,
    Quit,
}

impl Action {
    /// Název akce v konfiguraci: klávesa ovladače (`VolumeUp`) nebo `PowerOn`,
    /// `PowerOff`, `ShowWindow`, `ToggleWindow`.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "PowerOn" => Some(Self::PowerOn),
            "PowerOff" => Some(Self::PowerOff),
            "ShowWindow" => Some(Self::ShowWindow),
            "ToggleWindow" => Some(Self::ToggleWindow),
            _ => TV_KEYS
                .iter()
                .find(|(key, _, _)| *key == name)
                .map(|(key, _, _)| Self::Key(key)),
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Self::Key(key) => TV_KEYS
                .iter()
                .find(|(known, _, _)| *known == key)
                .map_or(key, |(_, cs, en)| tr(cs, en)),
            Self::PowerOn => tr("zapnout TV", "turn TV on"),
            Self::PowerOff => tr("vypnout TV", "turn TV off"),
            // Globální zkratka nad aktivním ovladačem ho schová (viz `hotkeys`).
            Self::ShowWindow => tr("zobrazit / skrýt ovladač", "show / hide the remote"),
            Self::HideWindow => tr("skrýt ovladač", "hide the remote"),
            Self::ToggleWindow => tr("zobrazit / skrýt ovladač", "show / hide the remote"),
            Self::Quit => tr("ukončit", "quit"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_keys_and_special_actions() {
        assert_eq!(Action::parse("VolumeUp"), Some(Action::Key("VolumeUp")));
        assert_eq!(Action::parse("ShowWindow"), Some(Action::ShowWindow));
        assert_eq!(Action::parse("volumeup"), None);
        assert_eq!(Action::Key("Mute").describe(), tr("ztlumit", "mute"));
        assert_eq!(Action::Key("Unknown").describe(), "Unknown");
    }
}
