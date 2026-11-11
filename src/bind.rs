//! Private-only multi-address bind selectors.
//! CIDRs select the machine's currently assigned, UP interface IPs at startup,
//! and never create wildcard listening sockets.
use crate::config::validate_bind;
use anyhow::{Context, Result, bail};
use std::{
    collections::BTreeSet,
    fmt,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    str::FromStr,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindSpec {
    Address(IpAddr),
    Network(Cidr),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cidr {
    network: IpAddr,
    prefix: u8,
}

impl FromStr for BindSpec {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        if let Some((ip_text, prefix_text)) = value.split_once('/') {
            let ip: IpAddr = ip_text.parse().context("invalid CIDR address")?;
            let prefix: u8 = prefix_text.parse().context("invalid CIDR prefix")?;
            let bits = if ip.is_ipv4() { 32 } else { 128 };
            if prefix > bits {
                bail!("CIDR prefix {prefix} exceeds {bits} bits");
            }
            let network = Cidr {
                network: mask(ip, prefix),
                prefix,
            };
            if !network.is_fully_private() {
                bail!("CIDR {value} is outside lserve's local/private address policy");
            }
            Ok(Self::Network(network))
        } else {
            let ip: IpAddr = value.parse().context("invalid bind IP address")?;
            validate_bind(ip)?;
            Ok(Self::Address(ip))
        }
    }
}

impl fmt::Display for Cidr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.network, self.prefix)
    }
}

impl Cidr {
    fn is_fully_private(self) -> bool {
        match self.network {
            IpAddr::V4(_) => {
                // A requested subnet must fit entirely within ONE allowed range.
                let allowed = [
                    (Ipv4Addr::new(127, 0, 0, 0), 8),
                    (Ipv4Addr::new(10, 0, 0, 0), 8),
                    (Ipv4Addr::new(172, 16, 0, 0), 12),
                    (Ipv4Addr::new(192, 168, 0, 0), 16),
                    (Ipv4Addr::new(100, 64, 0, 0), 10),
                ];
                allowed.iter().any(|(base, prefix)| {
                    self.prefix >= *prefix
                        && mask(self.network, *prefix) == IpAddr::V4(*base)
                })
            }
            IpAddr::V6(ip) => {
                (self.prefix == 128 && ip.is_loopback())
                    || (self.prefix >= 7
                        && mask(self.network, 7)
                            == IpAddr::V6(Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 0)))
            }
        }
    }

    fn contains(self, ip: IpAddr) -> bool {
        match (self.network, ip) {
            (IpAddr::V4(_), IpAddr::V4(_)) | (IpAddr::V6(_), IpAddr::V6(_)) => {
                mask(ip, self.prefix) == self.network
            }
            _ => false,
        }
    }
}

fn mask(ip: IpAddr, prefix: u8) -> IpAddr {
    match ip {
        IpAddr::V4(ip) => {
            let bits = if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - u32::from(prefix))
            };
            IpAddr::V4(Ipv4Addr::from(u32::from(ip) & bits))
        }
        IpAddr::V6(ip) => {
            let bits = if prefix == 0 {
                0
            } else {
                u128::MAX << (128 - u32::from(prefix))
            };
            IpAddr::V6(Ipv6Addr::from(u128::from(ip) & bits))
        }
    }
}

/// Resolve CIDRs once at startup and de-duplicate overlapping selectors.
pub fn resolve_binds(specs: &[BindSpec]) -> Result<Vec<IpAddr>> {
    let local = if specs.iter().any(|spec| matches!(spec, BindSpec::Network(_))) {
        local_interface_ips()?
    } else {
        Vec::new()
    };
    resolve_with_local(specs, &local)
}

fn resolve_with_local(specs: &[BindSpec], local: &[IpAddr]) -> Result<Vec<IpAddr>> {
    let mut selected = BTreeSet::new();
    for spec in specs {
        match spec {
            BindSpec::Address(ip) => {
                validate_bind(*ip)?;
                selected.insert(*ip);
            }
            BindSpec::Network(cidr) => {
                let matches: Vec<_> = local
                    .iter()
                    .copied()
                    .filter(|ip| cidr.contains(*ip))
                    .collect();
                if matches.is_empty() {
                    bail!("CIDR {cidr} matches no currently assigned UP local address");
                }
                selected.extend(matches);
            }
        }
    }
    if selected.is_empty() {
        bail!("at least one bind address is required");
    }
    Ok(selected.into_iter().collect())
}

#[cfg(unix)]
fn local_interface_ips() -> Result<Vec<IpAddr>> {
    // getifaddrs works on Linux/musl and Android. The result is a snapshot.
    struct Guard(*mut libc::ifaddrs);
    impl Drop for Guard {
        fn drop(&mut self) {
            unsafe { libc::freeifaddrs(self.0) };
        }
    }
    let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
    if unsafe { libc::getifaddrs(&mut head) } != 0 {
        return Err(std::io::Error::last_os_error()).context("failed to list local interfaces");
    }
    let _guard = Guard(head);
    let mut ips = BTreeSet::new();
    let mut current = head;
    while !current.is_null() {
        // SAFETY: getifaddrs owns these nodes/addresses until freeifaddrs.
        let entry = unsafe { &*current };
        if !entry.ifa_addr.is_null() && (entry.ifa_flags & libc::IFF_UP as u32) != 0 {
            let family = unsafe { (*entry.ifa_addr).sa_family as i32 };
            match family {
                libc::AF_INET => {
                    let addr = unsafe { &*(entry.ifa_addr.cast::<libc::sockaddr_in>()) };
                    ips.insert(IpAddr::V4(Ipv4Addr::from(addr.sin_addr.s_addr.to_ne_bytes())));
                }
                libc::AF_INET6 => {
                    let addr = unsafe { &*(entry.ifa_addr.cast::<libc::sockaddr_in6>()) };
                    ips.insert(IpAddr::V6(Ipv6Addr::from(addr.sin6_addr.s6_addr)));
                }
                _ => {}
            }
        }
        current = entry.ifa_next;
    }
    Ok(ips.into_iter().collect())
}

#[cfg(not(unix))]
fn local_interface_ips() -> Result<Vec<IpAddr>> {
    bail!("CIDR bind requires Unix local-interface enumeration")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selector(s: &str) -> BindSpec {
        s.parse().unwrap()
    }

    #[test]
    fn accepts_wholly_private_cidrs() {
        for s in [
            "127.0.0.0/8", "10.0.0.0/8", "172.16.0.0/12",
            "192.168.0.0/16", "100.64.0.0/10", "100.95.1.4/32",
            "::1/128", "fc00::/7", "fd7a:115c:a1e0::/48",
        ] {
            assert!(s.parse::<BindSpec>().is_ok(), "{s}");
        }
    }

    #[test]
    fn rejects_public_overlaps_wildcards_invalid_prefixes() {
        for s in [
            "0.0.0.0", "0.0.0.0/0", "8.8.8.8", "8.8.8.0/24",
            "192.168.0.0/15", "100.64.0.0/9", "100.0.0.0/8",
            "172.16.0.0/11", "::", "::/0", "::1/127",
            "fe80::/10", "fc00::/6", "2001:db8::/32",
            "10.0.0.0/33", "fd00::/129", "192.168.1.0/",
        ] {
            assert!(s.parse::<BindSpec>().is_err(), "{s}");
        }
    }

    #[test]
    fn cidr_matches_only_local_addresses_and_deduplicates() {
        let specs = [
            selector("100.100.10.1/10"), selector("127.0.0.1"),
            selector("fd7a:115c:a1e0::/48"), selector("100.95.2.3"),
        ];
        let assigned = [
            "100.95.2.3".parse().unwrap(),
            "100.100.10.1".parse().unwrap(),
            "fd7a:115c:a1e0::beef".parse().unwrap(),
            "8.8.8.8".parse().unwrap(),
        ];
        let result = resolve_with_local(&specs, &assigned).unwrap();
        let unique: BTreeSet<_> = result.iter().copied().collect();
        assert_eq!(unique.len(), 4);
        assert!(unique.contains(&"127.0.0.1".parse().unwrap()));
        assert!(unique.contains(&"100.95.2.3".parse().unwrap()));
        assert!(unique.contains(&"100.100.10.1".parse().unwrap()));
        assert!(unique.contains(&"fd7a:115c:a1e0::beef".parse().unwrap()));
        assert!(!unique.contains(&"8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn unmatched_cidr_is_fatal() {
        assert!(resolve_with_local(
            &[selector("192.168.0.0/16")],
            &["127.0.0.1".parse().unwrap()],
        ).is_err());
    }

    #[test]
    fn literal_ip_does_not_need_local_enumeration() {
        assert_eq!(
            resolve_with_local(&[selector("::1"), selector("127.0.0.1")], &[])
                .unwrap().len(),
            2
        );
    }
}
