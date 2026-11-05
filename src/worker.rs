//! Síťová komunikace s televizí běží ve vlastním vlákně, aby GUI nezamrzalo.
//!
//! Když TV přestane odpovídat, worker se k ní připojí znovu sám: se zobrazeným oknem
//! průběžně na pozadí (s prodlužujícími se pauzami), se skrytým hned při dalším příkazu.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use eframe::egui;

use crate::config::{Config, Layout};
use crate::discovery::{self, FoundTv};
use crate::i18n::{tr, trf};
use crate::tv::{self, AuthRejected, PairingSession, PhilipsTv};
use crate::wol;

/// Stisk, který čekal ve frontě déle, už nemá smysl posílat (TV byla nedostupná).
const KEY_MAX_AGE: Duration = Duration::from_millis(1500);
const POWER_ON_TIMEOUT: Duration = Duration::from_secs(25);
/// Jak často se při zobrazeném okně ověřuje, že TV pořád odpovídá.
const HEARTBEAT: Duration = Duration::from_secs(20);
const RETRY_FIRST: Duration = Duration::from_secs(1);
const RETRY_MAX: Duration = Duration::from_secs(15);
/// Stisky během výpadku se řadí do fronty – každý nemusí znovu čekat na nedostupnou TV.
const RETRY_ON_DEMAND_GAP: Duration = Duration::from_secs(2);

pub enum Command {
    Discover,
    Connect(String),
    StartPairing,
    FinishPairing(String),
    Key(&'static str, Instant),
    PowerOn,
    PowerOff,
    /// Okno se zobrazilo / schovalo – na pozadí se spojení hlídá, jen když je vidět.
    Visible(bool),
    /// Uloží zvolené rozložení tlačítek (nastavení ukládá jen worker).
    SaveLayout(Layout),
}

pub enum Event {
    Busy(String),
    Info(String),
    Error(String),
    Discovered(Vec<FoundTv>),
    Connected {
        host: String,
        name: String,
        needs_pairing: bool,
    },
    /// TV přestala odpovídat; worker se k ní připojí znovu sám.
    Offline(String),
    /// Nastavení nejde uložit, protože soubor mezitím přestal jít přečíst.
    ConfigError(String),
    PairingStarted,
    Paired,
}

pub fn spawn(ctx: egui::Context, config: Config) -> (Sender<Command>, Receiver<Event>) {
    let (command_tx, command_rx) = channel();
    let (event_tx, event_rx) = channel();
    thread::spawn(move || {
        let mut worker = Worker {
            config,
            tv: None,
            pairing: None,
            retry: None,
            last_seen: Instant::now(),
            visible: false,
            events: event_tx,
            ctx,
        };
        worker.start();
        loop {
            let command = match worker.next_check() {
                Some(at) => {
                    match command_rx.recv_timeout(at.saturating_duration_since(Instant::now())) {
                        Ok(command) => Some(command),
                        Err(RecvTimeoutError::Timeout) => None,
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
                None => match command_rx.recv() {
                    Ok(command) => Some(command),
                    Err(_) => break,
                },
            };
            match command {
                Some(command) => worker.handle(command),
                None => worker.check_connection(),
            }
        }
    });
    (command_tx, event_rx)
}

/// TV neodpovídá – kdy to zkusit znovu.
struct Retry {
    next: Instant,
    delay: Duration,
    last_attempt: Option<Instant>,
}

impl Retry {
    fn new() -> Self {
        Self {
            next: Instant::now() + RETRY_FIRST,
            delay: RETRY_FIRST,
            last_attempt: None,
        }
    }

    fn failed(&mut self) {
        let now = Instant::now();
        self.last_attempt = Some(now);
        self.next = now + self.delay;
        self.delay = (self.delay * 2).min(RETRY_MAX);
    }

    fn attempted_recently(&self) -> bool {
        self.last_attempt
            .is_some_and(|at| at.elapsed() < RETRY_ON_DEMAND_GAP)
    }
}

struct Worker {
    config: Config,
    tv: Option<PhilipsTv>,
    pairing: Option<PairingSession>,
    /// Spojení s TV vypadlo (nebo ještě nevzniklo); `tv` drží poslední známé údaje.
    retry: Option<Retry>,
    /// Kdy TV naposledy odpověděla.
    last_seen: Instant,
    visible: bool,
    events: Sender<Event>,
    ctx: egui::Context,
}

impl Worker {
    fn emit(&self, event: Event) {
        let _ = self.events.send(event);
        self.ctx.request_repaint();
    }

    fn report(&mut self, result: Result<()>) {
        let Err(err) = result else { return };
        if err.is::<AuthRejected>() {
            self.forget_credentials();
        }
        let text = if tv::is_unreachable(&err) && self.tv.is_some() {
            self.retry.get_or_insert_with(Retry::new);
            trf!(
                "TV přestala odpovídat ({}). Ovladač se připojí sám, jakmile zase odpoví.",
                "The TV stopped responding ({}). The remote will reconnect once it responds again.",
                err.root_cause()
            )
        } else {
            format!("{err:#}")
        };
        self.emit(if self.retry.is_some() {
            Event::Offline(text)
        } else {
            Event::Error(text)
        });
    }

    fn is_online(&self) -> bool {
        self.tv.is_some() && self.retry.is_none()
    }

    fn known_host(&self) -> Option<String> {
        self.tv
            .as_ref()
            .map(|tv| tv.host().to_owned())
            .or_else(|| self.config.host.clone())
    }

    fn unreachable_text(&self, cause: Option<&anyhow::Error>) -> String {
        let name = self
            .config
            .name
            .clone()
            .or_else(|| self.known_host())
            .unwrap_or_else(|| "TV".into());
        let cause = cause.map_or(String::new(), |err| format!(" ({})", err.root_cause()));
        trf!(
            "{name} neodpovídá{cause} – je zapnutá? Ovladač se připojí sám, jakmile odpoví.",
            "{name} is not responding{cause} – is it on? The remote will reconnect once it responds."
        )
    }

    /// Kdy se má worker sám probudit a zkontrolovat spojení.
    fn next_check(&self) -> Option<Instant> {
        if !self.visible {
            return None;
        }
        match (&self.retry, &self.tv) {
            (Some(retry), _) => Some(retry.next),
            (None, Some(_)) => Some(self.last_seen + HEARTBEAT),
            (None, None) => None,
        }
    }

    fn check_connection(&mut self) {
        if self.retry.is_some() {
            self.reconnect();
        } else if self.tv.is_some() {
            let result = self.with_tv(|tv| tv.ping());
            self.report(result);
        }
    }

    fn start(&mut self) {
        match self.config.host.clone() {
            Some(host) => {
                self.try_connect(&host);
            }
            None => self.discover(),
        }
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::Discover => self.discover(),
            Command::Connect(host) => {
                if let Err(err) = self.connect(&host) {
                    self.emit(Event::Error(format!("{err:#}")));
                }
            }
            Command::StartPairing => {
                if self.ensure_online() {
                    let result = self.start_pairing();
                    self.report(result);
                }
            }
            Command::FinishPairing(pin) => {
                let result = self.finish_pairing(&pin);
                self.report(result);
            }
            Command::Key(key, pressed_at) => {
                if pressed_at.elapsed() > KEY_MAX_AGE || !self.ensure_online() {
                    return;
                }
                // Opětovné připojení mohlo chvíli trvat.
                if pressed_at.elapsed() > KEY_MAX_AGE {
                    return;
                }
                let result = self.with_tv(|tv| tv.send_key(key));
                self.report(result);
            }
            Command::PowerOn => {
                let result = self.power_on();
                self.report(result);
            }
            Command::PowerOff => {
                if self.ensure_online() {
                    let result = self.power_off();
                    self.report(result);
                }
            }
            Command::Visible(visible) => {
                self.visible = visible;
                // Po zobrazení okna hned ověříme, že stav spojení v hlavičce platí.
                if visible {
                    self.check_connection();
                }
            }
            Command::SaveLayout(layout) => {
                self.config.layout = layout;
                self.save_config();
            }
        }
    }

    /// Provede požadavek na TV a zapamatuje si, jestli odpověděla.
    fn with_tv<T>(&mut self, request: impl FnOnce(&mut PhilipsTv) -> Result<T>) -> Result<T> {
        let tv = self
            .tv
            .as_mut()
            .ok_or_else(|| anyhow!(tr("Není připojená žádná TV", "No TV is connected")))?;
        let result = request(tv);
        if !result.as_ref().is_err_and(tv::is_unreachable) {
            self.last_seen = Instant::now();
        }
        result
    }

    /// Před příkazem pro TV: když neodpovídala, zkusí se hned připojit znovu.
    fn ensure_online(&mut self) -> bool {
        if self.is_online() {
            return true;
        }
        if self.known_host().is_none() {
            self.emit(Event::Error(
                tr("Není připojená žádná TV", "No TV is connected").into(),
            ));
            return false;
        }
        if self.retry.as_ref().is_some_and(Retry::attempted_recently) {
            self.emit(Event::Offline(self.unreachable_text(None)));
            return false;
        }
        self.reconnect()
    }

    /// Znovu se připojí k naposledy použité TV.
    fn reconnect(&mut self) -> bool {
        let Some(host) = self.known_host() else {
            self.retry = None;
            return false;
        };
        let was_connected = self.tv.is_some();
        let connected = self.try_connect(&host);
        if connected && was_connected && !self.tv.as_ref().is_some_and(PhilipsTv::needs_pairing) {
            self.emit(Event::Info(
                tr(
                    "Spojení obnoveno ✔ – ovládejte šipkami",
                    "Connection restored ✔ – use the arrow keys",
                )
                .into(),
            ));
        }
        connected
    }

    /// Připojí se, jestli TV odpovídá; jinak naplánuje další pokus.
    fn try_connect(&mut self, host: &str) -> bool {
        if !tv::accepts_connections(host) {
            self.retry.get_or_insert_with(Retry::new).failed();
            self.emit(Event::Offline(self.unreachable_text(None)));
            return false;
        }
        match self.connect(host) {
            Ok(()) => true,
            Err(err) => {
                self.retry.get_or_insert_with(Retry::new).failed();
                self.emit(Event::Offline(self.unreachable_text(Some(&err))));
                false
            }
        }
    }

    fn discover(&mut self) {
        self.emit(Event::Busy(
            tr("Hledám televize v síti…", "Searching the network for TVs…").into(),
        ));
        let found = discovery::discover();
        let only = (found.len() == 1).then(|| found[0].host.clone());
        self.emit(Event::Discovered(found));
        if let (Some(host), None) = (only, &self.tv) {
            let result = self.connect(&host);
            self.report(result);
        }
    }

    /// Uloží nastavení. Když to nejde, ovladač funguje dál a okno na to upozorní.
    fn save_config(&self) {
        if let Err(err) = self.config.save() {
            self.emit(Event::ConfigError(format!("{err:#}")));
        }
    }

    fn connect(&mut self, host: &str) -> Result<()> {
        self.emit(Event::Busy(trf!(
            "Připojuji se k {host}…",
            "Connecting to {host}…"
        )));
        let info = PhilipsTv::probe(host)?;

        if self.config.host.as_deref() != Some(host) {
            // Jiná TV – údaje té předchozí neplatí, zkratky zůstávají.
            self.config.host = Some(host.to_owned());
            self.config.mac = None;
            self.config.credentials = None;
        }
        self.config.name = Some(info.name.clone());
        if let Some(mac) = wol::mac_from_arp(host) {
            self.config.mac = Some(mac);
        }
        self.save_config();

        let mut tv = PhilipsTv::new(host, info, self.config.credentials.clone())?;
        // Ověříme, že uložené párování pořád platí.
        if !tv.needs_pairing()
            && let Err(err) = tv.power_state()
            && err.is::<AuthRejected>()
        {
            tv = PhilipsTv::new(host, tv.info().clone(), None)?;
            self.config.credentials = None;
            self.save_config();
        }

        self.emit(Event::Connected {
            host: host.to_owned(),
            name: tv.info().name.clone(),
            needs_pairing: tv.needs_pairing(),
        });
        self.tv = Some(tv);
        self.retry = None;
        self.last_seen = Instant::now();
        Ok(())
    }

    fn start_pairing(&mut self) -> Result<()> {
        let Some(tv) = &self.tv else {
            anyhow::bail!(tr(
                "Nejdřív se připojte k televizi",
                "Connect to a TV first"
            ));
        };
        self.emit(Event::Busy(
            tr("Žádám TV o spárování…", "Asking the TV to pair…").into(),
        ));
        self.pairing = Some(tv.pair_request()?);
        self.emit(Event::PairingStarted);
        Ok(())
    }

    fn finish_pairing(&mut self, pin: &str) -> Result<()> {
        self.emit(Event::Busy(tr("Ověřuji PIN…", "Verifying the PIN…").into()));
        let (Some(tv), Some(session)) = (&mut self.tv, self.pairing.take()) else {
            anyhow::bail!(tr(
                "Párování nebylo zahájeno",
                "Pairing has not been started"
            ));
        };
        let credentials = tv.pair_grant(session, pin)?;
        self.config.credentials = Some(credentials);
        self.save_config();
        self.emit(Event::Paired);
        Ok(())
    }

    fn forget_credentials(&mut self) {
        let Some(tv) = &self.tv else { return };
        let (host, info) = (tv.host().to_owned(), tv.info().clone());
        if let Ok(tv) = PhilipsTv::new(&host, info, None) {
            self.tv = Some(tv);
            self.config.credentials = None;
            self.save_config();
            self.emit(Event::Connected {
                name: self.config.name.clone().unwrap_or_default(),
                host,
                needs_pairing: true,
            });
        }
    }

    fn power_off(&mut self) -> Result<()> {
        if self.with_tv(PhilipsTv::power_state)? != "On" {
            self.emit(Event::Info(
                tr("TV už je vypnutá", "The TV is already off").into(),
            ));
            return Ok(());
        }
        // Klávesa Standby přepíná, proto ji použijeme jen jako zálohu, když víme, že TV běží.
        self.with_tv(|tv| {
            tv.set_power_state("Standby")
                .or_else(|_| tv.send_key("Standby"))
        })?;
        self.emit(Event::Info(
            tr("TV se vypíná", "The TV is turning off").into(),
        ));
        Ok(())
    }

    fn power_on(&mut self) -> Result<()> {
        let host = self
            .known_host()
            .ok_or_else(|| anyhow!(tr("Není známá žádná TV", "No TV is known yet")))?;

        self.emit(Event::Busy(tr("Zapínám TV…", "Turning the TV on…").into()));
        if let Some(mac) = &self.config.mac {
            wol::wake(mac, &host)?;
        }

        let deadline = Instant::now() + POWER_ON_TIMEOUT;
        let mut requested = false;
        while Instant::now() < deadline {
            if !self.is_online() {
                // TV byla úplně vypnutá – čekáme, až se po Wake-on-LAN probudí API.
                if !tv::accepts_connections(&host) || self.connect(&host).is_err() {
                    thread::sleep(Duration::from_secs(1));
                    continue;
                }
                self.emit(Event::Busy(tr("Zapínám TV…", "Turning the TV on…").into()));
            }
            match self.with_tv(PhilipsTv::power_state) {
                Ok(state) if state == "On" => {
                    self.emit(Event::Info(tr("TV je zapnutá", "The TV is on").into()));
                    return Ok(());
                }
                Ok(_) if !requested => {
                    requested = true;
                    self.with_tv(|tv| {
                        tv.set_power_state("On").or_else(|_| tv.send_key("Standby"))
                    })?;
                }
                Err(err) if err.is::<AuthRejected>() => return Err(err),
                // Spojení vypadlo – další kolo počká, až TV zase přijme připojení.
                Err(err) if tv::is_unreachable(&err) => {
                    self.retry.get_or_insert_with(Retry::new);
                }
                _ => {}
            }
            thread::sleep(Duration::from_millis(700));
        }

        if self.config.mac.is_none() && !self.is_online() {
            anyhow::bail!(tr(
                "TV neodpovídá a neznám její MAC adresu pro Wake-on-LAN",
                "The TV is not responding and its MAC address for Wake-on-LAN is unknown",
            ));
        }
        anyhow::bail!(tr(
            "TV se nepodařilo zapnout – je v nastavení TV povolené „Zapnout přes Wi-Fi/síť“?",
            "Could not turn the TV on – is “Switch on with Wi-Fi/network” enabled \
             in the TV settings?",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_backs_off_up_to_limit() {
        let mut retry = Retry::new();
        assert!(!retry.attempted_recently());
        let mut delays = Vec::new();
        for _ in 0..6 {
            retry.failed();
            delays.push(retry.next.duration_since(retry.last_attempt.unwrap()));
        }
        let secs: Vec<u64> = delays.iter().map(Duration::as_secs).collect();
        assert_eq!(secs, [1, 2, 4, 8, 15, 15]);
        assert!(retry.attempted_recently());
    }
}
