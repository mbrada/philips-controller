//! Klient pro JointSpace API televizí Philips.
//!
//! Novější televize (API 6, od roku 2016) běží na HTTPS portu 1926 s digest autentizací
//! a před prvním použitím je potřeba je spárovat PINem z obrazovky. Starší modely
//! poslouchají na HTTP portu 1925 bez autentizace.

use std::fmt;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use digest_auth::{AuthContext, HttpMethod, WwwAuthenticateHeader};
use hmac::{Hmac, KeyInit, Mac};
use rand::RngExt;
use reqwest::blocking::{Client, Response};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, WWW_AUTHENTICATE};
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha1::Sha1;

use crate::i18n::{tr, trf};

/// Sdílený klíč oficiální aplikace Philips, slouží jen k podpisu párovacího PINu.
/// Je veřejně známý a stejný pro všechny klienty (používají ho např. pylips a ha-philipsjs),
/// nejde o tajemství tohoto projektu.
const PAIRING_SECRET: &str =
    "JCqdN5AcnAHgJYseUn7ER5k3qgtemfUvMRghQpTfTZq7Cvv8EPQPqfz6dDxPQPSu4gKFPWkJGw32zyASgJkHwCjU";

const HTTP_PORT: u16 = 1925;
const HTTPS_PORT: u16 = 1926;

/// TV odmítla požadavek kvůli chybějícímu nebo neplatnému spárování.
#[derive(Debug)]
pub struct AuthRejected;

impl fmt::Display for AuthRejected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(tr("TV vyžaduje spárování", "The TV requires pairing"))
    }
}

impl std::error::Error for AuthRejected {}

/// Chyba spojení – TV neodpověděla vůbec (na rozdíl od odpovědi s chybou).
pub fn is_unreachable(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause.downcast_ref::<reqwest::Error>().is_some_and(|err| {
            err.is_timeout() || err.is_connect() || err.is_request() || err.is_body()
        })
    })
}

/// Rychle ověří, že TV přijímá spojení – vypnutá TV tak nezdrží o celé timeouty HTTP klienta.
pub fn accepts_connections(host: &str) -> bool {
    [HTTPS_PORT, HTTP_PORT].into_iter().any(|port| {
        (host, port)
            .to_socket_addrs()
            .ok()
            .and_then(|mut addrs| addrs.next())
            .is_some_and(|addr| {
                TcpStream::connect_timeout(&addr, Duration::from_millis(700)).is_ok()
            })
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Credentials {
    pub user: String,
    pub password: String,
}

#[derive(Clone, Debug)]
pub struct SystemInfo {
    pub name: String,
    pub api_version: u32,
    /// TV vyžaduje HTTPS a párování.
    pub secure: bool,
}

/// Rozpracované párování mezi `pair/request` a `pair/grant`.
pub struct PairingSession {
    device_id: String,
    auth_key: String,
    timestamp: Value,
}

pub struct PhilipsTv {
    host: String,
    info: SystemInfo,
    credentials: Option<Credentials>,
    /// Poslední digest výzva – opakovaným použitím ušetříme u každého stisku jeden požadavek.
    digest_challenge: Option<WwwAuthenticateHeader>,
    client: Client,
}

impl PhilipsTv {
    pub fn new(host: &str, info: SystemInfo, credentials: Option<Credentials>) -> Result<Self> {
        Ok(Self {
            host: host.to_owned(),
            info,
            credentials,
            digest_challenge: None,
            client: http_client(Duration::from_secs(3))?,
        })
    }

    /// Zjistí, jestli na adrese běží televize Philips a jakou verzi API používá.
    pub fn probe(host: &str) -> Result<SystemInfo> {
        let client = http_client(Duration::from_secs(2))?;
        let urls = [
            format!("http://{host}:{HTTP_PORT}/system"),
            format!("https://{host}:{HTTPS_PORT}/6/system"),
            format!("http://{host}:{HTTP_PORT}/1/system"),
        ];
        let mut last_error = anyhow!(not_responding(host));
        for url in urls {
            match client.get(&url).send().and_then(Response::error_for_status) {
                Ok(resp) => {
                    let system: Value = resp.json().context(tr(
                        "TV vrátila neplatná data",
                        "The TV returned invalid data",
                    ))?;
                    return Ok(parse_system(&system, url.ends_with("/1/system")));
                }
                Err(err) => last_error = err.into(),
            }
        }
        Err(last_error)
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn info(&self) -> &SystemInfo {
        &self.info
    }

    pub fn needs_pairing(&self) -> bool {
        self.info.secure && self.credentials.is_none()
    }

    /// Ověří, že TV odpovídá. Stačí jakákoli odpověď – `system` nevyžaduje přihlášení.
    pub fn ping(&self) -> Result<()> {
        self.client
            .get(self.url("system"))
            .send()
            .with_context(|| not_responding(&self.host))?;
        Ok(())
    }

    pub fn send_key(&mut self, key: &str) -> Result<()> {
        self.post("input/key", &json!({ "key": key }))
    }

    /// Vrací např. `On`, `Standby` nebo `StandbyKeep`.
    pub fn power_state(&mut self) -> Result<String> {
        let resp = self.request(Method::GET, "powerstate", None)?;
        let state: Value = resp.json()?;
        state["powerstate"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| {
                anyhow!(tr(
                    "TV nevrátila stav napájení",
                    "The TV did not report its power state",
                ))
            })
    }

    pub fn set_power_state(&mut self, state: &str) -> Result<()> {
        self.post("powerstate", &json!({ "powerstate": state }))
    }

    /// První krok párování – TV zobrazí na obrazovce PIN.
    pub fn pair_request(&self) -> Result<PairingSession> {
        let device_id = random_device_id();
        let body = json!({
            "scope": ["read", "write", "control"],
            "device": device_spec(&device_id),
        });
        let resp: Value = self
            .client
            .post(self.url("pair/request"))
            .json(&body)
            .send()?
            .json()
            .context(tr(
                "TV na žádost o spárování odpověděla neplatnými daty",
                "The TV answered the pairing request with invalid data",
            ))?;
        if resp["error_id"] != "SUCCESS" {
            bail!(trf!(
                "TV odmítla spárování: {}",
                "The TV refused pairing: {}",
                resp["error_text"]
            ));
        }
        Ok(PairingSession {
            device_id,
            auth_key: resp["auth_key"]
                .as_str()
                .ok_or_else(|| {
                    anyhow!(tr(
                        "TV neposlala párovací klíč",
                        "The TV did not send a pairing key",
                    ))
                })?
                .to_owned(),
            timestamp: resp["timestamp"].clone(),
        })
    }

    /// Dokončí párování PINem z obrazovky a vrátí přihlašovací údaje k uložení.
    pub fn pair_grant(&mut self, session: PairingSession, pin: &str) -> Result<Credentials> {
        let timestamp = match &session.timestamp {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        let body = json!({
            "auth": {
                "auth_AppId": "1",
                "pin": pin,
                "auth_timestamp": session.timestamp,
                "auth_signature": pairing_signature(&format!("{timestamp}{pin}")),
            },
            "device": device_spec(&session.device_id),
        });
        let credentials = Credentials {
            user: session.device_id,
            password: session.auth_key,
        };
        self.credentials = Some(credentials.clone());
        self.digest_challenge = None;
        let result = self
            .request(Method::POST, "pair/grant", Some(&body))
            .and_then(|resp| {
                let status = resp.status();
                let reply: Value = resp.json().unwrap_or_default();
                match reply["error_id"].as_str() {
                    Some("SUCCESS") => Ok(()),
                    None if status.is_success() => Ok(()),
                    _ => Err(anyhow!(trf!(
                        "TV odpověděla {status}: {}",
                        "The TV answered {status}: {}",
                        reply["error_text"]
                    ))),
                }
            });
        if let Err(err) = result {
            self.credentials = None;
            return Err(err.context(tr(
                "spárování se nepovedlo – zkontrolujte PIN",
                "pairing failed – check the PIN",
            )));
        }
        Ok(credentials)
    }

    fn post(&mut self, path: &str, body: &Value) -> Result<()> {
        let resp = self.request(Method::POST, path, Some(body))?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().unwrap_or_default();
            bail!(trf!(
                "TV vrátila chybu {status}: {}",
                "The TV returned error {status}: {}",
                text.trim()
            ));
        }
        Ok(())
    }

    fn request(&mut self, method: Method, path: &str, body: Option<&Value>) -> Result<Response> {
        let url = self.url(path);
        let uri = format!("/{}/{path}", self.info.api_version);
        let body = body.map(serde_json::to_vec).transpose()?;

        let send = |authorization: Option<String>| {
            let mut req = self.client.request(method.clone(), &url);
            if let Some(body) = &body {
                req = req
                    .header(CONTENT_TYPE, "application/json")
                    .body(body.clone());
            }
            if let Some(authorization) = authorization {
                req = req.header(AUTHORIZATION, authorization);
            }
            req.send().with_context(|| not_responding(&self.host))
        };
        let authorize = |challenge: &mut WwwAuthenticateHeader, creds: &Credentials| {
            let context = AuthContext::new_with_method(
                creds.user.as_str(),
                creds.password.as_str(),
                uri.as_str(),
                body.as_deref(),
                HttpMethod::from(method.as_str()),
            );
            challenge
                .respond(&context)
                .map(|header| header.to_header_string())
        };

        // Nejdřív zkusíme uloženou výzvu, ať nemusí každý stisk čekat na 401.
        let cached = match (&mut self.digest_challenge, &self.credentials) {
            (Some(challenge), Some(creds)) => authorize(challenge, creds).ok(),
            _ => None,
        };
        let resp = send(cached)?;
        if resp.status() != StatusCode::UNAUTHORIZED {
            return Ok(resp);
        }

        let Some(creds) = &self.credentials else {
            bail!(AuthRejected);
        };
        let header = resp
            .headers()
            .get(WWW_AUTHENTICATE)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| {
                anyhow!(tr(
                    "TV nepožaduje digest autentizaci",
                    "The TV does not ask for digest authentication",
                ))
            })?;
        let mut challenge = digest_auth::parse(header)?;
        let authorization = authorize(&mut challenge, creds)?;
        let resp = send(Some(authorization))?;
        if resp.status() == StatusCode::UNAUTHORIZED {
            self.digest_challenge = None;
            bail!(AuthRejected);
        }
        self.digest_challenge = Some(challenge);
        Ok(resp)
    }

    fn url(&self, path: &str) -> String {
        let (scheme, port) = if self.info.secure {
            ("https", HTTPS_PORT)
        } else {
            ("http", HTTP_PORT)
        };
        format!(
            "{scheme}://{}:{port}/{}/{path}",
            self.host, self.info.api_version
        )
    }
}

fn not_responding(host: &str) -> String {
    trf!(
        "TV na {host} neodpovídá",
        "The TV at {host} is not responding"
    )
}

fn http_client(timeout: Duration) -> Result<Client> {
    // TV používá certifikát podepsaný sama sebou.
    Ok(Client::builder()
        .tls_danger_accept_invalid_certs(true)
        .connect_timeout(Duration::from_millis(1500))
        .timeout(timeout)
        .build()?)
}

fn parse_system(system: &Value, legacy: bool) -> SystemInfo {
    let api_version = system["api_version"]["Major"]
        .as_u64()
        .map_or(if legacy { 1 } else { 5 }, |major| major as u32);
    let secure = system["featuring"]["systemfeatures"]["pairing_type"] == "digest_auth_pairing";
    SystemInfo {
        name: system["name"].as_str().unwrap_or("Philips TV").to_owned(),
        api_version,
        secure,
    }
}

fn device_spec(device_id: &str) -> Value {
    json!({
        "device_name": "PhilipsController",
        "device_os": "Android",
        "app_name": "Philips Controller",
        "type": "native",
        "app_id": "app.id",
        "id": device_id,
    })
}

fn random_device_id() -> String {
    rand::rng()
        .sample_iter(rand::distr::Alphanumeric)
        .take(16)
        .map(char::from)
        .collect()
}

/// Base64 z hexadecimálního HMAC-SHA1, stejně jako oficiální aplikace.
fn pairing_signature(message: &str) -> String {
    let key = BASE64.decode(PAIRING_SECRET).expect("platný base64 klíč");
    let mut mac = Hmac::<Sha1>::new_from_slice(&key).expect("HMAC přijme klíč libovolné délky");
    mac.update(message.as_bytes());
    let hex: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    BASE64.encode(hex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_saphi_system_info() {
        let system = json!({
            "name": "55PUS7502/12",
            "api_version": { "Major": 6, "Minor": 1, "Patch": 0 },
            "featuring": { "systemfeatures": { "pairing_type": "digest_auth_pairing" } },
        });
        let info = parse_system(&system, false);
        assert_eq!(info.name, "55PUS7502/12");
        assert_eq!(info.api_version, 6);
        assert!(info.secure);
    }

    #[test]
    fn legacy_tv_without_version_uses_api_1() {
        let info = parse_system(&json!({ "name": "42PFL" }), true);
        assert_eq!(info.api_version, 1);
        assert!(!info.secure);
    }

    #[test]
    fn signature_is_base64_of_hex_digest() {
        let signature = pairing_signature("16000000001234");
        let hex = BASE64.decode(&signature).unwrap();
        assert_eq!(hex.len(), 40);
        assert!(hex.iter().all(u8::is_ascii_hexdigit));
    }

    #[test]
    fn refused_connection_is_unreachable() {
        // Port 1 na localhostu nikdo neposlouchá – spojení je okamžitě odmítnuto.
        let err = http_client(Duration::from_secs(1))
            .unwrap()
            .get("http://127.0.0.1:1/system")
            .send()
            .context("TV neodpovídá")
            .unwrap_err();
        assert!(is_unreachable(&err));
        assert!(!is_unreachable(&anyhow!("TV vrátila chybu 500")));
        assert!(!is_unreachable(&anyhow!(AuthRejected)));
    }

    #[test]
    fn device_id_is_alphanumeric() {
        let id = random_device_id();
        assert_eq!(id.len(), 16);
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric()));
    }
}
