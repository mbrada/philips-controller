//! Globální klávesové zkratky přes X11 (XGrabKey) – fungují i se skrytým oknem.
//!
//! Zkratky se zapisují jako v xbindkeys: modifikátory a název keysymu oddělené `+`,
//! např. `Super+Alt+Up` nebo `XF86Launch5`. Místo keysymu jde zadat i keycode (`c:191`).

use std::collections::{BTreeMap, HashMap};
use std::ffi::CString;
use std::sync::mpsc::{Sender, channel};
use std::thread;

use anyhow::{Context, Result, anyhow, bail};
use eframe::egui;
use x11_dl::xlib::Xlib;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ConnectionExt, GetKeyboardMappingReply, GrabMode, Keycode, ModMask, Window,
};
use x11rb::rust_connection::RustConnection;

use crate::APP_ID;
use crate::action::Action;
use crate::i18n::{tr, trf};

// Masky modifikátorů X11 (ShiftMask, LockMask, ControlMask, Mod1–Mod4).
const SHIFT: u16 = 1 << 0;
const CAPS_LOCK: u16 = 1 << 1;
const CONTROL: u16 = 1 << 2;
const ALT: u16 = 1 << 3;
const NUM_LOCK: u16 = 1 << 4;
const SUPER: u16 = 1 << 6;
const RELEVANT_MODS: u16 = SHIFT | CONTROL | ALT | SUPER;
/// X11 bere zapnutý NumLock a CapsLock jako modifikátory – zkratka musí fungovat i s nimi.
const LOCK_MODS: [u16; 4] = [0, NUM_LOCK, CAPS_LOCK, NUM_LOCK | CAPS_LOCK];

/// Výsledek registrace jedné zkratky pro nápovědu v okně.
pub struct Hotkey {
    pub combo: String,
    pub action: Option<Action>,
    pub action_name: String,
    pub error: Option<String>,
}

pub fn spawn(
    bindings: &BTreeMap<String, String>,
    actions: Sender<Action>,
    ctx: egui::Context,
) -> Result<Vec<Hotkey>> {
    let bindings = bindings.clone();
    let (ready_tx, ready_rx) = channel();
    thread::spawn(move || {
        let (conn, root, grabbed, report) = match setup(bindings) {
            Ok(setup) => setup,
            Err(err) => {
                let _ = ready_tx.send(Err(err));
                return;
            }
        };
        let _ = ready_tx.send(Ok(report));
        let active_window = conn
            .intern_atom(false, b"_NET_ACTIVE_WINDOW")
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .map(|reply| reply.atom);
        while let Ok(event) = conn.wait_for_event() {
            let Event::KeyPress(press) = event else {
                continue;
            };
            let mods = u16::from(press.state) & RELEVANT_MODS;
            let Some(&action) = grabbed.get(&(press.detail, mods)) else {
                continue;
            };
            // Fokus se zjišťuje z WM, ne z egui: při grabu klávesy okno dočasně dostane FocusOut.
            let action = match action {
                Action::ShowWindow
                    if active_window.is_some_and(|atom| is_active(&conn, root, atom)) =>
                {
                    Action::HideWindow
                }
                action => action,
            };
            let _ = actions.send(action);
            ctx.request_repaint();
        }
    });
    ready_rx.recv()?
}

type Grabbed = HashMap<(Keycode, u16), Action>;

fn setup(
    bindings: BTreeMap<String, String>,
) -> Result<(RustConnection, Window, Grabbed, Vec<Hotkey>)> {
    let (conn, screen) = x11rb::connect(None).context(tr(
        "nelze se připojit k X serveru",
        "cannot connect to the X server",
    ))?;
    let root = conn.setup().roots[screen].root;
    let keymap = Keymap::load(&conn)?;
    let xlib = Xlib::open().context(tr("nelze načíst libX11", "cannot load libX11"))?;

    let mut grabbed = HashMap::new();
    let mut report = Vec::new();
    for (combo, action_name) in bindings {
        let action = Action::parse(&action_name);
        let result = action
            .ok_or_else(|| {
                anyhow!(trf!(
                    "neznámá akce „{action_name}“",
                    "unknown action “{action_name}”"
                ))
            })
            .and_then(|action| {
                let (mods, keycodes) = parse_combo(&combo, &keymap, &xlib)?;
                // Stejný keysym může mít víc kláves – stačí, když se podaří zabrat aspoň jednu.
                let mut any = false;
                for keycode in keycodes {
                    if !grabbed.contains_key(&(keycode, mods))
                        && grab(&conn, root, mods, keycode).is_ok()
                    {
                        grabbed.insert((keycode, mods), action);
                        any = true;
                    }
                }
                if !any {
                    bail!(tr(
                        "zkratku už používá jiný program nebo je v konfiguraci dvakrát",
                        "the hotkey is taken by another program or listed twice in the config",
                    ));
                }
                Ok(())
            });
        report.push(Hotkey {
            combo,
            action,
            action_name,
            error: result.err().map(|err| format!("{err:#}")),
        });
    }
    Ok((conn, root, grabbed, report))
}

/// Je aktivním oknem (podle `_NET_ACTIVE_WINDOW`) právě ovladač?
fn is_active(conn: &RustConnection, root: Window, active_window: Atom) -> bool {
    let active = conn
        .get_property(false, root, active_window, AtomEnum::WINDOW, 0, 1)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .and_then(|reply| reply.value32()?.next());
    let Some(window) = active.filter(|&window| window != 0) else {
        return false;
    };
    conn.get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 64)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .is_some_and(|reply| {
            reply
                .value
                .split(|&byte| byte == 0)
                .any(|part| part == APP_ID.as_bytes())
        })
}

fn parse_combo(combo: &str, keymap: &Keymap, xlib: &Xlib) -> Result<(u16, Vec<Keycode>)> {
    let (mods, key) = split_combo(combo)?;
    if let Some(code) = key.strip_prefix("c:") {
        let keycode = code
            .parse()
            .map_err(|_| anyhow!(trf!("neplatný keycode „{key}“", "invalid keycode “{key}”")))?;
        return Ok((mods, vec![keycode]));
    }
    let name = CString::new(key)?;
    // SAFETY: XStringToKeysym jen čte řetězec zakončený nulou a nepotřebuje spojení s displejem.
    let keysym = unsafe { (xlib.XStringToKeysym)(name.as_ptr()) };
    if keysym == 0 {
        bail!(trf!("neznámá klávesa „{key}“", "unknown key “{key}”"));
    }
    let keycodes = keymap.keycodes(keysym as u32);
    if keycodes.is_empty() {
        bail!(trf!(
            "klávesa „{key}“ není na klávesnici namapovaná",
            "key “{key}” is not mapped on the keyboard"
        ));
    }
    Ok((mods, keycodes))
}

fn split_combo(combo: &str) -> Result<(u16, &str)> {
    let mut parts: Vec<&str> = combo.split('+').map(str::trim).collect();
    let key = parts
        .pop()
        .filter(|key| !key.is_empty())
        .ok_or_else(|| anyhow!(tr("chybí klávesa", "missing key")))?;
    let mut mods = 0;
    for modifier in parts {
        mods |= match modifier.to_ascii_lowercase().as_str() {
            "shift" => SHIFT,
            "ctrl" | "control" => CONTROL,
            "alt" | "mod1" => ALT,
            "super" | "win" | "mod4" => SUPER,
            other => bail!(trf!(
                "neznámý modifikátor „{other}“",
                "unknown modifier “{other}”"
            )),
        };
    }
    Ok((mods, key))
}

/// Selže, když klávesu už zabral jiný program (BadAccess).
fn grab(conn: &RustConnection, root: Window, mods: u16, keycode: Keycode) -> Result<()> {
    for lock in LOCK_MODS {
        let result = conn
            .grab_key(
                false,
                root,
                ModMask::from(mods | lock),
                keycode,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
            )?
            .check();
        if let Err(err) = result {
            for lock in LOCK_MODS {
                let _ = conn.ungrab_key(keycode, root, ModMask::from(mods | lock));
            }
            return Err(err.into());
        }
    }
    Ok(())
}

/// Tabulka keycode → keysymy aktuálního rozložení klávesnice.
struct Keymap {
    min_keycode: Keycode,
    mapping: GetKeyboardMappingReply,
}

impl Keymap {
    fn load(conn: &RustConnection) -> Result<Self> {
        let setup = conn.setup();
        let (min_keycode, max_keycode) = (setup.min_keycode, setup.max_keycode);
        let mapping = conn
            .get_keyboard_mapping(min_keycode, max_keycode - min_keycode + 1)?
            .reply()?;
        Ok(Self {
            min_keycode,
            mapping,
        })
    }

    fn keycodes(&self, keysym: u32) -> Vec<Keycode> {
        let per_keycode = usize::from(self.mapping.keysyms_per_keycode).max(1);
        self.mapping
            .keysyms
            .chunks(per_keycode)
            .enumerate()
            .filter(|(_, keysyms)| keysyms.contains(&keysym))
            .map(|(index, _)| self.min_keycode + index as u8)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_modifiers_and_key() {
        assert_eq!(split_combo("Super+Alt+Up").unwrap(), (SUPER | ALT, "Up"));
        assert_eq!(
            split_combo("ctrl + shift + space").unwrap(),
            (CONTROL | SHIFT, "space")
        );
        assert_eq!(split_combo("XF86Launch5").unwrap(), (0, "XF86Launch5"));
        assert!(split_combo("Super+").is_err());
        assert!(split_combo("Hyper+a").is_err());
    }
}
