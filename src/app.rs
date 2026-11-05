//! Okno ovladače.

use std::os::unix::net::UnixListener;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align2, Color32, FontId, Key, Painter, Pos2, Rect, RichText, Role, Sense, Stroke,
    StrokeKind, Vec2, ViewportCommand, WidgetInfo,
};

use crate::action::Action;
use crate::config::{Config, Layout};
use crate::discovery::FoundTv;
#[cfg(target_os = "linux")]
use crate::hotkeys::{self, Hotkey};
use crate::i18n::{tr, trf};
use crate::instance;
use crate::tray::Tray;
use crate::window_keys::{self, WindowKey};
use crate::worker::{self, Command, Event};

/// Jak dlouho tlačítko svítí po stisku klávesy.
const FLASH: Duration = Duration::from_millis(180);
const GAP: f32 = 8.0;
const CELL: Vec2 = Vec2::new(76.0, 58.0);
const WIDE: Vec2 = Vec2::new((CELL.x * 3.0 + GAP) / 2.0, 56.0);
/// Šířka tří buněk – podle ní se zarovnávají řady se čtyřmi tlačítky.
const ROW_WIDTH: f32 = CELL.x * 3.0 + GAP * 2.0;
/// Mezera mezi skupinami tlačítek v plném rozložení (navíc k `GAP`).
const SECTION: f32 = 6.0;
const POWER_ON: Color32 = Color32::from_rgb(46, 160, 67);
const POWER_OFF: Color32 = Color32::from_rgb(218, 54, 51);
const COLOR_KEYS: [(&str, Color32); 4] = [
    ("RedColour", Color32::from_rgb(214, 64, 61)),
    ("GreenColour", Color32::from_rgb(56, 163, 79)),
    ("YellowColour", Color32::from_rgb(228, 178, 40)),
    ("BlueColour", Color32::from_rgb(52, 120, 214)),
];

/// Velikost okna pro dané rozložení (okno má pevnou velikost).
pub fn window_size(layout: Layout) -> Vec2 {
    match layout {
        Layout::Minimal => Vec2::new(320.0, 720.0),
        Layout::Full => Vec2::new(320.0, 960.0),
    }
}

#[cfg(not(target_os = "linux"))]
struct Hotkey {
    combo: String,
    action: Option<Action>,
    action_name: String,
    error: Option<String>,
}

enum Status {
    Info(String),
    Error(String),
    /// TV neodpovídá, ovladač se k ní připojí sám.
    Offline(String),
}

enum Glyph {
    /// Symbol nebo číslice.
    Text(&'static str),
    /// Krátký nápis místo symbolu (Titulky, Teletext).
    Word(&'static str),
    Power,
    /// Barevné tlačítko.
    Color(Color32),
}

impl From<&'static str> for Glyph {
    fn from(text: &'static str) -> Self {
        Self::Text(text)
    }
}

struct ConnectedTv {
    name: String,
    host: String,
}

pub struct RemoteApp {
    commands: Sender<Command>,
    events: Receiver<Event>,
    /// Akce z lišty, globálních zkratek a dalších spuštění aplikace.
    actions: Receiver<Action>,
    tray: Option<Tray>,
    hotkeys: Result<Vec<Hotkey>, String>,
    /// Klávesy v okně ovladače a chyby v jejich nastavení.
    window_keys: Vec<WindowKey>,
    window_key_errors: Vec<String>,
    tv: Option<ConnectedTv>,
    /// TV odpovídá (jinak se worker snaží připojit znovu).
    online: bool,
    needs_pairing: bool,
    pairing_active: bool,
    focus_pin: bool,
    pin: String,
    manual_host: String,
    found: Vec<FoundTv>,
    busy: Option<String>,
    status: Option<Status>,
    /// Soubor s nastavením nejde přečíst – aplikace ho nepřepíše.
    config_error: Option<String>,
    /// Vynucené otevření/zavření sekce Připojení při změně stavu.
    connection_open: Option<bool>,
    flash: Option<(&'static str, Instant)>,
    layout: Layout,
    visible: bool,
    quitting: bool,
}

impl RemoteApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        listener: Option<UnixListener>,
        start_hidden: bool,
        config: &Config,
    ) -> Self {
        let ctx = cc.egui_ctx.clone();
        let (commands, events) = worker::spawn(ctx.clone(), config.clone());
        let (action_tx, actions) = channel();
        let tray = Tray::spawn(action_tx.clone(), ctx.clone())
            .map_err(|err| {
                eprintln!(
                    "{}",
                    trf!(
                        "Ikonu v liště se nepodařilo vytvořit: {err:#}",
                        "Failed to create the tray icon: {err:#}"
                    )
                );
            })
            .ok();
        #[cfg(target_os = "linux")]
        let hotkeys = hotkeys::spawn(&config.hotkeys, action_tx.clone(), ctx.clone())
            .map_err(|err| format!("{err:#}"));
        #[cfg(not(target_os = "linux"))]
        let hotkeys = Err(tr(
            "globální zkratky jsou jen pro X11",
            "global hotkeys are only available on X11",
        )
        .to_owned());
        if let Some(listener) = listener {
            instance::listen(listener, action_tx, ctx.clone());
        }
        let (window_keys, window_key_errors) = window_keys::parse(&config.window_keys);
        let config_error = config.load_error().map(str::to_owned);
        // Bez ikony v liště by se skryté okno nedalo otevřít; chybu v nastavení je potřeba vidět.
        let visible = !start_hidden || tray.is_none() || config_error.is_some();
        if !visible {
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        } else if start_hidden {
            ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        }
        let _ = commands.send(Command::Visible(visible));
        Self {
            commands,
            events,
            actions,
            tray,
            hotkeys,
            window_keys,
            window_key_errors,
            tv: None,
            online: false,
            needs_pairing: false,
            pairing_active: false,
            focus_pin: false,
            pin: String::new(),
            manual_host: String::new(),
            found: Vec::new(),
            busy: None,
            status: None,
            config_error,
            connection_open: None,
            flash: None,
            layout: config.layout,
            visible,
            quitting: false,
        }
    }

    fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    fn press(&mut self, key: &'static str) {
        self.send(Command::Key(key, Instant::now()));
        self.flash = Some((key, Instant::now()));
        if matches!(self.status, Some(Status::Error(_))) {
            self.status = None;
        }
    }

    fn is_lit(&self, key: &str) -> bool {
        self.flash
            .is_some_and(|(lit, at)| lit == key && at.elapsed() < FLASH)
    }

    fn process_events(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            if !matches!(event, Event::Busy(_)) {
                self.busy = None;
            }
            match event {
                Event::Busy(text) => self.busy = Some(text),
                Event::Info(text) => self.status = Some(Status::Info(text)),
                Event::Error(text) => {
                    self.status = Some(Status::Error(text));
                    if self.tv.is_none() {
                        self.connection_open = Some(true);
                    }
                }
                Event::Discovered(found) => {
                    self.status = Some(match found.len() {
                        0 => Status::Error(
                            tr(
                                "V síti jsem žádnou TV Philips nenašel",
                                "No Philips TV found on the network",
                            )
                            .into(),
                        ),
                        1 => Status::Info(tr("Nalezena 1 televize", "Found 1 TV").into()),
                        n @ 2..=4 => Status::Info(trf!(
                            "Nalezeny {n} televize – vyberte jednu",
                            "Found {n} TVs – pick one"
                        )),
                        n => Status::Info(trf!(
                            "Nalezeno {n} televizí – vyberte jednu",
                            "Found {n} TVs – pick one"
                        )),
                    });
                    if found.len() != 1 {
                        self.connection_open = Some(true);
                    }
                    self.found = found;
                }
                Event::Connected {
                    host,
                    name,
                    needs_pairing,
                } => {
                    self.status = Some(Status::Info(if needs_pairing {
                        tr(
                            "TV je potřeba jednou spárovat",
                            "The TV needs to be paired once",
                        )
                        .into()
                    } else {
                        tr(
                            "Připraveno – ovládejte šipkami",
                            "Ready – use the arrow keys",
                        )
                        .into()
                    }));
                    self.manual_host = host.clone();
                    self.tv = Some(ConnectedTv { name, host });
                    self.online = true;
                    self.needs_pairing = needs_pairing;
                    self.pairing_active = false;
                    self.connection_open = Some(false);
                }
                Event::ConfigError(text) => {
                    self.config_error.get_or_insert(text);
                }
                Event::Offline(text) => {
                    self.online = false;
                    self.status = Some(Status::Offline(text));
                }
                Event::PairingStarted => {
                    self.pairing_active = true;
                    self.focus_pin = true;
                    self.pin.clear();
                    self.status = Some(Status::Info(
                        tr("Na televizi se zobrazil PIN", "The TV is showing a PIN").into(),
                    ));
                }
                Event::Paired => {
                    self.needs_pairing = false;
                    self.pairing_active = false;
                    self.pin.clear();
                    self.status = Some(Status::Info(
                        tr(
                            "Spárováno ✔ – ovládejte šipkami",
                            "Paired ✔ – use the arrow keys",
                        )
                        .into(),
                    ));
                }
            }
        }
    }

    fn process_actions(&mut self, ctx: &egui::Context) {
        while let Ok(action) = self.actions.try_recv() {
            self.perform(ctx, action);
        }
    }

    fn perform(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::Key(key) => self.press(key),
            Action::PowerOn => self.send(Command::PowerOn),
            Action::PowerOff => self.send(Command::PowerOff),
            Action::ShowWindow => self.set_visible(ctx, true),
            Action::HideWindow => self.set_visible(ctx, false),
            Action::ToggleWindow => self.set_visible(ctx, !self.visible),
            Action::Quit => {
                self.quitting = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }
    }

    fn set_visible(&mut self, ctx: &egui::Context, visible: bool) {
        self.visible = visible;
        self.send(Command::Visible(visible));
        ctx.send_viewport_cmd(ViewportCommand::Visible(visible));
        if visible {
            ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(ViewportCommand::Focus);
        }
    }

    /// Převezme klávesy dřív, než je uvidí widgety (šipky by jinak přesouvaly fokus).
    fn handle_keyboard(&mut self, ui: &mut egui::Ui) {
        if ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let mut pressed = Vec::new();
        ui.input_mut(|input| {
            input.events.retain(|event| {
                let egui::Event::Key {
                    key,
                    pressed: true,
                    repeat,
                    modifiers,
                    ..
                } = event
                else {
                    return true;
                };
                if modifiers.ctrl || modifiers.alt || modifiers.command {
                    return true;
                }
                let Some(window_key) = self.window_keys.iter().find(|known| known.key == *key)
                else {
                    return true;
                };
                // Držená klávesa opakuje jen klávesy TV (hlasitost…), ne zapnutí nebo skrytí okna.
                if !*repeat || matches!(window_key.action, Action::Key(_)) {
                    pressed.push(window_key.action);
                }
                false
            });
        });
        let ctx = ui.ctx().clone();
        for action in pressed {
            match action {
                // Okno má fokus, takže „zobrazit / skrýt“ ho schová.
                Action::ShowWindow => self.perform(&ctx, Action::ToggleWindow),
                action => self.perform(&ctx, action),
            }
        }
    }

    fn set_layout(&mut self, ctx: &egui::Context, layout: Layout) {
        self.layout = layout;
        self.send(Command::SaveLayout(layout));
        ctx.send_viewport_cmd(ViewportCommand::InnerSize(window_size(layout)));
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        let (color, state) = match (&self.tv, self.needs_pairing) {
            (Some(_), _) if !self.online => (Color32::GRAY, tr("nedostupná", "unreachable")),
            (Some(_), false) => (POWER_ON, tr("připojeno", "connected")),
            (Some(_), true) => (
                Color32::from_rgb(230, 160, 40),
                tr("nespárováno", "not paired"),
            ),
            (None, _) => (Color32::GRAY, tr("odpojeno", "disconnected")),
        };
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
            ui.painter().circle_filled(rect.center(), 5.0, color);
            ui.vertical(|ui| {
                let name = self.tv.as_ref().map_or("Philips TV", |tv| tv.name.as_str());
                ui.label(RichText::new(name).strong().size(17.0));
                let host = self.tv.as_ref().map_or("", |tv| tv.host.as_str());
                ui.label(RichText::new(format!("{host} {state}")).small().weak());
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                // Zprava doleva: „Plné“ je vpravo.
                let choices = [
                    (Layout::Full, tr("Plné", "Full")),
                    (Layout::Minimal, tr("Minimální", "Minimal")),
                ];
                for (layout, label) in choices {
                    let selected = self.layout == layout;
                    if ui
                        .selectable_label(selected, RichText::new(label).small())
                        .on_hover_text(tr("Rozložení tlačítek", "Button layout"))
                        .clicked()
                        && !selected
                    {
                        self.set_layout(ui.ctx(), layout);
                    }
                }
            });
        });
    }

    fn controls(&mut self, ui: &mut egui::Ui) {
        match self.layout {
            Layout::Minimal => self.minimal_controls(ui),
            Layout::Full => self.full_controls(ui),
        }
    }

    fn power_row(&mut self, ui: &mut egui::Ui, height: f32) {
        let size = Vec2::new(WIDE.x, height);
        centered_row(ui, 2, WIDE.x, |ui| {
            let on = tr("Zapnout", "Power on");
            if remote_button(ui, size, Glyph::Power, Some(on), on, POWER_ON, false).clicked() {
                self.send(Command::PowerOn);
            }
            let off = tr("Vypnout", "Power off");
            if remote_button(ui, size, Glyph::Power, Some(off), off, POWER_OFF, false).clicked() {
                self.send(Command::PowerOff);
            }
        });
    }

    /// Nejčastější klávesy ve větších tlačítkách.
    fn minimal_controls(&mut self, ui: &mut egui::Ui) {
        self.power_row(ui, WIDE.y);
        ui.add_space(16.0);

        let dpad: [[Option<(&'static str, &'static str)>; 3]; 3] = [
            [None, Some(("⏶", "CursorUp")), None],
            [
                Some(("⏴", "CursorLeft")),
                Some(("OK", "Confirm")),
                Some(("⏵", "CursorRight")),
            ],
            [None, Some(("⏷", "CursorDown")), None],
        ];
        for row in dpad {
            centered_row(ui, 3, CELL.x, |ui| {
                for cell in row {
                    match cell {
                        Some((glyph, key)) => self.key_button(ui, CELL, glyph, None, key),
                        None => {
                            ui.allocate_exact_size(CELL, Sense::hover());
                        }
                    }
                }
            });
        }
        ui.add_space(16.0);

        centered_row(ui, 2, WIDE.x, |ui| {
            self.key_button(ui, WIDE, "↩", Some(tr("Zpět", "Back")), "Back");
            self.key_button(ui, WIDE, "🏠", Some(tr("Domů", "Home")), "Home");
        });
        centered_row(ui, 2, WIDE.x, |ui| {
            self.key_button(ui, WIDE, "▶", Some(tr("Pokračovat", "Play")), "Play");
            self.key_button(ui, WIDE, "⏸", Some(tr("Pauza", "Pause")), "Pause");
        });
        let small = Vec2::new(CELL.x, WIDE.y);
        centered_row(ui, 3, CELL.x, |ui| {
            let volume_down = tr("Hlasitost −", "Volume −");
            self.key_button(ui, small, "🔉", Some(volume_down), "VolumeDown");
            self.key_button(ui, small, "🔇", Some(tr("Ztlumit", "Mute")), "Mute");
            let volume_up = tr("Hlasitost +", "Volume +");
            self.key_button(ui, small, "🔊", Some(volume_up), "VolumeUp");
        });
    }

    /// Skoro celý dálkový ovladač, seřazený podobně jako skutečný ovladač Philips.
    fn full_controls(&mut self, ui: &mut egui::Ui) {
        let compact = Vec2::new(CELL.x, 34.0);
        let labeled = Vec2::new(CELL.x, 46.0);

        self.power_row(ui, labeled.y);
        ui.add_space(SECTION);

        let transport = [
            [("⏹", "Stop"), ("⏸", "Pause"), ("⏺", "Record")],
            [("⏪", "Rewind"), ("▶", "Play"), ("⏩", "FastForward")],
        ];
        for row in transport {
            centered_row(ui, 3, CELL.x, |ui| {
                for (glyph, key) in row {
                    self.key_button(ui, compact, glyph, None, key);
                }
            });
        }
        ui.add_space(SECTION);

        centered_row(ui, 3, CELL.x, |ui| {
            self.key_button(ui, labeled, "📥", Some(tr("Zdroj", "Source")), "Source");
            self.key_button(ui, labeled, "🔍", Some(tr("Hledat", "Search")), "Find");
            self.key_button(ui, labeled, "📺", Some("TV"), "WatchTV");
        });
        let color = Vec2::new((ROW_WIDTH - GAP * 3.0) / 4.0, 22.0);
        centered_row(ui, 4, color.x, |ui| {
            for (key, fill) in COLOR_KEYS {
                self.key_button(ui, color, Glyph::Color(fill), None, key);
            }
        });
        ui.add_space(SECTION);

        // Kříž, v rozích Info, Možnosti, Zpět a Domů.
        let dpad = Vec2::new(CELL.x, 50.0);
        let rows = [
            [
                ("ℹ", Some("Info"), "Info"),
                ("⏶", None, "CursorUp"),
                ("☰", Some(tr("Možnosti", "Options")), "Options"),
            ],
            [
                ("⏴", None, "CursorLeft"),
                ("OK", None, "Confirm"),
                ("⏵", None, "CursorRight"),
            ],
            [
                ("↩", Some(tr("Zpět", "Back")), "Back"),
                ("⏷", None, "CursorDown"),
                ("🏠", Some(tr("Domů", "Home")), "Home"),
            ],
        ];
        for row in rows {
            centered_row(ui, 3, CELL.x, |ui| {
                for (glyph, caption, key) in row {
                    self.key_button(ui, dpad, glyph, caption, key);
                }
            });
        }
        ui.add_space(SECTION);

        // Hlasitost vlevo, kanály vpravo.
        let volume = Some(tr("Hlasitost", "Volume"));
        let channel = Some(tr("Kanál", "Channel"));
        centered_row(ui, 3, CELL.x, |ui| {
            self.key_button(ui, labeled, "+", volume, "VolumeUp");
            self.key_button(ui, labeled, "💡", Some("Ambilight"), "AmbilightOnOff");
            self.key_button(ui, labeled, "+", channel, "ChannelStepUp");
        });
        centered_row(ui, 3, CELL.x, |ui| {
            self.key_button(ui, labeled, "−", volume, "VolumeDown");
            self.key_button(ui, labeled, "🔇", Some(tr("Ztlumit", "Mute")), "Mute");
            self.key_button(ui, labeled, "−", channel, "ChannelStepDown");
        });
        ui.add_space(SECTION);

        let digits = [
            [("1", "Digit1"), ("2", "Digit2"), ("3", "Digit3")],
            [("4", "Digit4"), ("5", "Digit5"), ("6", "Digit6")],
            [("7", "Digit7"), ("8", "Digit8"), ("9", "Digit9")],
        ];
        for row in digits {
            centered_row(ui, 3, CELL.x, |ui| {
                for (glyph, key) in row {
                    self.key_button(ui, compact, glyph, None, key);
                }
            });
        }
        centered_row(ui, 3, CELL.x, |ui| {
            let subtitles = Glyph::Word(tr("Titulky", "Subtitles"));
            self.key_button(ui, compact, subtitles, None, "Subtitle");
            self.key_button(ui, compact, "0", None, "Digit0");
            self.key_button(ui, compact, Glyph::Word("Teletext"), None, "Teletext");
        });
    }

    fn key_button(
        &mut self,
        ui: &mut egui::Ui,
        size: Vec2,
        glyph: impl Into<Glyph>,
        caption: Option<&str>,
        key: &'static str,
    ) {
        let glyph = glyph.into();
        let accent = match glyph {
            Glyph::Color(color) => color,
            _ => ui.visuals().selection.bg_fill,
        };
        let label = Action::Key(key).describe();
        if remote_button(ui, size, glyph, caption, label, accent, self.is_lit(key)).clicked() {
            self.press(key);
        }
    }

    fn config_warning(&self, ui: &mut egui::Ui) {
        let Some(error) = &self.config_error else {
            return;
        };
        let color = ui.visuals().error_fg_color;
        egui::Frame::group(ui.style())
            .stroke(Stroke::new(1.0, color))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let title = tr("⚠ Nastavení nejde přečíst", "⚠ Cannot read the settings");
                ui.colored_label(color, RichText::new(title).strong());
                ui.label(RichText::new(error).small());
                ui.label(tr(
                    "Ovladač soubor nepřepíše a změny (televize, párování, rozložení) zatím \
                     neukládá. Opravte ho a aplikaci restartujte.",
                    "The remote will not overwrite the file and does not save changes \
                     (TV, pairing, layout) for now. Fix it and restart the app.",
                ));
            });
    }

    fn status_line(&self, ui: &mut egui::Ui) {
        if let Some(text) = &self.busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(text);
            });
            return;
        }
        match &self.status {
            Some(Status::Info(text)) => {
                ui.label(text);
            }
            Some(Status::Error(text)) => {
                ui.colored_label(ui.visuals().error_fg_color, text);
            }
            Some(Status::Offline(text)) => {
                ui.colored_label(ui.visuals().warn_fg_color, text);
            }
            None => {}
        }
    }

    fn pairing_panel(&mut self, ui: &mut egui::Ui) {
        if !self.needs_pairing {
            return;
        }
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !self.pairing_active {
                ui.label(tr(
                    "Televize vyžaduje jednorázové spárování.",
                    "The TV requires one-time pairing.",
                ));
                if ui.button(tr("Spárovat", "Pair")).clicked() {
                    self.send(Command::StartPairing);
                }
                return;
            }
            ui.label(tr(
                "Zadejte PIN zobrazený na televizi:",
                "Enter the PIN shown on the TV:",
            ));
            ui.horizontal(|ui| {
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.pin)
                        .desired_width(90.0)
                        .hint_text("PIN"),
                );
                if std::mem::take(&mut self.focus_pin) {
                    field.request_focus();
                }
                let submitted = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                if (ui.button(tr("Potvrdit", "Confirm")).clicked() || submitted)
                    && !self.pin.trim().is_empty()
                {
                    self.send(Command::FinishPairing(self.pin.trim().to_owned()));
                }
            });
        });
    }

    fn connection_panel(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(tr("Připojení", "Connection"))
            .default_open(false)
            .open(self.connection_open.take())
            .show(ui, |ui| {
                if ui
                    .button(tr("🔍 Hledat TV v síti", "🔍 Search the network"))
                    .clicked()
                {
                    self.send(Command::Discover);
                }
                let current = self.tv.as_ref().map(|tv| tv.host.as_str());
                let mut selected = None;
                for tv in &self.found {
                    let label = format!("{} – {}", tv.info.name, tv.host);
                    if ui
                        .selectable_label(current == Some(tv.host.as_str()), label)
                        .clicked()
                    {
                        selected = Some(tv.host.clone());
                    }
                }
                ui.horizontal(|ui| {
                    ui.label(tr("IP adresa:", "IP address:"));
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut self.manual_host)
                            .desired_width(120.0)
                            .hint_text("192.168.1.50"),
                    );
                    let submitted = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    if (ui.button(tr("Připojit", "Connect")).clicked() || submitted)
                        && !self.manual_host.trim().is_empty()
                    {
                        selected = Some(self.manual_host.trim().to_owned());
                    }
                });
                if let Some(host) = selected {
                    self.send(Command::Connect(host));
                }
            });
    }

    fn help_panel(&self, ui: &mut egui::Ui) {
        let title = tr("Nápověda – klávesy a zkratky", "Help – keys and hotkeys");
        egui::CollapsingHeader::new(title).show(ui, |ui| {
            let error_color = ui.visuals().error_fg_color;
            ui.label(RichText::new(tr("V okně ovladače", "In the remote window")).strong());
            egui::Grid::new("window_keys")
                .num_columns(2)
                .spacing([14.0, 4.0])
                .show(ui, |ui| {
                    for (label, action) in window_keys::grouped(&self.window_keys) {
                        ui.label(RichText::new(label).monospace());
                        ui.label(action.describe());
                        ui.end_row();
                    }
                });
            for error in &self.window_key_errors {
                ui.colored_label(error_color, format!("⚠ {error}"));
            }

            ui.add_space(4.0);
            let heading = tr("Globální zkratky (odkudkoli)", "Global hotkeys (anywhere)");
            ui.label(RichText::new(heading).strong());
            let hotkeys = match &self.hotkeys {
                Ok(hotkeys) => hotkeys,
                Err(err) => {
                    let text = trf!("Nedostupné: {err}", "Unavailable: {err}");
                    ui.colored_label(ui.visuals().error_fg_color, text);
                    return;
                }
            };
            egui::Grid::new("global_hotkeys")
                .num_columns(2)
                .spacing([14.0, 4.0])
                .show(ui, |ui| {
                    for hotkey in hotkeys {
                        let description = hotkey
                            .action
                            .map_or(hotkey.action_name.as_str(), |action| action.describe());
                        match &hotkey.error {
                            None => {
                                ui.label(RichText::new(&hotkey.combo).strong());
                                ui.label(description);
                            }
                            Some(error) => {
                                ui.label(RichText::new(&hotkey.combo).strong().strikethrough())
                                    .on_hover_text(error);
                                ui.colored_label(error_color, format!("⚠ {description}"))
                                    .on_hover_text(error);
                            }
                        }
                        ui.end_row();
                    }
                });
            if hotkeys.iter().any(|hotkey| hotkey.error.is_some()) {
                ui.colored_label(
                    error_color,
                    tr(
                        "Přeškrtnuté zkratky nefungují – důvod ukáže najetí myší.",
                        "Struck-out hotkeys don't work – hover to see why.",
                    ),
                );
            }
            ui.label(
                RichText::new(tr(
                    "Klávesy v okně a zkratky upravíte v ~/.config/philips-controller/\
                     config.json (sekce window_keys a hotkeys), projeví se po restartu \
                     aplikace.",
                    "Edit window keys and hotkeys in ~/.config/philips-controller/\
                     config.json (window_keys and hotkeys sections); changes apply after \
                     restarting the app.",
                ))
                .small()
                .weak(),
            );
        });
    }
}

impl eframe::App for RemoteApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_events();
        self.process_actions(ctx);
        // Křížek okno jen schová do lišty, ukončuje se z menu ikony.
        if ctx.input(|i| i.viewport().close_requested()) && self.tray.is_some() && !self.quitting {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.set_visible(ctx, false);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.handle_keyboard(ui);
        egui::CentralPanel::default_margins().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::splat(GAP);
                self.header(ui);
                self.config_warning(ui);
                ui.add_space(6.0);
                self.controls(ui);
                ui.separator();
                self.status_line(ui);
                self.pairing_panel(ui);
                self.connection_panel(ui);
                self.help_panel(ui);
            });
        });
        if let Some((_, at)) = self.flash
            && let Some(remaining) = FLASH.checked_sub(at.elapsed())
        {
            ui.ctx().request_repaint_after(remaining);
        }
    }
}

/// Řada `count` buněk šířky `cell_width` zarovnaná na střed.
fn centered_row(
    ui: &mut egui::Ui,
    count: usize,
    cell_width: f32,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    let width = cell_width * count as f32 + GAP * (count - 1) as f32;
    ui.horizontal(|ui| {
        ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
        add_contents(ui);
    });
}

fn remote_button(
    ui: &mut egui::Ui,
    size: Vec2,
    glyph: Glyph,
    caption: Option<&str>,
    label: &str,
    accent: Color32,
    lit: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(Role::Button, true, label));
    if !ui.is_rect_visible(rect) {
        return response;
    }

    let visuals = ui.style().interact(&response);
    let pressed = lit || response.is_pointer_button_down_on();
    let fill = if pressed {
        accent.gamma_multiply(0.6)
    } else {
        visuals.weak_bg_fill
    };
    let stroke = match glyph {
        Glyph::Power => Stroke::new(1.5, accent),
        _ => visuals.bg_stroke,
    };
    let painter = ui.painter();
    let rounding = (size.y / 2.0).min(12.0);
    painter.rect(rect, rounding, fill, stroke, StrokeKind::Inside);

    let text_color = visuals.text_color();
    let glyph_center = match caption {
        Some(_) => rect.center() - Vec2::new(0.0, 8.0),
        None => rect.center(),
    };
    match glyph {
        Glyph::Text(text) => {
            let font = FontId::proportional(if caption.is_some() { 20.0 } else { 24.0 });
            painter.text(glyph_center, Align2::CENTER_CENTER, text, font, text_color);
        }
        Glyph::Word(text) => {
            let font = FontId::proportional(13.5);
            painter.text(glyph_center, Align2::CENTER_CENTER, text, font, text_color);
        }
        Glyph::Power => paint_power_icon(painter, glyph_center, 8.0, Stroke::new(2.2, accent)),
        Glyph::Color(color) => {
            let bar = Vec2::new(rect.width() * 0.55, 6.0);
            painter.rect_filled(Rect::from_center_size(glyph_center, bar), 3.0, color);
        }
    }
    if let Some(caption) = caption {
        let caption_pos = rect.center_bottom() - Vec2::new(0.0, 12.0);
        painter.text(
            caption_pos,
            Align2::CENTER_CENTER,
            caption,
            FontId::proportional(12.5),
            text_color,
        );
    }
    response
}

/// Symbol ⏻ – ve vestavěných fontech egui chybí.
fn paint_power_icon(painter: &Painter, center: Pos2, radius: f32, stroke: Stroke) {
    let arc = (0..=32)
        .map(|i| {
            let angle = (40.0 + 280.0 * i as f32 / 32.0).to_radians();
            center + radius * Vec2::new(angle.sin(), -angle.cos())
        })
        .collect();
    painter.line(arc, stroke);
    painter.line_segment(
        [
            center - Vec2::new(0.0, radius * 1.25),
            center - Vec2::new(0.0, radius * 0.2),
        ],
        stroke,
    );
}
