//! Local IPv4 addresses the board can reach. Loopback, link-local, and the VPN and benchmark
//! ranges (198.18.0.0/15, 100.64.0.0/10) are excluded.
use std::collections::HashSet;
use std::net::Ipv4Addr;

/// LAN addresses, with private `192.168.x.x` and `10.x.x.x` first.
pub fn lan_ipv4() -> Vec<Ipv4Addr> {
    let Ok(interfaces) = if_addrs::get_if_addrs() else {
        return Vec::new();
    };
    let found = interfaces
        .into_iter()
        .filter_map(|interface| match interface.addr {
            if_addrs::IfAddr::V4(v4) => Some(v4.ip),
            _ => None,
        });
    order(found.filter(|ip| is_lan(*ip)).collect())
}

fn order(addresses: Vec<Ipv4Addr>) -> Vec<Ipv4Addr> {
    let mut seen = HashSet::new();
    let mut unique: Vec<Ipv4Addr> = addresses
        .into_iter()
        .filter(|ip| seen.insert(*ip))
        .collect();
    unique.sort_by_key(|ip| !is_preferred(*ip));
    unique
}

pub fn is_lan(ip: Ipv4Addr) -> bool {
    !(ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || in_range(ip, [198, 18, 0, 0], 15)
        || in_range(ip, [100, 64, 0, 0], 10))
}

fn is_preferred(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    (a, b) == (192, 168) || a == 10
}

fn in_range(ip: Ipv4Addr, base: [u8; 4], prefix: u32) -> bool {
    let mask = u32::MAX << (32 - prefix);
    (u32::from(ip) & mask) == (u32::from(Ipv4Addr::from(base)) & mask)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> Ipv4Addr {
        text.parse().unwrap()
    }

    #[test]
    fn excludes_loopback_link_local_and_vpn_ranges() {
        for excluded in [
            "127.0.0.1",
            "169.254.10.1",
            "198.18.0.5",
            "198.19.255.1",
            "100.64.0.1",
            "100.127.9.9",
            "0.0.0.0",
        ] {
            assert!(!is_lan(ip(excluded)), "{excluded} should be excluded");
        }
    }

    #[test]
    fn keeps_lan_addresses_including_neighbours_of_excluded_ranges() {
        for included in [
            "192.168.1.20",
            "10.0.0.2",
            "172.20.1.1",
            "100.128.0.1",
            "198.20.0.1",
        ] {
            assert!(is_lan(ip(included)), "{included} should be kept");
        }
    }

    #[test]
    fn private_addresses_come_first() {
        let ordered = order(vec![
            ip("172.20.1.1"),
            ip("192.168.1.20"),
            ip("10.0.0.2"),
            ip("192.168.1.20"),
        ]);
        assert_eq!(
            ordered,
            vec![ip("192.168.1.20"), ip("10.0.0.2"), ip("172.20.1.1")]
        );
    }
}
