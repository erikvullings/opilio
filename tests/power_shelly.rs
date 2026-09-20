use std::{collections::VecDeque, sync::Mutex, time::Duration};

use opilio::{
    config::Config,
    power::{
        ElectricalTelemetry, Metric, OutletCommand, OutletState, PowerAvailability, PowerProvider,
        shelly::{
            HttpClient, HttpError, HttpRequest, HttpResponse, ShellyCredentials, ShellyGeneration,
            ShellyProvider,
        },
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecordedRequest {
    url: String,
    timeout: Duration,
    username: Option<String>,
    password: Option<String>,
    debug: String,
}

#[derive(Debug)]
struct FakeHttpClient {
    responses: Mutex<VecDeque<Result<HttpResponse, HttpError>>>,
    requests: Mutex<Vec<RecordedRequest>>,
}

impl FakeHttpClient {
    fn new(responses: impl IntoIterator<Item = Result<HttpResponse, HttpError>>) -> Self {
        Self {
            responses: Mutex::new(responses.into_iter().collect()),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn requests(&self) -> Vec<RecordedRequest> {
        self.requests.lock().unwrap().clone()
    }
}

impl HttpClient for FakeHttpClient {
    fn execute(&self, request: HttpRequest<'_>) -> Result<HttpResponse, HttpError> {
        self.requests.lock().unwrap().push(RecordedRequest {
            url: request.url().to_owned(),
            timeout: request.timeout(),
            username: request
                .credentials()
                .map(|credentials| credentials.username().to_owned()),
            password: request
                .credentials()
                .map(|credentials| credentials.password().to_owned()),
            debug: format!("{request:?}"),
        });
        self.responses.lock().unwrap().pop_front().unwrap()
    }
}

fn ok(body: &str) -> Result<HttpResponse, HttpError> {
    Ok(HttpResponse::new(200, body))
}

#[test]
fn gen1_status_maps_outlet_and_available_electrical_telemetry() {
    let client = FakeHttpClient::new([
        ok(r#"{"type":"SHSW-PM","mac":"AA"}"#),
        ok(
            r#"{"relays":[{"ison":true}],"meters":[{"voltage":230.4,"current":0.52,"power":118.7,"total":7200.0}]}"#,
        ),
    ]);
    let provider =
        ShellyProvider::new(client, "shelly-one.local", None, Duration::from_secs(4)).unwrap();
    let provider_interface: &dyn PowerProvider = &provider;

    let status = provider_interface.status();

    assert_eq!(status.availability, PowerAvailability::Reachable);
    assert_eq!(status.outlet, OutletState::On);
    assert_eq!(
        status.telemetry,
        ElectricalTelemetry {
            voltage_volts: Metric::Value(230.4),
            current_amperes: Metric::Value(0.52),
            power_watts: Metric::Value(118.7),
            energy_watt_hours: Metric::Value(120.0),
        }
    );
    assert_eq!(status.error, None);
    assert_eq!(provider.generation().unwrap(), ShellyGeneration::Gen1);
    assert_eq!(
        provider.client().requests()[1].url,
        "http://shelly-one.local/status"
    );
}

#[test]
fn rpc_generations_switch_and_parse_status_through_the_provider_interface() {
    let client = FakeHttpClient::new([
        ok(r#"{"gen":3,"model":"S3SW-001P16EU"}"#),
        ok(r#"{"was_on":false}"#),
        ok(
            r#"{"output":true,"voltage":231.2,"current":0.49,"apower":112.5,"aenergy":{"total":456.7}}"#,
        ),
    ]);
    let provider =
        ShellyProvider::new(client, "http://192.0.2.20", None, Duration::from_secs(3)).unwrap();

    assert_eq!(
        provider.set_outlet(OutletCommand::On).unwrap(),
        OutletState::On
    );
    let status = provider.status();

    assert_eq!(status.outlet, OutletState::On);
    assert_eq!(status.telemetry.voltage_volts, Metric::Value(231.2));
    assert_eq!(status.telemetry.current_amperes, Metric::Value(0.49));
    assert_eq!(status.telemetry.power_watts, Metric::Value(112.5));
    assert_eq!(status.telemetry.energy_watt_hours, Metric::Value(456.7));
    let requests = provider.client().requests();
    assert_eq!(
        requests[1].url,
        "http://192.0.2.20/rpc/Switch.Set?id=0&on=true"
    );
    assert_eq!(
        requests[2].url,
        "http://192.0.2.20/rpc/Switch.GetStatus?id=0"
    );
}

#[test]
fn gen1_outlet_can_be_switched_off() {
    let client = FakeHttpClient::new([ok(r#"{"type":"SHSW-1"}"#), ok(r#"{"ison":false}"#)]);
    let provider =
        ShellyProvider::new(client, "shelly.local", None, Duration::from_secs(3)).unwrap();

    assert_eq!(
        provider.set_outlet(OutletCommand::Off).unwrap(),
        OutletState::Off
    );
    assert_eq!(
        provider.client().requests()[1].url,
        "http://shelly.local/relay/0?turn=off"
    );
}

#[test]
fn absent_metrics_are_explicitly_unsupported_not_fabricated() {
    let client = FakeHttpClient::new([ok(r#"{"gen":2}"#), ok(r#"{"output":false,"apower":0.0}"#)]);
    let provider =
        ShellyProvider::new(client, "shelly.local", None, Duration::from_secs(3)).unwrap();

    let status = provider.status();

    assert_eq!(status.availability, PowerAvailability::Reachable);
    assert_eq!(status.outlet, OutletState::Off);
    assert_eq!(status.telemetry.voltage_volts, Metric::Unsupported);
    assert_eq!(status.telemetry.current_amperes, Metric::Unsupported);
    assert_eq!(status.telemetry.power_watts, Metric::Value(0.0));
    assert_eq!(status.telemetry.energy_watt_hours, Metric::Unsupported);
}

#[test]
fn common_power_status_json_schema_is_stable() {
    let client = FakeHttpClient::new([ok(r#"{"gen":2}"#), ok(r#"{"output":false,"apower":0.0}"#)]);
    let provider =
        ShellyProvider::new(client, "shelly.local", None, Duration::from_secs(3)).unwrap();

    assert_eq!(
        serde_json::to_string_pretty(&provider.status()).unwrap(),
        r#"{
  "availability": "reachable",
  "outlet": "off",
  "telemetry": {
    "voltage_volts": {
      "status": "unsupported"
    },
    "current_amperes": {
      "status": "unsupported"
    },
    "power_watts": {
      "status": "value",
      "value": 0.0
    },
    "energy_watt_hours": {
      "status": "unsupported"
    }
  },
  "error": null
}"#
    );
}

#[test]
fn network_and_timeout_failures_produce_actionable_unreachable_status() {
    for (failure, expected) in [
        (
            HttpError::Timeout,
            "Shelly request to `http://shelly.local/shelly` timed out after 2s",
        ),
        (
            HttpError::Transport("connection refused".to_owned()),
            "Shelly request to `http://shelly.local/shelly` failed: connection refused",
        ),
    ] {
        let client = FakeHttpClient::new([Err(failure)]);
        let provider =
            ShellyProvider::new(client, "shelly.local", None, Duration::from_secs(2)).unwrap();

        let status = provider.status();

        assert_eq!(status.availability, PowerAvailability::Unreachable);
        assert_eq!(status.outlet, OutletState::Unknown);
        assert_eq!(status.telemetry, ElectricalTelemetry::unknown());
        assert_eq!(status.error.as_deref(), Some(expected));
    }
}

#[test]
fn unsupported_generation_and_missing_outlet_capability_are_explicit_errors() {
    let unsupported = FakeHttpClient::new([ok(r#"{"gen":0}"#)]);
    let unsupported =
        ShellyProvider::new(unsupported, "shelly.local", None, Duration::from_secs(3)).unwrap();
    let no_switch = FakeHttpClient::new([ok(r#"{"gen":2}"#), ok(r#"{"temperature":{"tC":22.1}}"#)]);
    let no_switch =
        ShellyProvider::new(no_switch, "shelly.local", None, Duration::from_secs(3)).unwrap();

    let unsupported_status = unsupported.status();
    let no_switch_status = no_switch.status();

    assert_eq!(
        unsupported_status.error.as_deref(),
        Some("Shelly API generation 0 is unsupported")
    );
    assert_eq!(
        no_switch_status.error.as_deref(),
        Some("Shelly response does not expose switch output capability")
    );
    assert_eq!(no_switch_status.availability, PowerAvailability::Reachable);
    assert_eq!(no_switch_status.outlet, OutletState::Unknown);
}

#[test]
fn environment_auth_is_resolved_but_redacted_from_debug_and_failures() {
    let config = Config::from_yaml(
        r#"
devices:
  alpha:
    ssh: alpha
    power:
      type: shelly
      host: shelly.local
      auth:
        username: admin
        password: "${env:SHELLY_PASSWORD}"
"#,
    )
    .unwrap();
    let auth = match &config.devices()["alpha"].power {
        Some(opilio::domain::PowerProvider::Shelly {
            auth: Some(auth), ..
        }) => auth,
        _ => panic!("expected Shelly auth"),
    };
    let credentials = ShellyCredentials::resolve(auth, |name| {
        (name == "SHELLY_PASSWORD").then(|| "very-secret".to_owned())
    })
    .unwrap();
    let client = FakeHttpClient::new([Err(HttpError::Transport(
        "peer rejected very-secret".to_owned(),
    ))]);
    let provider = ShellyProvider::new(
        client,
        "shelly.local",
        Some(credentials),
        Duration::from_secs(3),
    )
    .unwrap();

    let status = provider.status();
    let request = &provider.client().requests()[0];

    assert_eq!(request.username.as_deref(), Some("admin"));
    assert_eq!(request.password.as_deref(), Some("very-secret"));
    assert!(!request.debug.contains("very-secret"));
    assert!(!format!("{provider:?}").contains("very-secret"));
    let error = status.error.unwrap();
    assert!(!error.contains("very-secret"));
    assert!(error.contains("[REDACTED]"));
}

#[test]
fn configured_provider_factory_reports_the_secret_reference_not_its_value() {
    let config = Config::from_yaml(
        r#"
devices:
  alpha:
    ssh: alpha
    power:
      type: shelly
      host: shelly.local
      auth:
        password: "${env:SHELLY_PASSWORD}"
"#,
    )
    .unwrap();

    let error = ShellyProvider::from_device_with_lookup(
        &config.devices()["alpha"],
        Duration::from_secs(3),
        |_| None,
    )
    .unwrap_err();

    assert_eq!(
        error.to_string(),
        "environment variable `SHELLY_PASSWORD` referenced by a secret is not set"
    );
}

#[test]
fn switching_reports_http_failures() {
    let client = FakeHttpClient::new([
        ok(r#"{"gen":2}"#),
        Ok(HttpResponse::new(401, r#"{"error":"unauthorized"}"#)),
    ]);
    let provider =
        ShellyProvider::new(client, "shelly.local", None, Duration::from_secs(3)).unwrap();
    let error = provider.set_outlet(OutletCommand::Off).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Shelly request to `http://shelly.local/rpc/Switch.Set?id=0&on=false` returned HTTP 401 (authentication failed)"
    );
}
