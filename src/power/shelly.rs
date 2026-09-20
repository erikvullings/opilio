//! Local Shelly Gen1 and RPC-generation power control.

use std::{fmt, sync::Mutex, time::Duration};

use diqwest::blocking::WithDigestAuth;
use serde::Deserialize;
use url::Url;

use crate::{
    domain::{
        Device, PowerProvider as ConfiguredPowerProvider, SecretError, SecretValue, ShellyAuth,
    },
    power::{
        ElectricalTelemetry, Metric, OutletCommand, OutletState, PowerAvailability,
        PowerCapabilities, PowerError, PowerProvider, PowerStatus,
    },
};

/// HTTP transport error kept independent from a particular client implementation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HttpError {
    #[error("request timed out")]
    Timeout,
    #[error("{0}")]
    Transport(String),
}

/// Resolved credentials. Formatting is always redacted.
pub struct ShellyCredentials {
    username: String,
    password: SecretValue,
}

impl ShellyCredentials {
    pub fn resolve(
        auth: &ShellyAuth,
        lookup: impl FnOnce(&str) -> Option<String>,
    ) -> Result<Self, SecretError> {
        Ok(Self {
            username: auth.username.clone().unwrap_or_else(|| "admin".to_owned()),
            password: auth.password.resolve_with(lookup)?,
        })
    }

    pub fn from_environment(auth: &ShellyAuth) -> Result<Self, SecretError> {
        Self::resolve(auth, |name| std::env::var(name).ok())
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn password(&self) -> &str {
        self.password.expose()
    }
}

impl fmt::Debug for ShellyCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ShellyCredentials")
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .finish()
    }
}

/// An HTTP request issued by a Shelly provider.
pub struct HttpRequest<'a> {
    url: String,
    timeout: Duration,
    credentials: Option<&'a ShellyCredentials>,
}

impl<'a> HttpRequest<'a> {
    fn new(url: String, timeout: Duration, credentials: Option<&'a ShellyCredentials>) -> Self {
        Self {
            url,
            timeout,
            credentials,
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub const fn timeout(&self) -> Duration {
        self.timeout
    }

    pub const fn credentials(&self) -> Option<&ShellyCredentials> {
        self.credentials
    }
}

impl fmt::Debug for HttpRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpRequest")
            .field("url", &self.url)
            .field("timeout", &self.timeout)
            .field("credentials", &self.credentials)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

impl HttpResponse {
    pub fn new(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            body: body.into(),
        }
    }
}

/// Fakeable boundary around local HTTP.
pub trait HttpClient: Send + Sync {
    fn execute(&self, request: HttpRequest<'_>) -> Result<HttpResponse, HttpError>;
}

#[derive(Debug, Default)]
pub struct ReqwestHttpClient {
    client: reqwest::blocking::Client,
}

impl ReqwestHttpClient {
    pub fn new() -> Result<Self, HttpError> {
        reqwest::blocking::Client::builder()
            .build()
            .map(|client| Self { client })
            .map_err(|error| HttpError::Transport(error.to_string()))
    }

    fn response(response: reqwest::blocking::Response) -> Result<HttpResponse, HttpError> {
        let status = response.status().as_u16();
        response
            .text()
            .map(|body| HttpResponse { status, body })
            .map_err(map_reqwest_error)
    }
}

impl HttpClient for ReqwestHttpClient {
    fn execute(&self, request: HttpRequest<'_>) -> Result<HttpResponse, HttpError> {
        let builder = self.client.get(request.url()).timeout(request.timeout());
        let Some(credentials) = request.credentials() else {
            return builder
                .send()
                .map_err(map_reqwest_error)
                .and_then(Self::response);
        };
        let first = builder
            .try_clone()
            .ok_or_else(|| HttpError::Transport("could not clone Shelly request".to_owned()))?
            .send()
            .map_err(map_reqwest_error)?;
        if first.status() != reqwest::StatusCode::UNAUTHORIZED {
            return Self::response(first);
        }
        let challenge = first
            .headers()
            .get(reqwest::header::WWW_AUTHENTICATE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        let response = if challenge
            .get(..6)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("digest"))
        {
            builder
                .send_digest_auth((credentials.username(), credentials.password()))
                .map_err(|error| HttpError::Transport(error.to_string()))?
        } else {
            builder
                .basic_auth(credentials.username(), Some(credentials.password()))
                .send()
                .map_err(map_reqwest_error)?
        };
        Self::response(response)
    }
}

fn map_reqwest_error(error: reqwest::Error) -> HttpError {
    if error.is_timeout() {
        HttpError::Timeout
    } else {
        HttpError::Transport(error.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellyGeneration {
    Gen1,
    Gen2,
    Gen3,
    Rpc(u8),
}

impl ShellyGeneration {
    fn from_api(generation: Option<u8>) -> Result<Self, PowerError> {
        match generation {
            None | Some(1) => Ok(Self::Gen1),
            Some(2) => Ok(Self::Gen2),
            Some(3) => Ok(Self::Gen3),
            Some(generation @ 4..) => Ok(Self::Rpc(generation)),
            Some(generation) => Err(PowerError::Unsupported(format!(
                "Shelly API generation {generation} is unsupported"
            ))),
        }
    }

    const fn uses_rpc(self) -> bool {
        !matches!(self, Self::Gen1)
    }
}

pub struct ShellyProvider<C> {
    client: C,
    base_url: Url,
    credentials: Option<ShellyCredentials>,
    timeout: Duration,
    generation: Mutex<Option<ShellyGeneration>>,
}

impl<C> fmt::Debug for ShellyProvider<C> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ShellyProvider")
            .field("client", &std::any::type_name::<C>())
            .field("base_url", &self.base_url)
            .field("credentials", &self.credentials)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl<C: HttpClient> ShellyProvider<C> {
    pub fn new(
        client: C,
        host: &str,
        credentials: Option<ShellyCredentials>,
        timeout: Duration,
    ) -> Result<Self, PowerError> {
        if timeout.is_zero() {
            return Err(PowerError::Configuration(
                "Shelly request timeout must be greater than zero".to_owned(),
            ));
        }
        let base_url = parse_host(host)?;
        Ok(Self {
            client,
            base_url,
            credentials,
            timeout,
            generation: Mutex::new(None),
        })
    }

    pub const fn client(&self) -> &C {
        &self.client
    }

    pub fn generation(&self) -> Result<ShellyGeneration, PowerError> {
        if let Some(generation) = *self.generation.lock().expect("generation lock poisoned") {
            return Ok(generation);
        }
        let response: DeviceInfo = self.get_json("/shelly")?;
        let generation = ShellyGeneration::from_api(response.generation)?;
        *self.generation.lock().expect("generation lock poisoned") = Some(generation);
        Ok(generation)
    }

    fn read_status(&self) -> Result<PowerStatus, PowerError> {
        let generation = self.generation()?;
        if generation.uses_rpc() {
            let status: RpcSwitchStatus = self.get_json("/rpc/Switch.GetStatus?id=0")?;
            let output = status.output.ok_or_else(|| {
                PowerError::Unsupported(
                    "Shelly response does not expose switch output capability".to_owned(),
                )
            })?;
            Ok(reachable_status(
                if output {
                    OutletState::On
                } else {
                    OutletState::Off
                },
                ElectricalTelemetry {
                    voltage_volts: metric(status.voltage),
                    current_amperes: metric(status.current),
                    power_watts: metric(status.active_power),
                    energy_watt_hours: metric(status.active_energy.map(|energy| energy.total)),
                },
            ))
        } else {
            let status: Gen1Status = self.get_json("/status")?;
            let relay = status.relays.first().ok_or_else(|| {
                PowerError::Unsupported(
                    "Shelly response does not expose relay output capability".to_owned(),
                )
            })?;
            let telemetry = status
                .meters
                .first()
                .map(|meter| ElectricalTelemetry {
                    voltage_volts: metric(meter.voltage),
                    current_amperes: metric(meter.current),
                    power_watts: metric(meter.power),
                    // Gen1 reports the lifetime `total` counter in watt-minutes.
                    energy_watt_hours: metric(meter.total.map(|total| total / 60.0)),
                })
                .unwrap_or_else(ElectricalTelemetry::unsupported);
            Ok(reachable_status(
                if relay.is_on {
                    OutletState::On
                } else {
                    OutletState::Off
                },
                telemetry,
            ))
        }
    }

    fn get_json<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T, PowerError> {
        let url = self.url(path)?;
        let response = self
            .client
            .execute(HttpRequest::new(
                url.clone(),
                self.timeout,
                self.credentials.as_ref(),
            ))
            .map_err(|error| self.transport_error(&url, error))?;
        if !(200..300).contains(&response.status) {
            let detail = match response.status {
                401 | 403 => "authentication failed".to_owned(),
                404 => "endpoint or outlet is unsupported".to_owned(),
                status => format!("unexpected response status {status}"),
            };
            return Err(PowerError::Protocol(format!(
                "Shelly request to `{url}` returned HTTP {} ({detail})",
                response.status
            )));
        }
        serde_json::from_str(&response.body).map_err(|error| {
            PowerError::Protocol(format!(
                "Shelly response from `{url}` was invalid JSON: {error}"
            ))
        })
    }

    fn url(&self, path: &str) -> Result<String, PowerError> {
        self.base_url
            .join(path.trim_start_matches('/'))
            .map(|url| url.to_string())
            .map_err(|error| {
                PowerError::Configuration(format!("invalid Shelly API path `{path}`: {error}"))
            })
    }

    fn transport_error(&self, url: &str, error: HttpError) -> PowerError {
        match error {
            HttpError::Timeout => PowerError::Unreachable(format!(
                "Shelly request to `{url}` timed out after {}",
                humantime::format_duration(self.timeout)
            )),
            HttpError::Transport(message) => PowerError::Unreachable(format!(
                "Shelly request to `{url}` failed: {}",
                self.redact(&message)
            )),
        }
    }

    fn redact(&self, message: &str) -> String {
        self.credentials.as_ref().map_or_else(
            || message.to_owned(),
            |credentials| message.replace(credentials.password(), "[REDACTED]"),
        )
    }
}

impl ShellyProvider<ReqwestHttpClient> {
    pub fn from_device(device: &Device, timeout: Duration) -> Result<Self, PowerError> {
        Self::from_device_with_lookup(device, timeout, |name| std::env::var(name).ok())
    }

    pub fn from_device_with_lookup(
        device: &Device,
        timeout: Duration,
        lookup: impl FnOnce(&str) -> Option<String>,
    ) -> Result<Self, PowerError> {
        let Some(ConfiguredPowerProvider::Shelly { host, auth }) = &device.power else {
            return Err(PowerError::Configuration(
                "device does not configure a Shelly power provider".to_owned(),
            ));
        };
        let credentials = auth
            .as_ref()
            .map(|auth| ShellyCredentials::resolve(auth, lookup))
            .transpose()
            .map_err(|error| PowerError::Configuration(error.to_string()))?;
        let client = ReqwestHttpClient::new()
            .map_err(|error| PowerError::Configuration(format!("HTTP client error: {error}")))?;
        Self::new(client, host, credentials, timeout)
    }
}

impl<C: HttpClient> PowerProvider for ShellyProvider<C> {
    fn capabilities(&self) -> PowerCapabilities {
        PowerCapabilities {
            can_request_power_on: true,
            can_cut_physical_power: true,
        }
    }

    fn status(&self) -> PowerStatus {
        self.read_status().unwrap_or_else(|error| PowerStatus {
            availability: if matches!(error, PowerError::Unreachable(_)) {
                PowerAvailability::Unreachable
            } else {
                PowerAvailability::Reachable
            },
            outlet: OutletState::Unknown,
            telemetry: ElectricalTelemetry::unknown(),
            error: Some(error.to_string()),
        })
    }

    fn set_outlet(&self, command: OutletCommand) -> Result<OutletState, PowerError> {
        let (turn_on, resulting_state) = match command {
            OutletCommand::On => (true, OutletState::On),
            OutletCommand::Off => (false, OutletState::Off),
        };
        let generation = self.generation()?;
        if generation.uses_rpc() {
            let path = format!("/rpc/Switch.Set?id=0&on={turn_on}");
            let _: RpcSetResponse = self.get_json(&path)?;
        } else {
            let turn = if turn_on { "on" } else { "off" };
            let response: Gen1RelayStatus = self.get_json(&format!("/relay/0?turn={turn}"))?;
            if response.is_on != turn_on {
                return Err(PowerError::Protocol(format!(
                    "Shelly accepted the switch request but reported outlet {}",
                    if response.is_on { "on" } else { "off" }
                )));
            }
        }
        Ok(resulting_state)
    }
}

fn parse_host(host: &str) -> Result<Url, PowerError> {
    let candidate = if host.contains("://") {
        host.to_owned()
    } else {
        format!("http://{host}")
    };
    let mut url = Url::parse(&candidate)
        .map_err(|error| PowerError::Configuration(format!("invalid Shelly host: {error}")))?;
    if !matches!(url.scheme(), "http" | "https") || url.host().is_none() {
        return Err(PowerError::Configuration(
            "Shelly host must be an HTTP(S) host or URL".to_owned(),
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(PowerError::Configuration(
            "Shelly credentials must use an environment secret reference, not the host URL"
                .to_owned(),
        ));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(PowerError::Configuration(
            "Shelly host must not include a query or fragment".to_owned(),
        ));
    }
    url.set_path("/");
    Ok(url)
}

fn metric(value: Option<f64>) -> Metric<f64> {
    value.map_or(Metric::Unsupported, Metric::Value)
}

fn reachable_status(outlet: OutletState, telemetry: ElectricalTelemetry) -> PowerStatus {
    PowerStatus {
        availability: PowerAvailability::Reachable,
        outlet,
        telemetry,
        error: None,
    }
}

#[derive(Deserialize)]
struct DeviceInfo {
    #[serde(rename = "gen")]
    generation: Option<u8>,
}

#[derive(Deserialize)]
struct Gen1Status {
    #[serde(default)]
    relays: Vec<Gen1RelayStatus>,
    #[serde(default)]
    meters: Vec<Gen1Meter>,
}

#[derive(Deserialize)]
struct Gen1RelayStatus {
    #[serde(rename = "ison")]
    is_on: bool,
}

#[derive(Deserialize)]
struct Gen1Meter {
    voltage: Option<f64>,
    current: Option<f64>,
    power: Option<f64>,
    total: Option<f64>,
}

#[derive(Deserialize)]
struct RpcSwitchStatus {
    output: Option<bool>,
    voltage: Option<f64>,
    current: Option<f64>,
    #[serde(rename = "apower")]
    active_power: Option<f64>,
    #[serde(rename = "aenergy")]
    active_energy: Option<RpcEnergy>,
}

#[derive(Deserialize)]
struct RpcEnergy {
    total: f64,
}

#[derive(Deserialize)]
struct RpcSetResponse {
    #[allow(dead_code)]
    was_on: Option<bool>,
}
