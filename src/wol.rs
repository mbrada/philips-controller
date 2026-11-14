//! Wake-on-LAN pro zapnutí televize z hlubokého standby.

use std::net::{Ipv4Addr, UdpSocket};

use anyhow::{Result, anyhow};

use crate::i18n::trf;

/// Pošle magic packet na všechny MAC adresy televize – TV může být připojená kabelem
/// i přes Wi-Fi a každé rozhraní se probouzí jen svou vlastní adresou.
pub fn wake(macs: &[String], host: &str) -> Result<()> {
    let macs = macs
        .iter()
        .map(|mac| {
            parse_mac(mac).ok_or_else(|| {
                anyhow!(trf!(
                    "neplatná MAC adresa {mac}",
                    "invalid MAC address {mac}"
                ))
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.set_broadcast(true)?;
    // Globální broadcast odchází jen výchozím rozhraním, proto i broadcast podsítě televize.
    let mut targets = vec![Ipv4Addr::BROADCAST];
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        let [a, b, c, _] = ip.octets();
        targets.push(Ipv4Addr::new(a, b, c, 255));
    }
    for mac in macs {
        let mut packet = vec![0xFF; 6];
        for _ in 0..16 {
            packet.extend_from_slice(&mac);
        }
        for target in &targets {
            for port in [9, 7] {
                socket.send_to(&packet, (*target, port))?;
            }
        }
    }
    Ok(())
}

/// Spojí nově zjištěné MAC adresy s uloženými. Seznam z API televize obsahuje všechna její
/// rozhraní, a proto uložené nahradí; ARP tabulka zná jen rozhraní, přes které TV právě
/// komunikuje, takže se jen přidá. Adresy jsou sjednocené na malá písmena s dvojtečkami.
pub fn merge_macs(known: &[String], from_tv: &[String], from_arp: Option<&str>) -> Vec<String> {
    let base = if from_tv.is_empty() { known } else { from_tv };
    let mut macs: Vec<String> = Vec::new();
    for mac in base.iter().map(String::as_str).chain(from_arp) {
        if let Some(mac) = normalize_mac(mac)
            && !macs.contains(&mac)
        {
            macs.push(mac);
        }
    }
    macs
}

fn normalize_mac(mac: &str) -> Option<String> {
    let bytes = parse_mac(mac)?;
    Some(
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<Vec<_>>()
            .join(":"),
    )
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

    #[test]
    fn mac_list_from_tv_replaces_stored_one() {
        // Uložená je jen Wi-Fi, TV teď hlásí Wi-Fi i kabel (velkými písmeny).
        let known = vec!["00:00:5e:00:53:01".to_string()];
        let from_tv = vec![
            "00:00:5E:00:53:01".to_string(),
            "00:00:5E:00:53:02".to_string(),
        ];
        assert_eq!(
            merge_macs(&known, &from_tv, Some("00:00:5e:00:53:02")),
            ["00:00:5e:00:53:01", "00:00:5e:00:53:02"]
        );
    }

    #[test]
    fn mac_from_arp_is_added_to_stored_ones() {
        // Starší TV bez seznamu rozhraní: adresa z ARP se přidá, stará zůstane.
        let known = vec!["00:00:5e:00:53:01".to_string()];
        assert_eq!(
            merge_macs(&known, &[], Some("00-00-5E-00-53-02")),
            ["00:00:5e:00:53:01", "00:00:5e:00:53:02"]
        );
        assert_eq!(
            merge_macs(&known, &[], Some("00:00:5e:00:53:01")),
            ["00:00:5e:00:53:01"]
        );
        assert_eq!(merge_macs(&known, &[], None), ["00:00:5e:00:53:01"]);
    }

    #[test]
    fn invalid_mac_is_rejected_before_sending() {
        let macs = vec!["00:00:5e:00:53:01".to_string(), "nesmysl".to_string()];
        let err = wake(&macs, "192.0.2.25").unwrap_err();
        assert!(err.to_string().contains("nesmysl"));
    }
}
