//! Hlídá, aby běžela jen jedna instance. Další spuštění jen ukáže okno té první.

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::thread;

use eframe::egui;

use crate::action::Action;

pub enum Instance {
    /// Jsme první; posluchač chybí, jen když se socket nepodařilo vytvořit.
    First(Option<UnixListener>),
    AlreadyRunning,
}

/// `show` určuje, jestli má běžící instance zobrazit okno (při autostartu ne).
pub fn claim(show: bool) -> Instance {
    let path = socket_path();
    if let Ok(mut stream) = UnixStream::connect(&path) {
        if show {
            let _ = stream.write_all(b"show\n");
        }
        return Instance::AlreadyRunning;
    }
    // Socket po spadlé instanci.
    let _ = std::fs::remove_file(&path);
    Instance::First(UnixListener::bind(&path).ok())
}

pub fn listen(listener: UnixListener, actions: Sender<Action>, ctx: egui::Context) {
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut message = String::new();
            if stream.read_to_string(&mut message).is_ok() && message.trim() == "show" {
                let _ = actions.send(Action::ShowWindow);
                ctx.request_repaint();
            }
        }
    });
}

fn socket_path() -> PathBuf {
    dirs::runtime_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("philips-controller.sock")
}
