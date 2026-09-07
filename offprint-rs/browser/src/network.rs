use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

use ipnet::IpNet;
use offprint_model::{ErrorStage, NetworkPolicy, NetworkRules, OffprintError, Result};
use url::{Host, Url};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// A security classification for a resolved network address.
pub enum AddressClass {
    /// A globally routable address.
    Public,
    /// A host loopback address.
    Loopback,
    /// A private or carrier-grade NAT address.
    Private,
    /// A link-local address, including cloud metadata ranges.
    LinkLocal,
    /// An unspecified, multicast, documentation, benchmark, or reserved
    /// address.
    NonRoutable,
}

impl AddressClass {
    /// Classifies an IPv4 or IPv6 address for network-policy evaluation.
    #[must_use]
    pub fn classify(address: IpAddr) -> Self {
        match address {
            IpAddr::V4(address) => classify_v4(address),
            IpAddr::V6(address) => classify_v6(address),
        }
    }
}

#[derive(Clone, Debug)]
/// Enforces a capture network policy across URL and DNS boundaries.
pub struct NetworkGuard {
    policy: NetworkPolicy,
    initial_host: Option<String>,
    initial_loopback: bool,
    custom_networks: Vec<IpNet>,
}

impl NetworkGuard {
    /// Creates a guard bound to the initial capture URL.
    ///
    /// Standard policy permits loopback traffic only when the initial URL is
    /// loopback and later requests keep the same host.
    pub fn new(policy: NetworkPolicy, initial_url: &Url) -> Result<Self> {
        let initial_host = initial_url.host_str().map(str::to_ascii_lowercase);
        let initial_loopback = literal_address(initial_url)
            .is_some_and(|address| AddressClass::classify(address) == AddressClass::Loopback)
            || initial_host.as_deref() == Some("localhost");
        let custom_networks = match &policy {
            NetworkPolicy::Custom(rules) => parse_custom_networks(rules)?,
            NetworkPolicy::Standard | NetworkPolicy::Server | NetworkPolicy::Unrestricted => {
                Vec::new()
            }
        };
        Ok(Self {
            policy,
            initial_host,
            initial_loopback,
            custom_networks,
        })
    }

    /// Returns the policy enforced by this guard.
    #[must_use]
    pub const fn policy(&self) -> &NetworkPolicy {
        &self.policy
    }

    /// Validates URL scheme, host presence, and literal address policy.
    pub fn validate_url(&self, url: &Url) -> Result<()> {
        if !matches!(url.scheme(), "http" | "https") {
            return Err(network_error(
                "offprint.navigation.scheme",
                format!("network URL scheme `{}` is blocked", url.scheme()),
            ));
        }
        if let Some(address) = literal_address(url) {
            self.validate_address(url, address)?;
        } else if url.host_str().is_none() {
            return Err(network_error(
                "offprint.navigation.host",
                "network URL must contain a host",
            ));
        }
        Ok(())
    }

    /// Validates every address returned by DNS for `url`.
    ///
    /// An empty answer and any blocked address fail the request.
    pub fn validate_resolved(
        &self,
        url: &Url,
        addresses: impl IntoIterator<Item = IpAddr>,
    ) -> Result<()> {
        self.validate_url(url)?;
        let addresses = addresses.into_iter().collect::<Vec<_>>();
        if addresses.is_empty() {
            return Err(network_error(
                "offprint.navigation.dns",
                "network host resolved to no addresses",
            ));
        }
        for address in addresses {
            self.validate_address(url, address)?;
        }
        Ok(())
    }

    fn validate_address(&self, url: &Url, address: IpAddr) -> Result<()> {
        let class = AddressClass::classify(address);
        let allowed = match &self.policy {
            NetworkPolicy::Unrestricted => true,
            NetworkPolicy::Server => class == AddressClass::Public,
            NetworkPolicy::Standard => {
                class == AddressClass::Public
                    || (class == AddressClass::Loopback
                        && self.initial_loopback
                        && same_host(url.host_str(), self.initial_host.as_deref()))
            }
            NetworkPolicy::Custom(rules) => {
                custom_allows(rules, &self.custom_networks, url, address, class)
            }
        };
        if allowed {
            Ok(())
        } else {
            Err(network_error(
                "offprint.navigation.address_blocked",
                format!("network policy blocked address class {class:?}"),
            )
            .with_detail("address", address.to_string()))
        }
    }
}

fn same_host(candidate: Option<&str>, initial: Option<&str>) -> bool {
    candidate
        .zip(initial)
        .is_some_and(|(candidate, initial)| candidate.eq_ignore_ascii_case(initial))
}

fn custom_allows(
    rules: &NetworkRules,
    networks: &[IpNet],
    url: &Url,
    address: IpAddr,
    class: AddressClass,
) -> bool {
    let host_allowed = url.host_str().is_some_and(|host| {
        rules
            .allowed_hosts
            .iter()
            .any(|allowed| host.eq_ignore_ascii_case(allowed))
    });
    host_allowed
        || networks.iter().any(|network| network.contains(&address))
        || (rules.allow_loopback && class == AddressClass::Loopback)
        || (rules.allow_private && class == AddressClass::Private)
        || (rules.allow_link_local && class == AddressClass::LinkLocal)
        || class == AddressClass::Public
}

fn parse_custom_networks(rules: &NetworkRules) -> Result<Vec<IpNet>> {
    rules
        .allowed_cidrs
        .iter()
        .map(|value| {
            IpNet::from_str(value).map_err(|error| {
                network_error(
                    "offprint.input.network_cidr",
                    format!("invalid network CIDR `{value}`: {error}"),
                )
            })
        })
        .collect()
}

fn literal_address(url: &Url) -> Option<IpAddr> {
    match url.host()? {
        Host::Ipv4(address) => Some(IpAddr::V4(address)),
        Host::Ipv6(address) => Some(IpAddr::V6(address)),
        Host::Domain(_) => None,
    }
}

fn classify_v4(address: Ipv4Addr) -> AddressClass {
    if address.is_loopback() {
        AddressClass::Loopback
    } else if address.is_private() || is_carrier_grade_nat(address) {
        AddressClass::Private
    } else if address.is_link_local() {
        AddressClass::LinkLocal
    } else if is_globally_routable_v4(address) {
        AddressClass::Public
    } else {
        AddressClass::NonRoutable
    }
}

fn classify_v6(address: Ipv6Addr) -> AddressClass {
    if address.is_loopback() {
        AddressClass::Loopback
    } else if address.is_unique_local() {
        AddressClass::Private
    } else if address.is_unicast_link_local() {
        AddressClass::LinkLocal
    } else if is_globally_routable_v6(address) {
        AddressClass::Public
    } else {
        AddressClass::NonRoutable
    }
}

fn is_carrier_grade_nat(address: Ipv4Addr) -> bool {
    let octets = address.octets();
    octets[0] == 100 && (64..=127).contains(&octets[1])
}

fn is_benchmark_v4(address: Ipv4Addr) -> bool {
    let octets = address.octets();
    octets[0] == 198 && matches!(octets[1], 18 | 19)
}

fn is_globally_routable_v4(address: Ipv4Addr) -> bool {
    let [first, second, third, fourth] = address.octets();
    if [first, second, third] == [192, 0, 0] && matches!(fourth, 9 | 10) {
        return true;
    }

    first != 0
        && !address.is_private()
        && !is_carrier_grade_nat(address)
        && !address.is_loopback()
        && !address.is_link_local()
        && [first, second, third] != [192, 0, 0]
        && !address.is_documentation()
        && [first, second, third] != [192, 88, 99]
        && !is_benchmark_v4(address)
        && !address.is_multicast()
        && first < 240
}

fn is_globally_routable_v6(address: Ipv6Addr) -> bool {
    if in_v6_prefix(address, Ipv6Addr::new(0x64, 0xff9b, 0, 0, 0, 0, 0, 0), 96) {
        return true;
    }
    if !in_v6_prefix(address, Ipv6Addr::new(0x2000, 0, 0, 0, 0, 0, 0, 0), 3) {
        return false;
    }
    if in_v6_prefix(address, Ipv6Addr::new(0x2001, 0, 0, 0, 0, 0, 0, 0), 23) {
        return is_globally_routable_ietf_v6(address);
    }

    !in_v6_prefix(address, Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 0), 32)
        && !in_v6_prefix(address, Ipv6Addr::new(0x2002, 0, 0, 0, 0, 0, 0, 0), 16)
        && !in_v6_prefix(address, Ipv6Addr::new(0x3fff, 0, 0, 0, 0, 0, 0, 0), 20)
}

fn is_globally_routable_ietf_v6(address: Ipv6Addr) -> bool {
    address == Ipv6Addr::new(0x2001, 1, 0, 0, 0, 0, 0, 1)
        || address == Ipv6Addr::new(0x2001, 1, 0, 0, 0, 0, 0, 2)
        || address == Ipv6Addr::new(0x2001, 1, 0, 0, 0, 0, 0, 3)
        || in_v6_prefix(address, Ipv6Addr::new(0x2001, 3, 0, 0, 0, 0, 0, 0), 32)
        || in_v6_prefix(address, Ipv6Addr::new(0x2001, 4, 0x112, 0, 0, 0, 0, 0), 48)
        || in_v6_prefix(address, Ipv6Addr::new(0x2001, 0x20, 0, 0, 0, 0, 0, 0), 28)
        || in_v6_prefix(address, Ipv6Addr::new(0x2001, 0x30, 0, 0, 0, 0, 0, 0), 28)
}

fn in_v6_prefix(address: Ipv6Addr, network: Ipv6Addr, prefix_length: u32) -> bool {
    let mask = u128::MAX << (u128::BITS - prefix_length);
    u128::from(address) & mask == u128::from(network) & mask
}

fn network_error(code: &'static str, message: impl Into<String>) -> OffprintError {
    OffprintError::new(code, ErrorStage::Navigation, message)
}

#[cfg(test)]
#[path = "network/tests.rs"]
mod tests;
