//! Philips Controller – dálkový ovladač televizí Philips přes síť (JointSpace API).
//!
//! `--hidden` spustí aplikaci jen s ikonou v liště (pro autostart).

mod action;
mod app;
mod config;
mod discovery;
#[cfg(target_os = "linux")]
mod hotkeys;
mod i18n;
mod icon;
mod instance;
mod tray;
mod tv;
mod window_keys;
mod wol;
mod worker;

use eframe::egui;

use crate::config::Config;
use crate::i18n::tr;
use crate::instance::Instance;

/// WM_CLASS okna – podle něj zkratky poznají, že je ovladač aktivní.
pub const APP_ID: &str = "philips-controller";

fn main() -> eframe::Result {
    let start_hidden = std::env::args().any(|arg| arg == "--hidden");
    let listener = match instance::claim(!start_hidden) {
        Instance::First(listener) => listener,
        Instance::AlreadyRunning => return Ok(()),
    };
    let config = Config::load();
    i18n::init(config.language);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(tr("Philips ovladač", "Philips Remote"))
            .with_app_id(APP_ID)
            .with_inner_size(app::window_size(config.layout))
            // Pevná velikost – tilovací WM (awesome, i3…) pak okno nechají plavat.
            .with_resizable(false)
            .with_visible(!start_hidden)
            .with_icon(egui::IconData {
                rgba: icon::rgba(),
                width: icon::SIZE,
                height: icon::SIZE,
            }),
        ..Default::default()
    };
    eframe::run_native(
        // Interní název pro eframe nezávisí na jazyce rozhraní.
        "Philips ovladač",
        options,
        Box::new(move |cc| {
            Ok(Box::new(app::RemoteApp::new(
                cc,
                listener,
                start_hidden,
                &config,
            )))
        }),
    )
}
