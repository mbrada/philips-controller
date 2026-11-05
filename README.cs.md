# Philips ovladač

[![CI](https://github.com/mbrada/philips-controller/actions/workflows/ci.yml/badge.svg)](https://github.com/mbrada/philips-controller/actions/workflows/ci.yml)
[![Licence: MIT OR Apache-2.0](https://img.shields.io/badge/licence-MIT%20OR%20Apache--2.0-blue.svg)](#licence)

[English](README.md)

Dálkový ovladač televizí Philips přes síť (JointSpace API) – malé GUI v Rustu
([egui](https://github.com/emilk/egui)) s ikonou v liště a globálními klávesovými
zkratkami pro X11, takže televizi ovládáte, aniž byste odbíhali od práce.

## Co umí

- Najde televizi v lokální síti (sken /24 na portech 1925/1926), uloží si ji a příště se
  připojí rovnou.
- Jednorázové spárování PINem z obrazovky (API 6, HTTPS + digest auth).
- Výpadky spojení řeší sám: když TV přestane odpovídat (nebo při startu ještě neběží),
  hlavička ukáže „nedostupná“ a ovladač se připojí znovu – se zobrazeným oknem průběžně
  na pozadí (a každých 20 s ověřuje, že TV odpovídá), se skrytým hned při dalším stisku.
- Dvě rozložení tlačítek, přepínají se v hlavičce okna: **Minimální** s nejčastějšími
  klávesami (šipky, OK, Zpět, Domů, pauza / pokračovat, hlasitost, ztlumení, zapnutí
  a vypnutí) a **Plné**, seřazené jako ovladač Philips – navíc přehrávání, zdroj,
  hledání, barevná tlačítka, info, možnosti, kanály, Ambilight, číslice, titulky
  a teletext.
- Zapnutí používá Wake-on-LAN + `powerstate`, takže televizi probudí i z hlubokého
  standby.
- Ikona v systémové liště (zobrazit / skrýt, zapnout, vypnout, ztlumit, ukončit).
  Křížek okno jen schová do lišty.
- Ovládání klávesnicí v okně: šipky, Enter, Backspace / Esc, mezerník, + / −, M atd. –
  každou klávesu jde [přenastavit](#klávesové-zkratky).
- Globální zkratky (XGrabKey) fungují i se skrytým oknem. Výchozí jsou Super+Alt+šipky
  apod., Super+Alt+T zobrazí ovladač, nebo ho schová, když už je aktivní. Nastavit jde
  [libovolné](#klávesové-zkratky).
- Běží jen jedna instance: další spuštění ukáže okno té první.
- Rozhraní česky a anglicky.

Přehled kláves a zkratek je v aplikaci pod **Nápověda – klávesy a zkratky**.

## Požadavky

- **Linux s X11.** Globální zkratky používají XGrabKey, pod Waylandem proto spolehlivě
  nefungují (okno i ikona v liště ano). Jiné systémy nejsou podporované.
- **Televize Philips s JointSpace API** v téže síti. Novější modely (Android TV / Saphi,
  API 6, zhruba od roku 2016) komunikují přes HTTPS na portu 1926 a vyžadují spárování,
  starší přes HTTP na portu 1925 bez párování. Vyzkoušeno na 55PUS7502/12 (Saphi, API 6).
- Zapínání z hlubokého standby potřebuje v televizi povolené zapínání po síti
  (Wake-on-LAN) a počítač musí být ve stejné podsíti jako TV.

## Sestavení a instalace

Potřebujete Rust 1.95 nebo novější (nejlépe přes [rustup](https://rustup.rs)) a vývojové
knihovny GTK 3. Za běhu je navíc potřeba libayatana-appindicator (nebo libappindicator)
kvůli ikoně v liště. Na Debianu / Ubuntu:

```sh
sudo apt install build-essential pkg-config libgtk-3-dev libayatana-appindicator3-1
```

Instalace přímo z GitHubu, nebo z naklonovaného repozitáře:

```sh
cargo install --locked --git https://github.com/mbrada/philips-controller
cargo install --locked --path .
```

Binárka skončí v `~/.cargo/bin/philips-controller`.

## Použití

```sh
philips-controller            # s oknem
philips-controller --hidden   # jen ikona v liště (pro autostart)
```

Při prvním spuštění ovladač sám prohledá síť. Televizi vyberte v sekci **Připojení**
(nebo tam zadejte její IP adresu). Novější televize je pak potřeba spárovat: stiskněte
**Spárovat** a opište PIN z obrazovky. Adresa TV, její MAC adresa pro Wake-on-LAN
(zjistí se z ARP tabulky) a přihlašovací údaje z párování se uloží, příště se ovladač
připojí rovnou.

Pro spuštění po přihlášení stačí `.desktop` soubor v `~/.config/autostart/`:

```ini
[Desktop Entry]
Type=Application
Name=Philips ovladač
Exec=philips-controller --hidden
```

## Konfigurace

`~/.config/philips-controller/config.json` – adresa TV, MAC pro Wake-on-LAN, údaje
z párování, rozložení tlačítek `layout`, klávesy v okně `window_keys`, globální zkratky
`hotkeys` a volitelně `language`. Soubor obsahuje klíč k ovládání televize, proto ho
aplikace ukládá s právy `0600`.

```json
"language": "cs",
"layout": "full",
```

**Jazyk:** `"cs"`, nebo `"en"`. Bez něj se řídí locale systému (`LANGUAGE`, `LC_ALL`,
`LC_MESSAGES`, `LANG`): pro `cs…` česky, jinak anglicky.

**Rozložení:** `"minimal"` (výchozí), nebo `"full"`; ukládá ho přepínač v hlavičce okna.

Klávesy v okně a globální zkratky popisuje následující sekce.

## Klávesové zkratky

Ovladač zná dva druhy kláves a oba se nastavují v `~/.config/philips-controller/config.json`:

- **Globální zkratky** (sekce `hotkeys`) fungují odkudkoli, i se skrytým oknem – třeba
  Super+Alt+šipky nebo multimediální klávesy. Potřebují X11.
- **Klávesy v okně** (sekce `window_keys`) fungují, jen když je okno ovladače aktivní –
  šipky, Enter, mezerník…

### Postup

1. **Otevřete** `~/.config/philips-controller/config.json`. Obě sekce s výchozími
   klávesami do něj aplikace zapíše při prvním připojení k televizi; když v souboru ještě
   nejsou, přidejte je.
2. **Upravte řádky** ve tvaru `"klávesa": "akce"` – změňte, přidejte nebo smažte, co
   potřebujete. Sekce vždy nahrazuje výchozí nastavení celá: co v ní chybí, fungovat
   nebude. Když sekci smažete úplně, vrátí se výchozí klávesy.
3. **Restartujte ovladač** – ikona v liště → **Ukončit** a spusťte ho znovu. Klávesy se
   načítají jen při startu. Ovladač může při úpravě klidně běžet: když soubor ukládá,
   mění jen svoje údaje (televizi, párování, rozložení) a klávesy, zkratky i jazyk nechá
   tak, jak jste je napsali.
4. **Zkontrolujte výsledek** v okně pod **Nápověda – klávesy a zkratky**. Uvidíte tam,
   co platí, a chybné položky i s důvodem.

Příklad – číslice na klávesnici přepínají kanály, `P` zapíná televizi a multimediální
klávesy ovládají hlasitost:

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

Pozor na syntaxi JSON: názvy i akce v uvozovkách, mezi řádky čárka, za posledním řádkem
sekce čárka není. Když v souboru bude chyba, ovladač ho nepřepíše: poběží s výchozím
nastavením, nic neuloží a okno (i při spuštění s `--hidden`) ukáže, na kterém řádku
a sloupci chyba je. Soubor opravte a ovladač restartujte. Zkontrolovat ho můžete i
předem, třeba `python3 -m json.tool ~/.config/philips-controller/config.json`.

### Globální zkratky (`hotkeys`)

Zápis je stejný jako v xbindkeys: modifikátory a klávesa spojené znakem `+`.

- **Modifikátory:** `Shift`, `Ctrl` (nebo `Control`), `Alt` (`Mod1`), `Super` (`Win`,
  `Mod4`); na velikosti písmen nezáleží.
- **Klávesa:** název keysymu – `t`, `space`, `Return`, `BackSpace`, `Page_Up`, `F5`,
  `XF86AudioRaiseVolume`… (záleží na velikosti písmen) – nebo keycode ve tvaru `c:123`.

Název klávesy zjistíte příkazem `xev -event keyboard`. Po stisku vypíše mimo jiné řádek

```
state 0x0, keycode 123 (keysym 0x1008ff13, XF86AudioRaiseVolume), same_screen YES,
```

a do configu pak napíšete `XF86AudioRaiseVolume`, nebo `c:123`. Keycode se hodí, když
stejný keysym posílá víc kláves (třeba přídavná tlačítka klávesnice nebo myši) a chcete
použít jen jednu z nich.

Zkratka nejde zaregistrovat, když ji už drží jiný program nebo desktopové prostředí
(GNOME a KDE si obvykle berou multimediální klávesy). Nápověda v aplikaci ji pak ukáže
přeškrtnutou a po najetí myší řekne proč. Pod Waylandem globální zkratky spolehlivě
nefungují.

Výchozí globální zkratky:

| Zkratka | Akce |
| --- | --- |
| `Super+Alt+t` | `ShowWindow` – zobrazí ovladač, aktivní ho schová |
| `Super+Alt+Up` / `Down` / `Left` / `Right` | šipky (`CursorUp`…) |
| `Super+Alt+Return` | `Confirm` (OK) |
| `Super+Alt+BackSpace` | `Back` |
| `Super+Alt+Home` | `Home` |
| `Super+Alt+space` | `PlayPause` |
| `Super+Alt+Page_Up` / `Page_Down` | `VolumeUp` / `VolumeDown` |
| `Super+Alt+m` | `Mute` |
| `Super+Alt+Insert` / `Delete` | `PowerOn` / `PowerOff` |

### Klávesy v okně (`window_keys`)

Klávesa se zapisuje názvem, jak ho zná [egui](https://github.com/emilk/egui), a to bez
modifikátorů: `Up`, `Down`, `Left`, `Right`, `Enter`, `Escape`, `Backspace`, `Tab`,
`Space`, `Home`, `End`, `PageUp`, `PageDown`, `Insert`, `Delete`, písmena `A`–`Z`,
číslice `0`–`9`, `F1`–`F12`, nebo přímo znak jako `+`, `-`, `=`, `,`, `.`. U pojmenovaných
kláves záleží na velikosti písmen (`Up`, ne `up`).

- Rozhoduje znak, který klávesa píše v aktuálním rozložení klávesnice – v českém
  rozložení je tedy na klávese 1 znak `+`.
- Klávesy stisknuté s Ctrl nebo Alt ovladač nechává oknu a při psaní do políček
  (IP adresa, PIN) se nepoužijí vůbec.
- Držená klávesa se opakuje (hodí se pro hlasitost); `PowerOn`, `PowerOff`
  a schování okna se neopakují.
- `ShowWindow` i `ToggleWindow` aktivní okno schovají.

Výchozí klávesy v okně:

| Klávesa | Akce |
| --- | --- |
| `Up`, `Down`, `Left`, `Right` | šipky (`CursorUp`…) |
| `Enter` | `Confirm` (OK) |
| `Backspace`, `Escape` | `Back` |
| `H`, `Home` | `Home` |
| `Space` | `PlayPause` |
| `Plus`, `Equals` | `VolumeUp` |
| `Minus` | `VolumeDown` |
| `M` | `Mute` |

### Akce

Stejné pro obě sekce; na velikosti písmen záleží.

| Skupina | Akce |
| --- | --- |
| Navigace | `CursorUp`, `CursorDown`, `CursorLeft`, `CursorRight`, `Confirm`, `Back`, `Home`, `Options`, `Info`, `Find`, `Source`, `WatchTV` |
| Přehrávání | `PlayPause`, `Play`, `Pause`, `Stop`, `Rewind`, `FastForward`, `Previous`, `Next`, `Record` |
| Hlasitost a kanály | `VolumeUp`, `VolumeDown`, `Mute`, `ChannelStepUp`, `ChannelStepDown` |
| Barevná tlačítka | `RedColour`, `GreenColour`, `YellowColour`, `BlueColour` |
| Číslice | `Digit0` … `Digit9` |
| Ostatní klávesy TV | `Standby` (přepne standby), `AmbilightOnOff`, `Subtitle`, `Teletext` |
| Ovladač | `PowerOn` (Wake-on-LAN + zapnutí), `PowerOff`, `ShowWindow` (zobrazí ovladač, aktivní schová), `ToggleWindow` (zobrazí / schová) |

## Vývoj

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Totéž pouští CI na GitHubu (`.github/workflows/ci.yml`). Texty rozhraní jsou v obou
jazycích přímo v místě použití – `tr("česky", "English")` a `trf!(…)` ze
[`src/i18n.rs`](src/i18n.rs).

## Poděkování a upozornění

Protokol JointSpace API a postup párování jsou zdokumentované díky komunitním projektům
jako [pylips](https://github.com/eslavnov/pylips) a
[ha-philipsjs](https://github.com/danielperna84/ha-philipsjs). Sdílený klíč pro podpis
párovacího PINu v [`src/tv.rs`](src/tv.rs) pochází z oficiální aplikace Philips a je
veřejně známý.

Projekt není nijak spojen se společnostmi Philips ani TP Vision; Philips je ochranná
známka společnosti Koninklijke Philips N.V.

## Licence

Licencováno podle vaší volby pod [Apache License 2.0](LICENSE-APACHE) nebo
[MIT](LICENSE-MIT).

Pokud výslovně neuvedete jinak, jakýkoli příspěvek, který záměrně odešlete k zahrnutí
do projektu (ve smyslu licence Apache-2.0), bude licencován duálně jako výše, bez
dalších podmínek.
