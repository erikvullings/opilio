//! Core configuration domain types.

use std::{fmt, net::SocketAddrV4, str::FromStr, time::Duration};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

#[derive(Clone, PartialEq, Eq)]
pub struct SecretRef {
    environment_variable: String,
}

impl SecretRef {
    pub fn environment_variable(&self) -> &str {
        &self.environment_variable
    }

    pub fn resolve_with(
        &self,
        lookup: impl FnOnce(&str) -> Option<String>,
    ) -> Result<SecretValue, SecretError> {
        lookup(&self.environment_variable)
            .map(SecretValue)
            .ok_or_else(|| SecretError::Missing(self.environment_variable.clone()))
    }
}

impl FromStr for SecretRef {
    type Err = SecretError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let name = value
            .strip_prefix("${env:")
            .and_then(|value| value.strip_suffix('}'))
            .filter(|name| {
                !name.is_empty()
                    && name.chars().enumerate().all(|(index, character)| {
                        character == '_'
                            || character.is_ascii_uppercase()
                            || (index > 0 && character.is_ascii_digit())
                    })
            })
            .ok_or_else(|| SecretError::InvalidReference(value.to_owned()))?;
        Ok(Self {
            environment_variable: name.to_owned(),
        })
    }
}

impl fmt::Debug for SecretRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "${{env:{}}}", self.environment_variable)
    }
}

impl fmt::Display for SecretRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, formatter)
    }
}

impl Serialize for SecretRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for SecretRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(de::Error::custom)
    }
}

pub struct SecretValue(String);

impl SecretValue {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

impl fmt::Display for SecretValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SecretError {
    #[error("invalid secret reference `{0}`; expected `${{env:NAME}}`")]
    InvalidReference(String),
    #[error("environment variable `{0}` referenced by a secret is not set")]
    Missing(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HumanDuration(pub Duration);

impl<'de> Deserialize<'de> for HumanDuration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        humantime::parse_duration(&value)
            .map(Self)
            .map_err(de::Error::custom)
    }
}

impl Serialize for HumanDuration {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&humantime::format_duration(self.0).to_string())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Site {
    pub label: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    #[serde(default)]
    pub devices: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Device {
    pub label: Option<String>,
    pub site: Option<String>,
    pub ssh: String,
    /// Explicit remote shell for `command` actions.
    #[serde(default = "default_remote_shell")]
    pub shell: String,
    #[serde(default)]
    pub groups: Vec<String>,
    pub power: Option<PowerProvider>,
    pub shutdown: Option<Shutdown>,
    pub telemetry: Option<Telemetry>,
    #[serde(default)]
    pub services: Vec<String>,
}

fn default_remote_shell() -> String {
    "/bin/sh".to_owned()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum PowerProvider {
    Shelly {
        host: String,
        auth: Option<ShellyAuth>,
    },
    Wol {
        mac: MacAddress,
        #[serde(default)]
        broadcast: Option<SocketAddrV4>,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ShellyAuth {
    pub username: Option<String>,
    pub password: SecretRef,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MacAddress([u8; 6]);

impl FromStr for MacAddress {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let invalid = || {
            format!(
                "invalid Wake-on-LAN MAC address `{value}`: expected six two-digit hexadecimal octets for a unicast interface"
            )
        };
        let separator = value.as_bytes().get(2).copied().ok_or_else(&invalid)?;
        if !matches!(separator, b':' | b'-') {
            return Err(invalid());
        }
        let octets = value
            .split(char::from(separator))
            .map(|octet| {
                if octet.len() != 2 {
                    return Err(());
                }
                u8::from_str_radix(octet, 16).map_err(|_| ())
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|()| invalid())?;
        let bytes: [u8; 6] = octets.try_into().map_err(|_| invalid())?;
        if bytes.iter().all(|byte| *byte == 0) || bytes[0] & 1 != 0 {
            return Err(invalid());
        }
        Ok(Self(bytes))
    }
}

impl MacAddress {
    pub const fn octets(self) -> [u8; 6] {
        self.0
    }
}

impl fmt::Debug for MacAddress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl fmt::Display for MacAddress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5]
        )
    }
}

impl<'de> Deserialize<'de> for MacAddress {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(de::Error::custom)
    }
}

impl Serialize for MacAddress {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Shutdown {
    pub command: String,
    #[serde(default)]
    pub cut_power: bool,
    pub timeout: Option<HumanDuration>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "provider", rename_all = "lowercase", deny_unknown_fields)]
pub enum Telemetry {
    System,
    Nvidia,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Service {
    pub status: Option<CommandProbe>,
    pub health: Option<HttpProbe>,
    pub info: Option<HttpProbe>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CommandProbe {
    pub command: String,
    pub timeout: Option<HumanDuration>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HttpProbe {
    pub url: String,
    #[serde(default)]
    pub headers: std::collections::BTreeMap<String, SecretRef>,
    pub timeout: Option<HumanDuration>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Exec {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ActionImplementation {
    pub command: Option<String>,
    pub exec: Option<Exec>,
    pub cwd: Option<String>,
    pub timeout: Option<HumanDuration>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    pub command: Option<String>,
    pub exec: Option<Exec>,
    pub cwd: Option<String>,
    pub timeout: Option<HumanDuration>,
    #[serde(default)]
    pub overrides: std::collections::BTreeMap<String, ActionImplementation>,
}

impl Action {
    pub(crate) fn default_implementation(&self) -> ActionImplementation {
        ActionImplementation {
            command: self.command.clone(),
            exec: self.exec.clone(),
            cwd: self.cwd.clone(),
            timeout: self.timeout,
        }
    }
}

impl ActionImplementation {
    pub fn command(&self) -> Option<&str> {
        self.command.as_deref()
    }

    pub fn exec(&self) -> Option<&Exec> {
        self.exec.as_ref()
    }

    pub fn cwd(&self) -> Option<&str> {
        self.cwd.as_deref()
    }

    pub fn timeout(&self) -> Option<Duration> {
        self.timeout.map(|duration| duration.0)
    }

    pub(crate) fn with_defaults(&self, defaults: &Self) -> Self {
        Self {
            command: self.command.clone(),
            exec: self.exec.clone(),
            cwd: self.cwd.clone().or_else(|| defaults.cwd.clone()),
            timeout: self.timeout.or(defaults.timeout),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Operation {
    On,
    Off,
    Shutdown,
    Reboot,
    PowerOff,
    PowerCycle,
    Status,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Alias {
    pub operation: Operation,
    pub target: String,
    pub parallel: Option<usize>,
    #[serde(default)]
    pub wait: bool,
    #[serde(default)]
    pub force: bool,
}
