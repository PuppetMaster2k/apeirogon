// SPDX-License-Identifier: GPL-3.0-or-later

//! LAN announcements are address hints; only the pinned TLS handshake grants trust.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};

use a7n_proto::{MDNS_ID_KEY, MDNS_SERVICE_TYPE, MDNS_VERSION_KEY, MachineId, PROTOCOL_VERSION};
use mdns_sd::{DaemonEvent, ServiceDaemon, ServiceEvent, ServiceInfo};
use tokio::sync::mpsc;

/// Bound application state retained from an unauthenticated LAN.
pub(crate) const MAX_NEARBY: usize = 64;
pub(crate) const CHANNEL_CAPACITY: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Announcement {
    pub(crate) id: MachineId,
    pub(crate) addrs: Vec<SocketAddr>,
}

#[derive(Debug)]
pub(crate) enum Event {
    Discovered(Announcement),
    Lost(MachineId),
}

pub(crate) fn parse_announcement(
    own: MachineId,
    id: Option<&str>,
    version: Option<&str>,
    addresses: impl IntoIterator<Item = IpAddr>,
    port: u16,
) -> Option<Announcement> {
    if version?.parse::<u32>().ok()? != PROTOCOL_VERSION || port == 0 {
        return None;
    }
    let id = id?.parse::<MachineId>().ok()?;
    if id == own {
        return None;
    }
    let mut addrs: Vec<_> = addresses
        .into_iter()
        .filter(|ip| usable_address(*ip))
        .map(|ip| SocketAddr::new(ip, port))
        .collect();
    addrs.sort_unstable();
    addrs.dedup();
    (!addrs.is_empty()).then_some(Announcement { id, addrs })
}

fn usable_address(ip: IpAddr) -> bool {
    if ip.is_unspecified() || ip.is_multicast() || ip.is_loopback() {
        return false;
    }
    match ip {
        IpAddr::V4(ip) => !ip.is_broadcast(),
        // SocketAddr has no scope id here, so link-local IPv6 cannot be dialed.
        IpAddr::V6(ip) => !ip.is_unicast_link_local(),
    }
}

struct Discovery(ServiceDaemon);

impl Drop for Discovery {
    fn drop(&mut self) {
        let _ = self.0.shutdown();
    }
}

pub(crate) async fn run(own: MachineId, port: u16, sender: mpsc::Sender<Event>) {
    if browse(own, port, sender).await.is_err() {
        crate::log::event!("kind=discovery_unavailable");
    }
}

fn advertisement(own: MachineId, port: u16) -> Result<ServiceInfo, mdns_sd::Error> {
    let id = own.to_string();
    let version = PROTOCOL_VERSION.to_string();
    ServiceInfo::new(
        MDNS_SERVICE_TYPE,
        &id,
        &format!("{id}.local."),
        "",
        port,
        &[
            (MDNS_ID_KEY, id.as_str()),
            (MDNS_VERSION_KEY, version.as_str()),
        ][..],
    )
    .map(ServiceInfo::enable_addr_auto)
}

async fn browse(
    own: MachineId,
    port: u16,
    sender: mpsc::Sender<Event>,
) -> Result<(), mdns_sd::Error> {
    let daemon = Discovery(ServiceDaemon::new()?);
    let monitor = daemon.0.monitor()?;
    daemon.0.register(advertisement(own, port)?)?;
    let browser = daemon.0.browse(MDNS_SERVICE_TYPE)?;
    let mut names = HashMap::new();
    loop {
        let event = tokio::select! {
            () = sender.closed() => return Ok(()),
            event = browser.recv_async() => match event {
                Ok(event) => event,
                Err(_) => return Ok(()),
            },
            event = monitor.recv_async() => {
                match event {
                    Ok(DaemonEvent::Error(_)) => crate::log::event!("kind=discovery_failed"),
                    Err(_) => return Ok(()),
                    _ => {},
                }
                continue;
            }
        };
        if let Some(event) = service_event(own, event, &mut names) {
            if sender.send(event).await.is_err() {
                return Ok(());
            }
        }
    }
}

fn service_event(
    own: MachineId,
    event: ServiceEvent,
    names: &mut HashMap<String, MachineId>,
) -> Option<Event> {
    match event {
        ServiceEvent::ServiceResolved(service) => {
            let announcement = parse_announcement(
                own,
                service.txt_properties.get_property_val_str(MDNS_ID_KEY),
                service
                    .txt_properties
                    .get_property_val_str(MDNS_VERSION_KEY),
                service.addresses.iter().map(mdns_sd::ScopedIp::to_ip_addr),
                service.port,
            )?;
            if names.len() >= MAX_NEARBY && !names.contains_key(&service.fullname) {
                return None;
            }
            names.insert(service.fullname, announcement.id);
            Some(Event::Discovered(announcement))
        }
        ServiceEvent::ServiceRemoved(_, name) => names.remove(&name).map(Event::Lost),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
