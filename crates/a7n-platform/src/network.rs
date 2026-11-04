// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

/// MAC of the interface owning the link's local address, when available.
#[must_use]
pub fn hw_addr_for(local: IpAddr) -> Option<[u8; 6]> {
    let local = match local {
        IpAddr::V6(ip) => ip.to_ipv4_mapped().map_or(local, IpAddr::V4),
        IpAddr::V4(_) => local,
    };
    netdev::get_interfaces().into_iter().find_map(|interface| {
        let owns = interface
            .ipv4
            .iter()
            .any(|ip| IpAddr::V4(ip.addr()) == local)
            || interface
                .ipv6
                .iter()
                .any(|ip| IpAddr::V6(ip.addr()) == local);
        if !owns {
            return None;
        }
        a7n_core::wol::hw_addr(&interface.mac_addr?.octets())
    })
}

/// Ask the routing table for the source address without sending a packet.
#[must_use]
pub fn local_addr_for(remote: SocketAddr) -> Option<IpAddr> {
    let bind = if remote.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let socket = UdpSocket::bind(bind).ok()?;
    socket.connect(remote).ok()?;
    Some(socket.local_addr().ok()?.ip())
}

/// Local source addresses and directed broadcasts for IPv4 LAN interfaces.
#[must_use]
pub fn ipv4_broadcasts() -> Vec<(Ipv4Addr, Ipv4Addr)> {
    netdev::get_interfaces()
        .into_iter()
        .filter(|interface| interface.is_up() && !interface.is_loopback())
        .flat_map(|interface| interface.ipv4)
        .filter(|ip| ip.prefix_len() < 31)
        .map(|ip| (ip.addr(), ip.broadcast()))
        .collect()
}
