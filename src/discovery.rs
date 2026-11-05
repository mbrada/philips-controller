//! Hledání televizí Philips v lokální síti.
//!
//! Projde /24 rozsahy aktivních rozhraní, zkusí porty JointSpace API (1925/1926)
//! a každého kandidáta ověří dotazem na `/system`.

use std::collections::BTreeSet;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use if_addrs::IfAddr;

use crate::tv::{PhilipsTv, SystemInfo};

const PORTS: [u16; 2] = [1926, 1925];
const CONNECT_TIMEOUT: Duration = Duration::from_millis(300);
const SCAN_THREADS: usize = 128;
/// Virtuální rozhraní Dockeru, libvirtu apod. – televize za nimi nebude.
const IGNORED_INTERFACES: [&str; 4] = ["docker", "br-", "virbr", "veth"];

#[derive(Clone, Debug)]
pub struct FoundTv {
    pub host: String,
    pub info: SystemInfo,
}

pub fn discover() -> Vec<FoundTv> {
    let hosts: Vec<Ipv4Addr> = candidate_hosts().into_iter().collect();
    let next = AtomicUsize::new(0);
    let found = Mutex::new(Vec::new());

    thread::scope(|scope| {
        for _ in 0..SCAN_THREADS.min(hosts.len()) {
            scope.spawn(|| {
                while let Some(&ip) = hosts.get(next.fetch_add(1, Ordering::Relaxed)) {
                    if !has_open_port(ip) {
                        continue;
                    }
                    let host = ip.to_string();
                    if let Ok(info) = PhilipsTv::probe(&host) {
                        found.lock().unwrap().push(FoundTv { host, info });
                    }
                }
            });
        }
    });

    let mut found = found.into_inner().unwrap();
    found.sort_by_key(|tv| tv.host.parse::<Ipv4Addr>().ok());
    found
}

fn has_open_port(ip: Ipv4Addr) -> bool {
    PORTS.iter().any(|&port| {
        TcpStream::connect_timeout(&SocketAddr::from((ip, port)), CONNECT_TIMEOUT).is_ok()
    })
}

fn candidate_hosts() -> BTreeSet<Ipv4Addr> {
    let mut own = BTreeSet::new();
    let mut hosts = BTreeSet::new();
    for iface in if_addrs::get_if_addrs().unwrap_or_default() {
        let IfAddr::V4(addr) = &iface.addr else {
            continue;
        };
        if iface.is_loopback()
            || iface.is_p2p()
            || !iface.is_oper_up()
            || IGNORED_INTERFACES
                .iter()
                .any(|prefix| iface.name.starts_with(prefix))
        {
            continue;
        }
        own.insert(addr.ip);
        hosts.extend(subnet_hosts(addr.ip));
    }
    &hosts - &own
}

/// Všechny adresy v /24 kolem dané IP (větší sítě by se skenovaly příliš dlouho).
fn subnet_hosts(ip: Ipv4Addr) -> impl Iterator<Item = Ipv4Addr> {
    let [a, b, c, _] = ip.octets();
    (1..=254).map(move |d| Ipv4Addr::new(a, b, c, d))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subnet_covers_whole_24() {
        let hosts: Vec<_> = subnet_hosts(Ipv4Addr::new(192, 168, 1, 77)).collect();
        assert_eq!(hosts.len(), 254);
        assert_eq!(hosts[0], Ipv4Addr::new(192, 168, 1, 1));
        assert_eq!(hosts[253], Ipv4Addr::new(192, 168, 1, 254));
    }
}
