//! Wake-on-LAN pro zapnutí televize z hlubokého standby.

use std::net::{Ipv4Addr, UdpSocket};

use anyhow::{Result, anyhow};

use crate::i18n::trf;

pub fn wake(mac: &str, host: &str) -> Result<()> {
    let mac = parse_mac(mac).ok_or_else(|| {
        anyhow!(trf!(
            "neplatná MAC adresa {mac}",
            "invalid MAC address {mac}"
        ))
    })?;
    let mut packet = vec![0xFF; 6];
    for _ in 0..16 {
        packet.extend_from_slice(&mac);
    }

    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.set_broadcast(true)?;
    // Globální broadcast odchází jen výchozím rozhraním, proto i broadcast podsítě televize.
    let mut targets = vec![Ipv4Addr::BROADCAST];
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        let [a, b, c, _] = ip.octets();
        targets.push(Ipv4Addr::new(a, b, c, 255));
    }
    for target in targets {
        for port in [9, 7] {
            socket.send_to(&packet, (target, port))?;
        }
    }
    Ok(())
}

/// Najde MAC adresu v ARP tabulce jádra (jen Linux).
pub fn mac_from_arp(host: &str) -> Option<String> {
    let table = std::fs::read_to_string("/proc/net/arp").ok()?;
    find_in_arp_table(&table, host)
}

fn find_in_arp_table(table: &str, host: &str) -> Option<String> {
    table.lines().skip(1).find_map(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        match fields.as_slice() {
            [ip, _, _, mac, ..] if *ip == host && *mac != "00:00:00:00:00:00" => {
                Some(mac.to_string())
            }
            _ => None,
        }
    })
}

fn parse_mac(mac: &str) -> Option<[u8; 6]> {
    let bytes: Vec<u8> = mac
        .split([':', '-'])
        .map(|part| u8::from_str_radix(part, 16))
        .collect::<Result<_, _>>()
        .ok()?;
    bytes.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Adresy jsou z rozsahů pro dokumentaci (RFC 5737, RFC 7042).

    #[test]
    fn parses_mac_with_colons_and_dashes() {
        let expected = Some([0x00, 0x00, 0x5e, 0x00, 0x53, 0x2a]);
        assert_eq!(parse_mac("00:00:5e:00:53:2a"), expected);
        assert_eq!(parse_mac("00-00-5E-00-53-2A"), expected);
        assert_eq!(parse_mac("00:00:5e"), None);
    }

    #[test]
    fn finds_mac_in_arp_table() {
        let table = "IP address       HW type     Flags       HW address            Mask     Device\n\
                     192.0.2.1        0x1         0x2         00:00:5e:00:53:01     *        eth0\n\
                     192.0.2.25       0x1         0x2         00:00:5e:00:53:2a     *        eth0\n\
                     192.0.2.20       0x1         0x0         00:00:00:00:00:00     *        eth0\n";
        assert_eq!(
            find_in_arp_table(table, "192.0.2.25").as_deref(),
            Some("00:00:5e:00:53:2a")
        );
        assert_eq!(find_in_arp_table(table, "192.0.2.20"), None);
        assert_eq!(find_in_arp_table(table, "10.0.0.1"), None);
    }
}
