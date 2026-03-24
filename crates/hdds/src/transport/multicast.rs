// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2025-2026 naskel.com

//! Multicast group management and interface discovery.
//!
//! Handles joining multicast groups, discovering network interfaces,
//! and configuring multicast settings for RTPS communication.
//!
//! # Interface selection
//!
//! The `HDDS_INTERFACE` environment variable forces ALL network operations
//! (transport bind, SPDP/SEDP locators, multicast joins, IP_MULTICAST_IF)
//! onto a single interface. Accepts either an interface name (`eth0`) or
//! an IPv4 address (`192.168.1.121`).
//!
//! This is the recommended way to avoid issues with Docker bridges, VPNs,
//! multi-NIC machines, and other virtual interfaces. Every mature DDS stack
//! has an equivalent: RTI `-nic`, FastDDS `<interfaceWhiteList>`,
//! CycloneDDS `NetworkInterfaceAddress`.
//!
//! Legacy env vars (`HDDS_UNICAST_IF`, `HDDS_MULTICAST_IF`) are still
//! honored as fallbacks but `HDDS_INTERFACE` takes priority over both.

use std::io;
use std::net::{Ipv4Addr, UdpSocket};
use std::sync::OnceLock;

/// Cached result of `HDDS_INTERFACE` resolution.
///
/// Resolved once at first access and cached for the process lifetime.
/// `None` means the env var is not set (use auto-detection).
/// `Some(ip)` means all network operations use this IP.
static FORCED_INTERFACE: OnceLock<Option<Ipv4Addr>> = OnceLock::new();

/// Resolve `HDDS_INTERFACE` env var to an IPv4 address.
///
/// Accepts:
/// - An IPv4 address directly: `HDDS_INTERFACE=192.168.1.121`
/// - An interface name: `HDDS_INTERFACE=eth0` (resolved via system lookup)
///
/// Returns `None` if the env var is not set.
/// Logs a warning and returns `None` if set but cannot be resolved.
pub fn resolve_forced_interface() -> Option<Ipv4Addr> {
    *FORCED_INTERFACE.get_or_init(|| {
        let val = match std::env::var("HDDS_INTERFACE") {
            Ok(v) if !v.is_empty() => v,
            _ => return None,
        };

        // Try parsing as IPv4 address first
        if let Ok(ip) = val.parse::<Ipv4Addr>() {
            log::info!(
                "[UDP] HDDS_INTERFACE={} (direct IP)",
                ip
            );
            return Some(ip);
        }

        // Try resolving as interface name
        match resolve_interface_name(&val) {
            Some(ip) => {
                log::info!(
                    "[UDP] HDDS_INTERFACE={} resolved to {}",
                    val,
                    ip
                );
                Some(ip)
            }
            None => {
                log::warn!(
                    "[UDP] HDDS_INTERFACE='{}' could not be resolved to an IPv4 address. \
                     Use an IP (192.168.1.121) or interface name (eth0). Falling back to auto-detect.",
                    val
                );
                None
            }
        }
    })
}

/// Resolve an interface name (e.g. `eth0`) to its IPv4 address.
///
/// Linux: parses `ip -4 addr show dev <name>`.
/// Fallback: uses `local_ip_address` crate.
#[cfg(target_os = "linux")]
fn resolve_interface_name(name: &str) -> Option<Ipv4Addr> {
    use std::process::Command;

    // Try `ip -4 addr show dev <name>` first
    if let Ok(output) = Command::new("ip")
        .args(["-4", "addr", "show", "dev", name])
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if let Some(inet_part) = line.trim().strip_prefix("inet ") {
                if let Some(addr_str) = inet_part.split('/').next() {
                    if let Ok(addr) = addr_str.trim().parse::<Ipv4Addr>() {
                        return Some(addr);
                    }
                }
            }
        }
    }

    // Fallback: enumerate all interfaces and match by name
    resolve_interface_name_crate(name)
}

#[cfg(not(target_os = "linux"))]
fn resolve_interface_name(name: &str) -> Option<Ipv4Addr> {
    resolve_interface_name_crate(name)
}

/// Resolve interface name via `local_ip_address` crate (portable).
fn resolve_interface_name_crate(name: &str) -> Option<Ipv4Addr> {
    use std::net::IpAddr;

    let interfaces = local_ip_address::list_afinet_netifas().ok()?;
    for (iface_name, ip) in interfaces {
        if iface_name == name {
            if let IpAddr::V4(ipv4) = ip {
                if !ipv4.is_loopback() {
                    return Some(ipv4);
                }
            }
        }
    }
    None
}

/// Join RTPS multicast groups (SPDP and SEDP) on all available interfaces.
///
/// RTPS v2.5: SPDP uses 239.255.0.1, SEDP uses 239.255.0.2 (Sec.9.6.1.4.1).
/// Following RTI Connext behavior: join on ALL non-loopback interfaces.
///
/// If `HDDS_INTERFACE` is set, joins only on that single interface.
///
/// Resilient: tracks join successes and falls back to UNSPECIFIED if all
/// per-interface joins fail (common on Windows with virtual adapters like
/// Hyper-V Default Switch, WSL, Docker Desktop).
pub fn join_multicast_group(socket: &UdpSocket) -> io::Result<Ipv4Addr> {
    let forced_ip = resolve_forced_interface();

    let multicast_groups = [
        Ipv4Addr::new(239, 255, 0, 1), // SPDP (common practice)
        Ipv4Addr::new(239, 255, 0, 2), // SEDP (spec-compliant)
    ];

    // RTI strategy: Join multicast on ALL available interfaces (not just one).
    // strace shows RTI calls IP_ADD_MEMBERSHIP multiple times for each interface.
    // With HDDS_INTERFACE, we join on exactly one interface.
    let interfaces = if let Some(ip) = forced_ip {
        log::info!(
            "[UDP] HDDS_INTERFACE: joining multicast only on forced interface {}",
            ip
        );
        vec![ip]
    } else {
        get_multicast_interfaces()?
    };
    let mut any_joined = false;
    let mut used_unspecified_fallback = false;

    if interfaces.is_empty() {
        // No interfaces found -- try UNSPECIFIED (let OS choose)
        log::debug!("[UDP] No suitable interfaces found for multicast, trying UNSPECIFIED");
        for group in &multicast_groups {
            match socket.join_multicast_v4(group, &Ipv4Addr::UNSPECIFIED) {
                Ok(()) => {
                    any_joined = true;
                    used_unspecified_fallback = true;
                    log::debug!("[UDP] join_multicast_v4({}) on UNSPECIFIED", group);
                }
                Err(e) => {
                    log::warn!(
                        "[UDP] join_multicast_v4({}) on UNSPECIFIED failed: {}",
                        group,
                        e
                    );
                }
            }
        }
    } else {
        for iface in &interfaces {
            for group in &multicast_groups {
                match socket.join_multicast_v4(group, iface) {
                    Ok(()) => {
                        any_joined = true;
                        log::debug!("[UDP] join_multicast_v4({}) on interface {}", group, iface);
                    }
                    Err(e) if e.raw_os_error() == Some(98) => {
                        // EADDRINUSE (98) Linux: already joined on same physical NIC
                        any_joined = true;
                        log::debug!(
                            "[UDP] join_multicast_v4({}) on {} - already joined, skipping",
                            group,
                            iface
                        );
                    }
                    Err(e) => {
                        // Non-fatal: skip interfaces that can't join multicast
                        // Windows 10049 (WSAEADDRNOTAVAIL): adapter doesn't support multicast
                        // Windows 10065 (WSAEHOSTUNREACH): no route to multicast group
                        log::debug!(
                            "[UDP] join_multicast_v4({}) on {} failed (non-fatal): {}",
                            group,
                            iface,
                            e
                        );
                    }
                }
            }
        }

        // Fallback: if ALL interface joins failed (e.g. Windows virtual adapters only),
        // try UNSPECIFIED to let the OS pick a working interface
        if !any_joined {
            log::warn!(
                "[UDP] All per-interface multicast joins failed, trying UNSPECIFIED fallback"
            );
            for group in &multicast_groups {
                if socket
                    .join_multicast_v4(group, &Ipv4Addr::UNSPECIFIED)
                    .is_ok()
                {
                    any_joined = true;
                    used_unspecified_fallback = true;
                    log::debug!(
                        "[UDP] join_multicast_v4({}) on UNSPECIFIED (fallback)",
                        group
                    );
                }
            }
        }
    }

    if !any_joined {
        log::warn!("[UDP] WARNING: Could not join any multicast group! Discovery may not work.");
    }

    socket.set_multicast_loop_v4(true)?;
    log::debug!("[UDP] multicast loop enabled");
    let _ = socket.set_multicast_ttl_v4(1);

    // Return forced interface or first discovered.
    // If we fell back to UNSPECIFIED (all per-interface joins failed),
    // return UNSPECIFIED explicitly -- not the first failed interface.
    Ok(forced_ip.unwrap_or_else(|| {
        if used_unspecified_fallback || interfaces.is_empty() {
            Ipv4Addr::UNSPECIFIED
        } else {
            interfaces.first().copied().unwrap_or(Ipv4Addr::UNSPECIFIED)
        }
    }))
}

/// Get all non-loopback IPv4 interfaces suitable for multicast.
///
/// Mimics RTI Connext behavior of joining multicast on multiple interfaces.
/// - Linux: parses `ip -4 addr show` output
/// - Windows/other: uses `local_ip_address` crate
///
/// Note: `HDDS_INTERFACE` override is handled in the caller (`join_multicast_group`).
/// This function only checks the legacy `HDDS_MULTICAST_IF` env var.
pub fn get_multicast_interfaces() -> io::Result<Vec<Ipv4Addr>> {
    // Try env var override first (for testing/debugging)
    // HDDS_INTERFACE is checked by caller; this is legacy fallback
    if let Ok(var) = std::env::var("HDDS_MULTICAST_IF") {
        if let Ok(addr) = var.parse::<Ipv4Addr>() {
            log::debug!("[UDP] Using HDDS_MULTICAST_IF override: {}", addr);
            return Ok(vec![addr]);
        }
    }

    get_multicast_interfaces_platform()
}

/// Linux: parse `ip -4 addr show` to discover interfaces.
/// Falls back to `local_ip_address` crate if `ip` command is unavailable (e.g. Docker).
#[cfg(target_os = "linux")]
fn get_multicast_interfaces_platform() -> io::Result<Vec<Ipv4Addr>> {
    use std::process::Command;

    let output = match Command::new("ip").args(["-4", "addr", "show"]).output() {
        Ok(o) => o,
        Err(_) => {
            log::debug!("[UDP] 'ip' command not found, using local_ip_address crate");
            return get_multicast_interfaces_crate();
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut interfaces = Vec::new();

    for line in stdout.lines() {
        if line.contains("127.0.0.1") || line.contains("host lo") {
            continue;
        }
        if let Some(inet_part) = line.trim().strip_prefix("inet ") {
            if let Some(addr_str) = inet_part.split('/').next() {
                if let Ok(addr) = addr_str.trim().parse::<Ipv4Addr>() {
                    if is_docker_bridge_ip(&addr) {
                        log::debug!(
                            "[UDP] Skipping Docker bridge {} in multicast interfaces",
                            addr
                        );
                    } else {
                        interfaces.push(addr);
                    }
                }
            }
        }
    }

    Ok(interfaces)
}

/// Windows/other: use `local_ip_address` crate for interface discovery.
#[cfg(not(target_os = "linux"))]
fn get_multicast_interfaces_platform() -> io::Result<Vec<Ipv4Addr>> {
    get_multicast_interfaces_crate()
}

/// Portable interface discovery via `local_ip_address` crate.
fn get_multicast_interfaces_crate() -> io::Result<Vec<Ipv4Addr>> {
    use std::net::IpAddr;

    let interfaces = match local_ip_address::list_afinet_netifas() {
        Ok(ifs) => ifs,
        Err(e) => {
            log::debug!("[UDP] Failed to list network interfaces: {}", e);
            return Ok(vec![]);
        }
    };

    let mut addrs = Vec::new();
    for (_name, ip) in interfaces {
        if let IpAddr::V4(ipv4) = ip {
            if ipv4.is_loopback() {
                continue;
            }
            if is_docker_bridge_ip(&ipv4) {
                log::debug!(
                    "[UDP] Skipping Docker bridge {} in multicast interfaces (portable)",
                    ipv4
                );
            } else {
                addrs.push(ipv4);
            }
        }
    }

    log::debug!(
        "[UDP] Discovered {} non-loopback interfaces (portable)",
        addrs.len()
    );
    Ok(addrs)
}

/// Get primary interface IP address (the one used for default route).
///
/// If `HDDS_INTERFACE` is set, returns that IP directly (no probing).
///
/// Otherwise, returns the IP to bind unicast sockets to, avoiding 0.0.0.0
/// binding issues on multi-interface machines (e.g., with docker0 interface).
///
/// Uses the standard UDP "connect" trick to probe the OS routing table,
/// which correctly avoids virtual adapters (Hyper-V, WSL, Docker Desktop)
/// that plague Windows. Every major DDS impl does this (RTI, FastDDS, CycloneDDS).
pub fn get_primary_interface_ip() -> io::Result<Ipv4Addr> {
    // HDDS_INTERFACE takes absolute priority -- no probing, no guessing
    if let Some(ip) = resolve_forced_interface() {
        log::debug!("[UDP] Primary IP forced by HDDS_INTERFACE: {}", ip);
        return Ok(ip);
    }

    // Step 1: UDP "connect" probe -- asks the OS which interface reaches 8.8.8.8.
    // No data is sent; this just queries the routing table.
    let probe_ip = UdpSocket::bind("0.0.0.0:0")
        .ok()
        .and_then(|sock| sock.connect("8.8.8.8:80").ok().map(|()| sock))
        .and_then(|sock| sock.local_addr().ok())
        .and_then(|addr| match addr.ip() {
            std::net::IpAddr::V4(ip) if !ip.is_unspecified() && !ip.is_loopback() => Some(ip),
            _ => None,
        });

    // Step 2: Accept probe result only if it's not a VPN/tunnel or docker bridge.
    // When CloudflareWARP or WireGuard is the default route, the probe returns
    // the tunnel IP (e.g. 172.16.0.2) which is unreachable from the LAN.
    // Docker bridges (172.17-31.x.x) are also unreachable from remote DDS peers.
    if let Some(ip) = probe_ip {
        if !is_tunnel_ip(&ip) && !is_docker_bridge_ip(&ip) {
            log::debug!("[UDP] Primary IP via routing table probe: {}", ip);
            return Ok(ip);
        }
        if is_docker_bridge_ip(&ip) {
            log::debug!(
                "[UDP] Routing probe returned {} (Docker bridge), skipping",
                ip
            );
        } else {
            log::debug!(
                "[UDP] Routing probe returned {} (POINTOPOINT tunnel), skipping",
                ip
            );
        }
    }

    // Step 3: Fallback -- first non-tunnel, non-docker interface from enumeration.
    let interfaces = get_multicast_interfaces()?;
    for &ip in &interfaces {
        if !is_tunnel_ip(&ip) && !is_docker_bridge_ip(&ip) {
            log::debug!("[UDP] Primary IP via interface enumeration: {}", ip);
            return Ok(ip);
        }
    }

    // Step 4: Accept any interface if all are tunnels (weird but possible)
    if let Some(&ip) = interfaces.first() {
        log::debug!("[UDP] Primary IP (all tunnels, using first): {}", ip);
        return Ok(ip);
    }

    log::debug!(
        "[UDP] WARNING: No suitable interface found, using UNSPECIFIED (may cause send issues!)"
    );
    Ok(Ipv4Addr::UNSPECIFIED)
}

/// Check if an IPv4 address belongs to a Docker bridge network.
///
/// Docker creates bridge networks in the 172.16.0.0/12 range (172.16-31.x.x).
/// These are virtual bridges unreachable from remote DDS peers and should not
/// be used for SPDP unicast locator announcements.
///
/// NOTE: This range also includes some legitimate corporate LANs (e.g. 172.20.x.x).
/// When `HDDS_INTERFACE` is set, this filter is bypassed entirely -- the user's
/// explicit choice always wins. This heuristic only affects auto-detection.
fn is_docker_bridge_ip(ip: &Ipv4Addr) -> bool {
    let octets = ip.octets();
    // Docker default bridge: 172.17.0.0/16
    // Docker user-defined bridges: 172.16-31.0.0/12
    octets[0] == 172 && (16..=31).contains(&octets[1])
}

/// Check if an IPv4 address belongs to a POINTOPOINT interface (VPN tunnels).
///
/// On Linux, parses `ip -4 addr show` flags to detect POINTOPOINT interfaces
/// (CloudflareWARP, WireGuard, OpenVPN tun devices). These interfaces are
/// unreachable from the LAN and should not be used for DDS multicast.
///
/// The `ip` command output is cached via `OnceLock` -- parsed once per process,
/// not once per call. Safe to call in a loop.
#[cfg(target_os = "linux")]
fn is_tunnel_ip(ip: &Ipv4Addr) -> bool {
    use std::collections::HashSet;

    static TUNNEL_IPS: OnceLock<HashSet<Ipv4Addr>> = OnceLock::new();

    let set = TUNNEL_IPS.get_or_init(|| {
        use std::process::Command;

        let output = match Command::new("ip").args(["-4", "addr", "show"]).output() {
            Ok(o) => o,
            Err(_) => return HashSet::new(),
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut tunnel_ips = HashSet::new();
        let mut iface_ptp = false;

        for line in stdout.lines() {
            if line.starts_with(|c: char| c.is_ascii_digit()) {
                iface_ptp = line.contains("POINTOPOINT");
                continue;
            }
            if !iface_ptp {
                continue;
            }
            if let Some(inet_part) = line.trim_start().strip_prefix("inet ") {
                if let Some(addr_str) = inet_part.split('/').next() {
                    if let Ok(addr) = addr_str.trim().parse::<Ipv4Addr>() {
                        tunnel_ips.insert(addr);
                    }
                }
            }
        }

        log::debug!("[UDP] Cached {} POINTOPOINT tunnel IPs", tunnel_ips.len());
        tunnel_ips
    });

    set.contains(ip)
}

#[cfg(not(target_os = "linux"))]
fn is_tunnel_ip(_ip: &Ipv4Addr) -> bool {
    false
}

/// Get locators for a given port on all non-loopback interfaces.
///
/// Used to generate unicast locator lists for SPDP/SEDP announcements.
///
/// Priority order:
/// 1. `HDDS_INTERFACE` -- forces everything onto one interface
/// 2. `HDDS_UNICAST_IF` -- legacy override (unicast locators only)
/// 3. `primary_iface` -- auto-detected primary interface
pub fn get_unicast_locators(primary_iface: Ipv4Addr, port: u16) -> Vec<std::net::SocketAddr> {
    use std::net::IpAddr;

    // 1. HDDS_INTERFACE takes absolute priority
    if let Some(forced_ip) = resolve_forced_interface() {
        let sock = std::net::SocketAddr::new(IpAddr::V4(forced_ip), port);
        log::debug!(
            "[UDP] Using HDDS_INTERFACE={} -> locator {}",
            forced_ip,
            sock
        );
        return vec![sock];
    }

    // 2. Legacy: HDDS_UNICAST_IF (kept for backward compat)
    if let Ok(addr_str) = std::env::var("HDDS_UNICAST_IF") {
        if let Ok(ipv4) = addr_str.parse::<std::net::Ipv4Addr>() {
            let sock = std::net::SocketAddr::new(IpAddr::V4(ipv4), port);
            log::debug!("[UDP] Using HDDS_UNICAST_IF={} -> locator {}", ipv4, sock);
            return vec![sock];
        }
        log::debug!(
            "[UDP] [!]  Invalid HDDS_UNICAST_IF='{}' -- falling back to auto-detect",
            addr_str
        );
    }

    // 3. v98: FIX - Use primary interface only (not all interfaces)
    // This prevents announcing localhost IP when running on remote nodes
    // which caused FastDDS to respond to wrong IP (ICMP unreachable)
    if !primary_iface.is_unspecified() {
        let addr = std::net::SocketAddr::new(IpAddr::V4(primary_iface), port);
        log::debug!("[UDP] v98: Using primary interface for locator: {}", addr);
        return vec![addr];
    }

    // Fallback: enumerate all interfaces (only if primary not available)
    log::debug!("[UDP] v98: WARNING - Primary interface not available, enumerating all interfaces");
    let interfaces = match local_ip_address::list_afinet_netifas() {
        Ok(ifs) => ifs,
        Err(e) => {
            log::debug!("[UDP] Failed to list network interfaces: {}", e);
            return vec![];
        }
    };

    let mut locators = Vec::new();

    for (name, ip) in interfaces {
        // Only use IPv4 addresses, skip loopback for interop
        if let IpAddr::V4(ipv4) = ip {
            if ipv4.is_loopback() {
                continue;
            }

            let addr = std::net::SocketAddr::new(IpAddr::V4(ipv4), port);
            log::debug!(
                "[UDP] Found unicast locator: {} (interface: {})",
                addr,
                name
            );
            locators.push(addr);
        }
    }

    if locators.is_empty() {
        log::debug!(
            "[UDP] [!]  No unicast locators found! Remote peers won't be able to send us data."
        );
    }

    locators
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_forced_interface_not_set() {
        // When HDDS_INTERFACE is not set, should return None
        // (This test depends on the env not being set in the test runner)
        // We just verify the function doesn't panic
        let _ = resolve_forced_interface();
    }

    #[test]
    fn test_is_docker_bridge_ip() {
        assert!(is_docker_bridge_ip(&Ipv4Addr::new(172, 17, 0, 1)));
        assert!(is_docker_bridge_ip(&Ipv4Addr::new(172, 19, 0, 1)));
        assert!(is_docker_bridge_ip(&Ipv4Addr::new(172, 31, 255, 255)));
        assert!(!is_docker_bridge_ip(&Ipv4Addr::new(192, 168, 1, 1)));
        assert!(!is_docker_bridge_ip(&Ipv4Addr::new(10, 0, 0, 1)));
        assert!(!is_docker_bridge_ip(&Ipv4Addr::new(172, 15, 0, 1)));
        assert!(!is_docker_bridge_ip(&Ipv4Addr::new(172, 32, 0, 1)));
    }

    #[test]
    fn test_resolve_interface_name_crate_nonexistent() {
        // Non-existent interface should return None
        assert!(resolve_interface_name_crate("nonexistent_iface_xyz").is_none());
    }

    #[test]
    fn test_get_primary_interface_ip_returns_valid() {
        // Should always return something (even if UNSPECIFIED)
        let ip = get_primary_interface_ip().expect("should not fail");
        // Just verify it doesn't crash; the actual IP depends on the machine
        let _ = ip;
    }
}
