//! Wake-on-LAN power control.

use std::{
    io,
    net::{Ipv4Addr, SocketAddrV4, UdpSocket},
};

use crate::{
    domain::{Device, MacAddress, PowerProvider as ConfiguredPowerProvider},
    power::{
        ElectricalTelemetry, OutletCommand, OutletState, PowerAvailability, PowerCapabilities,
        PowerError, PowerProvider, PowerStatus,
    },
};

pub const MAGIC_PACKET_LEN: usize = 102;
pub const DEFAULT_WOL_DESTINATION: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::BROADCAST, 9);

pub fn magic_packet(mac: MacAddress) -> [u8; MAGIC_PACKET_LEN] {
    let mut packet = [0_u8; MAGIC_PACKET_LEN];
    packet[..6].fill(0xff);
    for chunk in packet[6..].chunks_exact_mut(6) {
        chunk.copy_from_slice(&mac.octets());
    }
    packet
}

/// UDP boundary used by Wake-on-LAN providers.
pub trait UdpSender: Send + Sync {
    fn send(&self, packet: &[u8], destination: SocketAddrV4) -> io::Result<()>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemUdpSender;

impl UdpSender for SystemUdpSender {
    fn send(&self, packet: &[u8], destination: SocketAddrV4) -> io::Result<()> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
        socket.set_broadcast(true)?;
        let sent = socket.send_to(packet, destination)?;
        if sent != packet.len() {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                format!("sent {sent} of {} Wake-on-LAN packet bytes", packet.len()),
            ));
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct WolProvider<S> {
    sender: S,
    mac: MacAddress,
    destination: SocketAddrV4,
}

impl<S: UdpSender> WolProvider<S> {
    pub const fn new(sender: S, mac: MacAddress, destination: SocketAddrV4) -> Self {
        Self {
            sender,
            mac,
            destination,
        }
    }

    pub fn from_device_with_sender(device: &Device, sender: S) -> Result<Self, PowerError> {
        let Some(ConfiguredPowerProvider::Wol { mac, broadcast }) = device.power else {
            return Err(PowerError::Configuration(
                "device is not configured with Wake-on-LAN power".to_owned(),
            ));
        };
        Ok(Self::new(
            sender,
            mac,
            broadcast.unwrap_or(DEFAULT_WOL_DESTINATION),
        ))
    }

    pub const fn destination(&self) -> SocketAddrV4 {
        self.destination
    }

    pub const fn sender(&self) -> &S {
        &self.sender
    }

    pub fn wake(&self) -> Result<(), PowerError> {
        self.sender
            .send(&magic_packet(self.mac), self.destination)
            .map_err(|error| {
                PowerError::Unreachable(format!(
                    "Wake-on-LAN packet to {} failed: {error}",
                    self.destination
                ))
            })
    }
}

impl WolProvider<SystemUdpSender> {
    pub fn from_device(device: &Device) -> Result<Self, PowerError> {
        Self::from_device_with_sender(device, SystemUdpSender)
    }
}

impl<S: UdpSender> PowerProvider for WolProvider<S> {
    fn capabilities(&self) -> PowerCapabilities {
        PowerCapabilities {
            can_request_power_on: true,
            can_cut_physical_power: false,
        }
    }

    fn request_on(&self) -> Result<(), PowerError> {
        self.wake()
    }

    fn status(&self) -> PowerStatus {
        PowerStatus {
            availability: PowerAvailability::Unknown,
            outlet: OutletState::Unknown,
            telemetry: ElectricalTelemetry::unsupported(),
            error: None,
        }
    }

    fn set_outlet(&self, command: OutletCommand) -> Result<OutletState, PowerError> {
        match command {
            OutletCommand::On => {
                self.wake()?;
                Ok(OutletState::Unknown)
            }
            OutletCommand::Off => Err(PowerError::Unsupported(
                "Wake-on-LAN cannot cut physical power; use the configured graceful shutdown strategy"
                    .to_owned(),
            )),
        }
    }
}
