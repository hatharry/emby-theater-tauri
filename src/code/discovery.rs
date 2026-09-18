//! LAN server discovery (UDP broadcast) and Wake-on-LAN magic packets.

use std::collections::HashSet;
use std::net::UdpSocket;
use std::time::{Duration, Instant};

/// LAN server discovery: UDP broadcast "who is EmbyServer?" on the standard
/// Emby discovery port and collect the JSON replies ({Address, Id, Name}).
pub(crate) fn discover_servers_sync() -> Vec<serde_json::Value> {
    const MSG: &[u8] = b"who is EmbyServer?";
    let sock = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let _ = sock.set_broadcast(true);
    let _ = sock.set_read_timeout(Some(Duration::from_millis(200)));

    for port in [7359u16] {
        let _ = sock.send_to(MSG, format!("255.255.255.255:{port}"));
        if let Ok(addrs) = local_subnet_broadcasts() {
            for addr in addrs {
                let _ = sock.send_to(MSG, (addr, port));
            }
        }
    }

    let mut servers = Vec::new();
    let mut seen = HashSet::new();
    let deadline = Instant::now() + Duration::from_millis(1500);
    let mut buf = [0u8; 4096];
    while Instant::now() < deadline {
        match sock.recv_from(&mut buf) {
            Ok((len, peer)) => {
                if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&buf[..len]) {
                    let id = v
                        .get("Id")
                        .and_then(|i| i.as_str())
                        .unwrap_or("")
                        .to_string();
                    if !id.is_empty() && seen.insert(id) {
                        let mut server = v;
                        if let Some(obj) = server.as_object_mut() {
                            obj.insert(
                                "EndpointAddress".to_string(),
                                serde_json::Value::String(peer.ip().to_string()),
                            );
                        }
                        servers.push(server);
                    }
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock
            || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => break,
        }
    }
    servers
}

/// Directed broadcast addresses for our interfaces (255.255.255.255 is often
/// dropped on multi-interface or strict-routing hosts).
fn local_subnet_broadcasts() -> Result<Vec<std::net::IpAddr>, std::io::Error> {
    let mut out = Vec::new();
    let hostname = hostname::get()?;
    for addr in std::net::ToSocketAddrs::to_socket_addrs(&hostname.to_string_lossy() as &str)? {
        if let std::net::IpAddr::V4(v4) = addr.ip() {
            let o = v4.octets();
            out.push(std::net::IpAddr::V4(std::net::Ipv4Addr::new(
                o[0], o[1], o[2], 255,
            )));
        }
    }
    Ok(out)
}

/// Parse a MAC address in any common notation into 6 bytes.
pub(crate) fn parse_mac(s: &str) -> Option<[u8; 6]> {
    let hex: String = s
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .flat_map(|c| c.to_lowercase())
        .collect();
    if hex.len() != 12 {
        return None;
    }
    let mut mac = [0u8; 6];
    for (i, b) in mac.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(mac)
}

/// Build and broadcast a Wake-on-LAN magic packet: 6x 0xFF followed by the
/// target MAC repeated 16 times, sent to the limited broadcast address, our
/// subnet-directed broadcasts, and (best-effort) unicast to the server host.
pub(crate) fn wake_on_lan_sync(mac: &str, address: Option<String>, port: Option<u16>) -> bool {
    let Some(mac) = parse_mac(mac) else {
        return false;
    };
    let mut packet = vec![0xFFu8; 6];
    for _ in 0..16 {
        packet.extend_from_slice(&mac);
    }
    let port = port.unwrap_or(9);

    let sock = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(_) => return false,
    };
    let _ = sock.set_broadcast(true);

    let mut sent = false;
    if sock.send_to(&packet, format!("255.255.255.255:{port}")).is_ok() {
        sent = true;
    }
    if let Ok(addrs) = local_subnet_broadcasts() {
        for addr in addrs {
            if sock.send_to(&packet, (addr, port)).is_ok() {
                sent = true;
            }
        }
    }
    // Many NICs also accept a WoL frame unicasted to their own address, and
    // this survives networks that filter broadcasts.
    if let Some(address) = address {
        let host = address
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .split(['/', ':'])
            .next()
            .unwrap_or(&address)
            .to_string();
        if sock.send_to(&packet, format!("{host}:{port}")).is_ok() {
            sent = true;
        }
    }
    sent
}
