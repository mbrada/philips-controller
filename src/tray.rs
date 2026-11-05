//! Ikona v systémové liště s rychlým menu.

use std::sync::mpsc::{Sender, channel};

use anyhow::Result;
use eframe::egui;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::action::Action;
use crate::i18n::tr;
use crate::icon;

/// Položky menu: id, popisek česky a anglicky, akce.
const ITEMS: [(&str, &str, &str, Action); 5] = [
    (
        "toggle",
        "Zobrazit / skrýt ovladač",
        "Show / hide remote",
        Action::ToggleWindow,
    ),
    ("power-on", "Zapnout TV", "Turn TV on", Action::PowerOn),
    ("power-off", "Vypnout TV", "Turn TV off", Action::PowerOff),
    ("mute", "Ztlumit", "Mute", Action::Key("Mute")),
    ("quit", "Ukončit", "Quit", Action::Quit),
];

pub struct Tray {
    /// Na Linuxu ikonu drží GTK vlákno, jinde musí žít v hlavním vlákně.
    _icon: Option<TrayIcon>,
}

impl Tray {
    pub fn spawn(actions: Sender<Action>, ctx: egui::Context) -> Result<Self> {
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if let Some((_, _, _, action)) = ITEMS.iter().find(|(id, ..)| event.id == *id) {
                let _ = actions.send(*action);
                // Probudí `App::logic` i při skrytém okně.
                ctx.request_repaint();
            }
        }));

        #[cfg(target_os = "linux")]
        {
            let (ready_tx, ready_rx) = channel();
            std::thread::spawn(move || run_gtk(ready_tx));
            ready_rx.recv()??;
            Ok(Self { _icon: None })
        }
        #[cfg(not(target_os = "linux"))]
        {
            Ok(Self {
                _icon: Some(build()?),
            })
        }
    }
}

#[cfg(target_os = "linux")]
fn run_gtk(ready: Sender<Result<()>>) {
    if let Err(err) = gtk::init() {
        let _ = ready.send(Err(err.into()));
        return;
    }
    match build() {
        Ok(_icon) => {
            let _ = ready.send(Ok(()));
            gtk::main();
        }
        Err(err) => {
            let _ = ready.send(Err(err));
        }
    }
}

fn build() -> Result<TrayIcon> {
    let menu = Menu::new();
    for (index, (id, cs, en, _)) in ITEMS.iter().enumerate() {
        // Oddělovače kolem ovládání TV.
        if index == 1 || index == ITEMS.len() - 1 {
            menu.append(&PredefinedMenuItem::separator())?;
        }
        menu.append(&MenuItem::with_id(*id, tr(cs, en), true, None))?;
    }
    Ok(TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(tr("Philips ovladač", "Philips Remote"))
        .with_icon(Icon::from_rgba(icon::rgba(), icon::SIZE, icon::SIZE)?)
        .build()?)
}
