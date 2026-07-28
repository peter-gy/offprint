use std::error::Error;
use std::net::IpAddr;

use pageknot_model::{ErrorCode, NetworkPolicy};
use url::Url;

use super::{AddressClass, NetworkGuard};

type TestResult = Result<(), Box<dyn Error>>;

#[test]
fn ipv4_special_use_registry_boundaries_have_expected_classes() -> TestResult {
    let cases = [
        ("0.0.0.0", "0.255.255.255", AddressClass::NonRoutable),
        ("0.0.0.0", "0.0.0.0", AddressClass::NonRoutable),
        ("10.0.0.0", "10.255.255.255", AddressClass::Private),
        ("100.64.0.0", "100.127.255.255", AddressClass::Private),
        ("127.0.0.0", "127.255.255.255", AddressClass::Loopback),
        ("169.254.0.0", "169.254.255.255", AddressClass::LinkLocal),
        ("172.16.0.0", "172.31.255.255", AddressClass::Private),
        ("192.0.0.0", "192.0.0.255", AddressClass::NonRoutable),
        ("192.0.0.0", "192.0.0.7", AddressClass::NonRoutable),
        ("192.0.0.8", "192.0.0.8", AddressClass::NonRoutable),
        ("192.0.0.9", "192.0.0.9", AddressClass::Public),
        ("192.0.0.10", "192.0.0.10", AddressClass::Public),
        ("192.0.0.170", "192.0.0.171", AddressClass::NonRoutable),
        ("192.0.2.0", "192.0.2.255", AddressClass::NonRoutable),
        ("192.31.196.0", "192.31.196.255", AddressClass::Public),
        ("192.52.193.0", "192.52.193.255", AddressClass::Public),
        ("192.88.99.0", "192.88.99.255", AddressClass::NonRoutable),
        ("192.88.99.2", "192.88.99.2", AddressClass::NonRoutable),
        ("192.168.0.0", "192.168.255.255", AddressClass::Private),
        ("192.175.48.0", "192.175.48.255", AddressClass::Public),
        ("198.18.0.0", "198.19.255.255", AddressClass::NonRoutable),
        ("198.51.100.0", "198.51.100.255", AddressClass::NonRoutable),
        ("203.0.113.0", "203.0.113.255", AddressClass::NonRoutable),
        ("240.0.0.0", "255.255.255.254", AddressClass::NonRoutable),
        (
            "255.255.255.255",
            "255.255.255.255",
            AddressClass::NonRoutable,
        ),
    ];

    assert_address_range_classes(&cases)
}

#[test]
fn ipv4_blocked_prefixes_stop_at_their_registered_boundaries() -> TestResult {
    let public_addresses = [
        "1.0.0.0",
        "9.255.255.255",
        "11.0.0.0",
        "100.63.255.255",
        "100.128.0.0",
        "126.255.255.255",
        "128.0.0.0",
        "169.253.255.255",
        "169.255.0.0",
        "172.15.255.255",
        "172.32.0.0",
        "191.255.255.255",
        "192.0.1.0",
        "192.0.1.255",
        "192.0.3.0",
        "192.88.98.255",
        "192.88.100.0",
        "192.167.255.255",
        "192.169.0.0",
        "198.17.255.255",
        "198.20.0.0",
        "198.51.99.255",
        "198.51.101.0",
        "203.0.112.255",
        "203.0.114.0",
        "223.255.255.255",
    ];

    assert_address_classes(&public_addresses, AddressClass::Public)
}

#[test]
fn ipv4_multicast_is_not_a_public_destination() -> TestResult {
    assert_address_range_classes(&[("224.0.0.0", "239.255.255.255", AddressClass::NonRoutable)])
}

#[test]
fn ipv6_special_use_registry_boundaries_have_expected_classes() -> TestResult {
    let cases = [
        ("::1", "::1", AddressClass::Loopback),
        ("::", "::", AddressClass::NonRoutable),
        (
            "::ffff:0.0.0.0",
            "::ffff:255.255.255.255",
            AddressClass::NonRoutable,
        ),
        ("64:ff9b::", "64:ff9b::ffff:ffff", AddressClass::Public),
        (
            "64:ff9b:1::",
            "64:ff9b:1:ffff:ffff:ffff:ffff:ffff",
            AddressClass::NonRoutable,
        ),
        (
            "100::",
            "100::ffff:ffff:ffff:ffff",
            AddressClass::NonRoutable,
        ),
        (
            "100:0:0:1::",
            "100:0:0:1:ffff:ffff:ffff:ffff",
            AddressClass::NonRoutable,
        ),
        (
            "2001::",
            "2001:1ff:ffff:ffff:ffff:ffff:ffff:ffff",
            AddressClass::NonRoutable,
        ),
        (
            "2001::",
            "2001::ffff:ffff:ffff:ffff",
            AddressClass::NonRoutable,
        ),
        ("2001:1::1", "2001:1::1", AddressClass::Public),
        ("2001:1::2", "2001:1::2", AddressClass::Public),
        ("2001:1::3", "2001:1::3", AddressClass::Public),
        (
            "2001:2::",
            "2001:2:ffff:ffff:ffff:ffff:ffff:ffff",
            AddressClass::NonRoutable,
        ),
        (
            "2001:3::",
            "2001:3:ffff:ffff:ffff:ffff:ffff:ffff",
            AddressClass::Public,
        ),
        (
            "2001:4:112::",
            "2001:4:112:ffff:ffff:ffff:ffff:ffff",
            AddressClass::Public,
        ),
        (
            "2001:10::",
            "2001:1f:ffff:ffff:ffff:ffff:ffff:ffff",
            AddressClass::NonRoutable,
        ),
        (
            "2001:20::",
            "2001:2f:ffff:ffff:ffff:ffff:ffff:ffff",
            AddressClass::Public,
        ),
        (
            "2001:30::",
            "2001:3f:ffff:ffff:ffff:ffff:ffff:ffff",
            AddressClass::Public,
        ),
        (
            "2001:db8::",
            "2001:db8:ffff:ffff:ffff:ffff:ffff:ffff",
            AddressClass::NonRoutable,
        ),
        (
            "2002::",
            "2002:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
            AddressClass::NonRoutable,
        ),
        (
            "2620:4f:8000::",
            "2620:4f:8000:ffff:ffff:ffff:ffff:ffff",
            AddressClass::Public,
        ),
        (
            "3fff::",
            "3fff:fff:ffff:ffff:ffff:ffff:ffff:ffff",
            AddressClass::NonRoutable,
        ),
        (
            "5f00::",
            "5f00:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
            AddressClass::NonRoutable,
        ),
        (
            "fc00::",
            "fdff:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
            AddressClass::Private,
        ),
        (
            "fe80::",
            "febf:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
            AddressClass::LinkLocal,
        ),
    ];

    assert_address_range_classes(&cases)
}

#[test]
fn ipv6_prefix_exceptions_stop_at_their_registered_boundaries() -> TestResult {
    let cases = [
        (
            "64:ff9a:ffff:ffff:ffff:ffff:ffff:ffff",
            AddressClass::NonRoutable,
        ),
        ("64:ff9b:0:0:0:1::", AddressClass::NonRoutable),
        (
            "2000:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
            AddressClass::Public,
        ),
        ("2001:200::", AddressClass::Public),
        ("2001:1::", AddressClass::NonRoutable),
        ("2001:1::4", AddressClass::NonRoutable),
        (
            "2001:2:ffff:ffff:ffff:ffff:ffff:ffff",
            AddressClass::NonRoutable,
        ),
        ("2001:4::", AddressClass::NonRoutable),
        (
            "2001:4:111:ffff:ffff:ffff:ffff:ffff",
            AddressClass::NonRoutable,
        ),
        ("2001:4:113::", AddressClass::NonRoutable),
        (
            "2001:1f:ffff:ffff:ffff:ffff:ffff:ffff",
            AddressClass::NonRoutable,
        ),
        ("2001:40::", AddressClass::NonRoutable),
        (
            "2001:db7:ffff:ffff:ffff:ffff:ffff:ffff",
            AddressClass::Public,
        ),
        ("2001:db9::", AddressClass::Public),
        (
            "2001:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
            AddressClass::Public,
        ),
        ("2003::", AddressClass::Public),
        (
            "3ffe:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
            AddressClass::Public,
        ),
        ("4000::", AddressClass::NonRoutable),
    ];

    for (address, expected) in cases {
        assert_eq!(
            AddressClass::classify(address.parse()?),
            expected,
            "{address}"
        );
    }
    Ok(())
}

#[test]
fn ipv6_multicast_and_unassigned_space_are_not_public_destinations() -> TestResult {
    let addresses = [
        "4000::1",
        "7fff:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
        "8000::1",
        "ff00::",
        "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
    ];

    assert_address_classes(&addresses, AddressClass::NonRoutable)
}

#[test]
fn ipv4_mapped_ipv6_addresses_never_inherit_public_classification() -> TestResult {
    let addresses = [
        "::ffff:0.0.0.0",
        "::ffff:10.0.0.1",
        "::ffff:127.0.0.1",
        "::ffff:169.254.169.254",
        "::ffff:192.0.0.9",
        "::ffff:192.168.1.1",
        "::ffff:1.1.1.1",
        "::ffff:255.255.255.255",
    ];

    assert_address_classes(&addresses, AddressClass::NonRoutable)
}

#[test]
fn standard_and_server_policies_accept_public_dns_answers() -> TestResult {
    let url = Url::parse("https://example.test/")?;
    let addresses = ["1.1.1.1", "2001:4860:4860::8888"];

    for policy in [NetworkPolicy::Standard, NetworkPolicy::Server] {
        let guard = NetworkGuard::new(policy, &url)?;
        for address in addresses {
            assert!(
                guard
                    .validate_resolved(&url, [address.parse::<IpAddr>()?])
                    .is_ok(),
                "{address}"
            );
        }
    }
    Ok(())
}

#[test]
fn standard_and_server_policies_reject_non_global_dns_answers() -> TestResult {
    let url = Url::parse("https://example.test/")?;
    let addresses = [
        "0.0.0.1",
        "10.0.0.1",
        "100.64.0.1",
        "127.0.0.1",
        "169.254.169.254",
        "192.0.0.8",
        "192.88.99.1",
        "198.18.0.1",
        "224.0.0.1",
        "240.0.0.1",
        "::ffff:1.1.1.1",
        "64:ff9b:1::1",
        "100::1",
        "100:0:0:1::1",
        "2001::1",
        "2001:2::1",
        "2001:db8::1",
        "2002::1",
        "3fff::1",
        "5f00::1",
        "fc00::1",
        "fe80::1",
        "ff02::1",
    ];

    for policy in [NetworkPolicy::Standard, NetworkPolicy::Server] {
        let guard = NetworkGuard::new(policy, &url)?;
        for address in addresses {
            let error = guard
                .validate_resolved(&url, [address.parse::<IpAddr>()?])
                .err()
                .ok_or("non-global address passed the network policy")?;
            assert_eq!(
                error.code,
                ErrorCode::from_static("pageknot.navigation.address_blocked"),
                "{address}"
            );
        }
    }
    Ok(())
}

#[test]
fn standard_policy_keeps_loopback_bound_to_the_initial_host() -> TestResult {
    let initial = Url::parse("http://127.0.0.1:8000/")?;
    let same_host = Url::parse("http://127.0.0.1:9000/")?;
    let other_host = Url::parse("http://127.0.0.2:8000/")?;
    let guard = NetworkGuard::new(NetworkPolicy::Standard, &initial)?;

    assert!(
        guard
            .validate_resolved(&same_host, ["127.0.0.1".parse::<IpAddr>()?])
            .is_ok()
    );
    assert_eq!(
        guard
            .validate_resolved(&other_host, ["127.0.0.2".parse::<IpAddr>()?])
            .err()
            .map(|error| error.code),
        Some(ErrorCode::from_static(
            "pageknot.navigation.address_blocked"
        ))
    );
    Ok(())
}

#[test]
fn standard_policy_preserves_localhost_dns_seed_behavior() -> TestResult {
    let initial = Url::parse("http://localhost:8000/")?;
    let same_host = Url::parse("http://LOCALHOST:9000/")?;
    let guard = NetworkGuard::new(NetworkPolicy::Standard, &initial)?;

    assert!(
        guard
            .validate_resolved(
                &same_host,
                ["127.0.0.1".parse::<IpAddr>()?, "::1".parse::<IpAddr>()?]
            )
            .is_ok()
    );
    Ok(())
}

#[test]
fn standard_policy_preserves_ipv6_loopback_seed_behavior() -> TestResult {
    let initial = Url::parse("http://[::1]:8000/")?;
    let same_host = Url::parse("http://[::1]:9000/")?;
    let guard = NetworkGuard::new(NetworkPolicy::Standard, &initial)?;

    assert!(
        guard
            .validate_resolved(&same_host, ["::1".parse::<IpAddr>()?])
            .is_ok()
    );
    Ok(())
}

fn assert_address_range_classes(cases: &[(&str, &str, AddressClass)]) -> TestResult {
    for &(first, last, expected) in cases {
        for address in [first, last] {
            assert_eq!(
                AddressClass::classify(address.parse()?),
                expected,
                "{address}"
            );
        }
    }
    Ok(())
}

fn assert_address_classes(addresses: &[&str], expected: AddressClass) -> TestResult {
    for &address in addresses {
        assert_eq!(
            AddressClass::classify(address.parse()?),
            expected,
            "{address}"
        );
    }
    Ok(())
}
