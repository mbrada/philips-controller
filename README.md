# Philips Remote

[![CI](https://github.com/mbrada/philips-controller/actions/workflows/ci.yml/badge.svg)](https://github.com/mbrada/philips-controller/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

[Česky](README.cs.md)

A small desktop remote control for Philips TVs over the local network (JointSpace API),
written in Rust with [egui](https://github.com/emilk/egui). It sits in the system tray
and offers global X11 hotkeys, so you can control the TV without leaving what you are
doing.

## Features

- Finds the TV on the local network (scans the /24 subnet on ports 1925/1926), remembers
  it and connects straight away next time.
- One-time pairing with the PIN shown on the TV (API 6, HTTPS + digest auth).
- Recovers from lost connections on its own: when the TV stops responding (or is not up
  yet at startup), the header shows "unreachable" and the remote reconnects – in the
  background while the window is visible (checking every 20 s that the TV still
  responds), or on the next key press while it is hidden.
- Two button layouts, switched in the window header: **Minimal** with the everyday keys
  (arrows, OK, Back, Home, play / pause, volume, mute, power on and off) and **Full**,
  arranged like a Philips remote, which adds playback controls, Source, Search, the
  colour keys, Info, Options, channels, Ambilight, digits, subtitles and teletext.
- Power on uses Wake-on-LAN + `powerstate`, so it wakes the TV even from deep standby.
- Tray icon with a quick menu (show / hide, power on, power off, mute, quit).
  Closing the window only hides it to the tray.
- Keyboard control in the window: arrows, Enter, Backspace / Esc, Space, + / −, M and
  so on – any key can be [reassigned](#keyboard-shortcuts).
- Global hotkeys (XGrabKey) work even with the window hidden. The defaults are
  Super+Alt+arrows and so on; Super+Alt+T shows the remote, or hides it when it is
  already active. [Any key combination](#keyboard-shortcuts) can be used.
- Single instance: launching it again just shows the window of the running one.
- English and Czech user interface.

All keys and hotkeys are listed in the app under **Help – keys and hotkeys**.

## Requirements

- **Linux with X11.** Global hotkeys rely on XGrabKey, so they are not reliable under
  Wayland (the window and the tray icon work). Other operating systems are not supported.
- **A Philips TV with the JointSpace API** on the same network. Newer models (Android TV /
  Saphi, API 6, roughly 2016 and later) talk HTTPS on port 1926 and need pairing; older
  ones use plain HTTP on port 1925 without pairing. Tested with a 55PUS7502/12 (Saphi,
  API 6).
- Powering on from deep standby needs network wake-up (Wake-on-LAN) enabled in the TV
  settings, and the computer has to be on the same subnet as the TV.

## Building and installing

You need Rust 1.95 or newer (preferably via [rustup](https://rustup.rs)) and the GTK 3
development libraries. At runtime the tray icon also needs libayatana-appindicator
(or libappindicator). On Debian / Ubuntu:

```sh
sudo apt install build-essential pkg-config libgtk-3-dev libayatana-appindicator3-1
```

Install straight from GitHub, or from a cloned repository:

```sh
cargo install --locked --git https://github.com/mbrada/philips-controller
cargo install --locked --path .
```

The binary ends up in `~/.cargo/bin/philips-controller`.

## Usage

```sh
philips-controller            # with the window
philips-controller --hidden   # tray icon only (for autostart)
```

On the first start the remote scans the network by itself. Pick your TV under
**Connection** (or type its IP address there). Newer TVs then need pairing: press
**Pair** and type in the PIN shown on the TV. The TV address, its MAC address for
Wake-on-LAN (taken from the ARP table) and the pairing credentials are saved, so next
time the remote connects right away.

To start it after login, a `.desktop` file in `~/.config/autostart/` is enough:

```ini
[Desktop Entry]
Type=Application
Name=Philips Remote
Exec=philips-controller --hidden
```

## Configuration

`~/.config/philips-controller/config.json` holds the TV address, the MAC address for
Wake-on-LAN, the pairing credentials, the button `layout`, the keys in the window
(`window_keys`), the global `hotkeys` and the optional `language`. The file contains the
key that controls the TV, so the app saves it with `0600` permissions.

```json
"language": "en",
"layout": "full",
```

**Language:** `"en"` or `"cs"`. Without it the language follows the system locale
(`LANGUAGE`, `LC_ALL`, `LC_MESSAGES`, `LANG`): Czech for `cs…`, English otherwise.

**Layout:** `"minimal"` (default) or `"full"`; the switch in the window header saves it.

Window keys and global hotkeys are described in the next section.

## Keyboard shortcuts

The remote has two kinds of keys, both set in `~/.config/philips-controller/config.json`:

- **Global hotkeys** (the `hotkeys` section) work anywhere, even with the window hidden –
  for example Super+Alt+arrows or multimedia keys. They need X11.
- **Window keys** (the `window_keys` section) work only while the remote window is
  focused – arrows, Enter, Space…

### Step by step

1. **Open** `~/.config/philips-controller/config.json`. The app writes both sections with
   the default keys on the first connection to the TV; if they are not there yet, add
   them.
2. **Edit the lines** of the form `"key": "action"` – change, add or delete whatever you
   need. A section always replaces the defaults as a whole: anything missing from it
   will not work. Delete a section completely to get the default keys back.
3. **Restart the remote** – tray icon → **Quit**, then start it again. Keys are only read
   at startup. The remote may keep running while you edit: when it saves the file, it
   only changes its own data (the TV, pairing, layout) and leaves the keys, hotkeys and
   language as you wrote them.
4. **Check the result** in the window under **Help – keys and hotkeys**. It shows what
   is in effect, and invalid entries with the reason.

Example – the number keys switch channels, `P` turns the TV on and the multimedia keys
control the volume:

```json
"window_keys": {
  "Up": "CursorUp",
  "Down": "CursorDown",
  "Left": "CursorLeft",
  "Right": "CursorRight",
  "Enter": "Confirm",
  "Backspace": "Back",
  "Space": "PlayPause",
  "1": "Digit1",
  "2": "Digit2",
  "3": "Digit3",
  "P": "PowerOn"
},
"hotkeys": {
  "Super+Alt+t": "ShowWindow",
  "XF86AudioRaiseVolume": "VolumeUp",
  "XF86AudioLowerVolume": "VolumeDown",
  "XF86AudioMute": "Mute",
  "c:191": "PlayPause"
}
```

Mind the JSON syntax: names and actions in quotes, a comma between lines and none after
the last line of a section. If the file contains an error, the remote will not overwrite
it: it runs with the default settings, saves nothing and its window (even when started
with `--hidden`) shows the line and column of the error. Fix the file and restart the
remote. You can also check it beforehand, for example with
`python3 -m json.tool ~/.config/philips-controller/config.json`.

### Global hotkeys (`hotkeys`)

The syntax is the same as in xbindkeys: modifiers and a key joined with `+`.

- **Modifiers:** `Shift`, `Ctrl` (or `Control`), `Alt` (`Mod1`), `Super` (`Win`,
  `Mod4`); case does not matter.
- **Key:** a keysym name – `t`, `space`, `Return`, `BackSpace`, `Page_Up`, `F5`,
  `XF86AudioRaiseVolume`… (case-sensitive) – or a keycode written as `c:123`.

To find a key name, run `xev -event keyboard` and press the key. Among other things it
prints a line like

```
state 0x0, keycode 123 (keysym 0x1008ff13, XF86AudioRaiseVolume), same_screen YES,
```

so you write `XF86AudioRaiseVolume`, or `c:123`, in the config. A keycode is handy when
several keys send the same keysym (extra keyboard or mouse buttons, for example) and you
want just one of them.

A hotkey cannot be registered when another program or the desktop environment already
holds it (GNOME and KDE usually take the multimedia keys). The in-app help then shows it
struck out and tells you why on hover. Under Wayland global hotkeys are not reliable.

Default global hotkeys:

| Hotkey | Action |
| --- | --- |
| `Super+Alt+t` | `ShowWindow` – shows the remote, hides it when active |
| `Super+Alt+Up` / `Down` / `Left` / `Right` | arrows (`CursorUp`…) |
| `Super+Alt+Return` | `Confirm` (OK) |
| `Super+Alt+BackSpace` | `Back` |
| `Super+Alt+Home` | `Home` |
| `Super+Alt+space` | `PlayPause` |
| `Super+Alt+Page_Up` / `Page_Down` | `VolumeUp` / `VolumeDown` |
| `Super+Alt+m` | `Mute` |
| `Super+Alt+Insert` / `Delete` | `PowerOn` / `PowerOff` |

### Window keys (`window_keys`)

A key is written by its [egui](https://github.com/emilk/egui) name, without modifiers:
`Up`, `Down`, `Left`, `Right`, `Enter`, `Escape`, `Backspace`, `Tab`, `Space`, `Home`,
`End`, `PageUp`, `PageDown`, `Insert`, `Delete`, letters `A`–`Z`, digits `0`–`9`,
`F1`–`F12`, or a character such as `+`, `-`, `=`, `,`, `.`. Named keys are
case-sensitive (`Up`, not `up`).

- What counts is the character the key types in the current keyboard layout – on
  a Czech layout, for example, the 1 key types `+`.
- Keys pressed with Ctrl or Alt are left to the window, and none of the keys apply while
  you type into a text field (IP address, PIN).
- A held key repeats (useful for the volume); `PowerOn`, `PowerOff` and hiding the
  window do not repeat.
- Both `ShowWindow` and `ToggleWindow` hide the focused window.

Default window keys:

| Key | Action |
| --- | --- |
| `Up`, `Down`, `Left`, `Right` | arrows (`CursorUp`…) |
| `Enter` | `Confirm` (OK) |
| `Backspace`, `Escape` | `Back` |
| `H`, `Home` | `Home` |
| `Space` | `PlayPause` |
| `Plus`, `Equals` | `VolumeUp` |
| `Minus` | `VolumeDown` |
| `M` | `Mute` |

### Actions

The same for both sections; case-sensitive.

| Group | Actions |
| --- | --- |
| Navigation | `CursorUp`, `CursorDown`, `CursorLeft`, `CursorRight`, `Confirm`, `Back`, `Home`, `Options`, `Info`, `Find`, `Source`, `WatchTV` |
| Playback | `PlayPause`, `Play`, `Pause`, `Stop`, `Rewind`, `FastForward`, `Previous`, `Next`, `Record` |
| Volume and channels | `VolumeUp`, `VolumeDown`, `Mute`, `ChannelStepUp`, `ChannelStepDown` |
| Colour keys | `RedColour`, `GreenColour`, `YellowColour`, `BlueColour` |
| Digits | `Digit0` … `Digit9` |
| Other TV keys | `Standby` (toggles standby), `AmbilightOnOff`, `Subtitle`, `Teletext` |
| Remote | `PowerOn` (Wake-on-LAN + power on), `PowerOff`, `ShowWindow` (shows the remote, hides it when active), `ToggleWindow` (show / hide) |

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

CI on GitHub runs the same checks (`.github/workflows/ci.yml`). User-facing texts are
written in both languages right where they are used, via `tr("česky", "English")` and
`trf!(…)` from [`src/i18n.rs`](src/i18n.rs).

## Acknowledgements and disclaimer

The JointSpace API and its pairing procedure are documented thanks to community projects
such as [pylips](https://github.com/eslavnov/pylips) and
[ha-philipsjs](https://github.com/danielperna84/ha-philipsjs). The shared key used to sign
the pairing PIN in [`src/tv.rs`](src/tv.rs) comes from the official Philips app and is
publicly known.

This project is not affiliated with Philips or TP Vision in any way; Philips is
a trademark of Koninklijke Philips N.V.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for
inclusion in this project by you, as defined in the Apache-2.0 license, shall be dual
licensed as above, without any additional terms or conditions.
