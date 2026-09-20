//! Physical power providers and shared power/telemetry DTOs.

use serde::Serialize;

pub mod shelly;
pub mod wol;

/// A physical outlet state. `Unknown` is observational and cannot be commanded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutletState {
    On,
    Off,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutletCommand {
    On,
    Off,
}

/// Whether a provider was reachable for the latest observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerAvailability {
    Reachable,
    Unreachable,
}

/// An electrical measurement, distinguishing unsupported from temporarily unknown.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "status", content = "value")]
pub enum Metric<T> {
    Value(T),
    Unsupported,
    Unknown,
}

/// Common electrical telemetry units exposed by power providers.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ElectricalTelemetry {
    pub voltage_volts: Metric<f64>,
    pub current_amperes: Metric<f64>,
    pub power_watts: Metric<f64>,
    pub energy_watt_hours: Metric<f64>,
}

impl ElectricalTelemetry {
    pub const fn unknown() -> Self {
        Self {
            voltage_volts: Metric::Unknown,
            current_amperes: Metric::Unknown,
            power_watts: Metric::Unknown,
            energy_watt_hours: Metric::Unknown,
        }
    }

    pub const fn unsupported() -> Self {
        Self {
            voltage_volts: Metric::Unsupported,
            current_amperes: Metric::Unsupported,
            power_watts: Metric::Unsupported,
            energy_watt_hours: Metric::Unsupported,
        }
    }
}

/// One provider observation suitable for CLI, TUI, and scheduler use cases.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PowerStatus {
    pub availability: PowerAvailability,
    pub outlet: OutletState,
    pub telemetry: ElectricalTelemetry,
    pub error: Option<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PowerError {
    #[error("{0}")]
    Configuration(String),
    #[error("{0}")]
    Unreachable(String),
    #[error("{0}")]
    Protocol(String),
    #[error("{0}")]
    Unsupported(String),
}

/// Shared interface for physical power implementations.
pub trait PowerProvider: Send + Sync {
    fn status(&self) -> PowerStatus;

    fn set_outlet(&self, command: OutletCommand) -> Result<OutletState, PowerError>;
}
