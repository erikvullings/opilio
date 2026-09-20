use std::{
    io,
    net::{Ipv4Addr, SocketAddrV4, UdpSocket},
    sync::Mutex,
    time::Duration,
};

use opilio::{
    config::Config,
    domain::MacAddress,
    power::{
        OutletCommand, OutletState, PowerAvailability, PowerProvider,
        wol::{
            DEFAULT_WOL_DESTINATION, MAGIC_PACKET_LEN, SystemUdpSender, UdpSender, WolProvider,
            magic_packet,
        },
    },
};

#[derive(Debug, Default)]
struct FakeUdpSender {
    sends: Mutex<Vec<(Vec<u8>, SocketAddrV4)>>,
    failure: Option<io::ErrorKind>,
}

impl FakeUdpSender {
    fn failing(kind: io::ErrorKind) -> Self {
        Self {
            sends: Mutex::default(),
            failure: Some(kind),
        }
    }
}

impl UdpSender for FakeUdpSender {
    fn send(&self, packet: &[u8], destination: SocketAddrV4) -> io::Result<()> {
        if let Some(kind) = self.failure {
            return Err(io::Error::from(kind));
        }
        self.sends
            .lock()
            .unwrap()
            .push((packet.to_vec(), destination));
        Ok(())
    }
}

fn wol_config(extra: &str) -> Config {
    Config::from_yaml(&format!(
        r#"
devices:
  alpha:
    ssh: alpha
    power:
      type: wol
      mac: "00:11:22:33:44:55"
{extra}
"#
    ))
    .unwrap()
}

#[test]
fn known_mac_produces_the_standard_magic_packet() {
    let mac: MacAddress = "00:11:22:33:44:55".parse().unwrap();

    let packet = magic_packet(mac);

    assert_eq!(packet.len(), MAGIC_PACKET_LEN);
    assert_eq!(&packet[..6], &[0xff; 6]);
    for repetition in 0..16 {
        let start = 6 + repetition * 6;
        assert_eq!(
            &packet[start..start + 6],
            &[0x00, 0x11, 0x22, 0x33, 0x44, 0x55]
        );
    }
}

#[test]
fn mac_parser_accepts_colon_and_hyphen_notation_and_canonicalizes_output() {
    let colon: MacAddress = "A0:b1:C2:d3:E4:f5".parse().unwrap();
    let hyphen: MacAddress = "a0-b1-c2-d3-e4-f5".parse().unwrap();

    assert_eq!(colon, hyphen);
    assert_eq!(colon.octets(), [0xa0, 0xb1, 0xc2, 0xd3, 0xe4, 0xf5]);
    assert_eq!(colon.to_string(), "a0:b1:c2:d3:e4:f5");
}

#[test]
fn malformed_or_non_unicast_mac_addresses_are_rejected_precisely() {
    for value in [
        "",
        "00:11:22:33:44",
        "00:11:22:33:44:555",
        "0:11:22:33:44:55",
        "00-11:22-33:44-55",
        "gg:11:22:33:44:55",
        "00:00:00:00:00:00",
        "01:11:22:33:44:55",
        "ff:ff:ff:ff:ff:ff",
    ] {
        assert_eq!(
            value.parse::<MacAddress>().unwrap_err(),
            format!(
                "invalid Wake-on-LAN MAC address `{value}`: expected six two-digit hexadecimal octets for a unicast interface"
            )
        );
    }
}

#[test]
fn invalid_mac_and_broadcast_configuration_fail_statically() {
    let invalid_mac = Config::from_yaml(
        r#"
devices:
  alpha:
    ssh: alpha
    power:
      type: wol
      mac: "not-a-mac"
"#,
    )
    .unwrap_err();
    assert!(invalid_mac.to_string().contains("Wake-on-LAN MAC address"));

    let invalid_destination = Config::from_yaml(
        r#"
devices:
  alpha:
    ssh: alpha
    power:
      type: wol
      mac: "00:11:22:33:44:55"
      broadcast: "not-an-address"
"#,
    )
    .unwrap_err();
    assert!(
        invalid_destination
            .to_string()
            .contains("IPv4 socket address")
    );

    let zero_port = Config::from_yaml(
        r#"
devices:
  alpha:
    ssh: alpha
    power:
      type: wol
      mac: "00:11:22:33:44:55"
      broadcast: "192.168.1.255:0"
"#,
    )
    .unwrap_err();
    assert_eq!(
        zero_port.to_string(),
        "invalid configuration: device `alpha` has Wake-on-LAN broadcast destination `192.168.1.255:0` with invalid port 0"
    );
}

#[test]
fn configured_provider_uses_default_or_explicit_broadcast_destination() {
    let default = wol_config("");
    let configured = wol_config(r#"      broadcast: "192.168.50.255:7""#);

    let default_provider =
        WolProvider::from_device_with_sender(&default.devices()["alpha"], FakeUdpSender::default())
            .unwrap();
    let configured_provider = WolProvider::from_device_with_sender(
        &configured.devices()["alpha"],
        FakeUdpSender::default(),
    )
    .unwrap();
    let production_provider = WolProvider::from_device(&default.devices()["alpha"]).unwrap();

    assert_eq!(default_provider.destination(), DEFAULT_WOL_DESTINATION);
    assert_eq!(production_provider.destination(), DEFAULT_WOL_DESTINATION);
    assert_eq!(
        configured_provider.destination(),
        SocketAddrV4::new(Ipv4Addr::new(192, 168, 50, 255), 7)
    );
}

#[test]
fn wol_requests_power_on_through_the_shared_provider_interface() {
    let config = wol_config("");
    let provider =
        WolProvider::from_device_with_sender(&config.devices()["alpha"], FakeUdpSender::default())
            .unwrap();
    let provider_interface: &dyn PowerProvider = &provider;

    provider_interface.request_on().unwrap();

    let capabilities = provider_interface.capabilities();
    assert!(capabilities.can_request_power_on);
    assert!(!capabilities.can_cut_physical_power);
    assert_eq!(
        provider_interface.status().availability,
        PowerAvailability::Unknown
    );
    assert_eq!(provider_interface.status().outlet, OutletState::Unknown);
    let sends = provider.sender().sends.lock().unwrap();
    assert_eq!(sends.len(), 1);
    assert_eq!(
        sends[0].0,
        magic_packet("00:11:22:33:44:55".parse().unwrap())
    );
    assert_eq!(sends[0].1, DEFAULT_WOL_DESTINATION);
}

#[test]
fn wol_never_claims_or_attempts_physical_power_off() {
    let config = wol_config("");
    let provider =
        WolProvider::from_device_with_sender(&config.devices()["alpha"], FakeUdpSender::default())
            .unwrap();

    let error = provider.set_outlet(OutletCommand::Off).unwrap_err();

    assert_eq!(
        error.to_string(),
        "Wake-on-LAN cannot cut physical power; use the configured graceful shutdown strategy"
    );
    assert!(provider.sender().sends.lock().unwrap().is_empty());
}

#[test]
fn udp_send_failures_include_the_destination_and_preserve_unknown_state() {
    let config = wol_config("");
    let provider = WolProvider::from_device_with_sender(
        &config.devices()["alpha"],
        FakeUdpSender::failing(io::ErrorKind::NetworkUnreachable),
    )
    .unwrap();

    let error = provider.request_on().unwrap_err();

    assert_eq!(
        error.to_string(),
        "Wake-on-LAN packet to 255.255.255.255:9 failed: network unreachable"
    );
    assert_eq!(provider.status().outlet, OutletState::Unknown);
}

#[test]
fn system_udp_sender_delivers_to_a_local_socket_without_hardware() {
    let receiver = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    receiver
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let destination = match receiver.local_addr().unwrap() {
        std::net::SocketAddr::V4(address) => address,
        std::net::SocketAddr::V6(_) => panic!("bound an IPv4 socket"),
    };
    let packet = magic_packet("00:11:22:33:44:55".parse().unwrap());

    SystemUdpSender.send(&packet, destination).unwrap();

    let mut received = [0_u8; MAGIC_PACKET_LEN];
    let (length, source) = receiver.recv_from(&mut received).unwrap();
    assert_eq!(length, MAGIC_PACKET_LEN);
    assert!(source.ip().is_loopback());
    assert_eq!(received, packet);
}
