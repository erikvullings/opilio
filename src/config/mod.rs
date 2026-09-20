//! Portable configuration loading, strict validation, and action resolution.

use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::domain::{
    Action, ActionImplementation, Alias, Device, Exec, Group, HttpProbe, Operation, PowerProvider,
    Service, Site,
};

pub use crate::domain::{SecretError, SecretRef, SecretValue};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    sites: BTreeMap<String, Site>,
    #[serde(default)]
    devices: BTreeMap<String, Device>,
    #[serde(default)]
    groups: BTreeMap<String, Group>,
    #[serde(default)]
    actions: BTreeMap<String, Action>,
    #[serde(default)]
    aliases: BTreeMap<String, Alias>,
    #[serde(default)]
    services: BTreeMap<String, Service>,
}

impl Config {
    pub fn from_yaml(yaml: &str) -> Result<Self, ConfigError> {
        let config: Self = serde_yaml::from_str(yaml).map_err(ConfigError::Yaml)?;
        config.validate()?;
        Ok(config)
    }

    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let yaml = fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_owned(),
            source,
        })?;
        Self::from_yaml(&yaml)
    }

    pub fn sites(&self) -> &BTreeMap<String, Site> {
        &self.sites
    }

    pub fn devices(&self) -> &BTreeMap<String, Device> {
        &self.devices
    }

    pub fn groups(&self) -> &BTreeMap<String, Group> {
        &self.groups
    }

    pub fn actions(&self) -> &BTreeMap<String, Action> {
        &self.actions
    }

    pub fn aliases(&self) -> &BTreeMap<String, Alias> {
        &self.aliases
    }

    pub fn services(&self) -> &BTreeMap<String, Service> {
        &self.services
    }

    pub fn resolve_action(
        &self,
        action_name: &str,
        device_name: &str,
    ) -> Result<ActionImplementation, ConfigError> {
        let action = self
            .actions
            .get(action_name)
            .ok_or_else(|| validation(format!("unknown action `{action_name}`")))?;
        let device = self
            .devices
            .get(device_name)
            .ok_or_else(|| validation(format!("unknown device `{device_name}`")))?;
        let defaults = action.default_implementation();

        if let Some(implementation) = action.overrides.get(device_name) {
            return Ok(implementation.with_defaults(&defaults));
        }

        let mut group_implementations = device
            .groups
            .iter()
            .filter_map(|group| {
                action
                    .overrides
                    .get(group)
                    .map(|implementation| (group, implementation.with_defaults(&defaults)))
            })
            .collect::<Vec<_>>();
        if let Some((_, implementation)) = group_implementations.pop() {
            if group_implementations
                .iter()
                .all(|(_, candidate)| *candidate == implementation)
            {
                return Ok(implementation);
            }
            return Err(validation(format!(
                "action `{action_name}` has ambiguous group overrides for device `{device_name}`"
            )));
        }

        if defaults.command.is_some() || defaults.exec.is_some() {
            Ok(defaults)
        } else {
            Err(validation(format!(
                "action `{action_name}` has no implementation for device `{device_name}`"
            )))
        }
    }

    fn validate(&self) -> Result<(), ConfigError> {
        self.validate_names()?;
        self.validate_devices_and_membership()?;
        self.validate_services()?;
        self.validate_actions()?;
        self.validate_aliases()
    }

    fn validate_names(&self) -> Result<(), ConfigError> {
        for (kind, names) in [
            ("site", self.sites.keys().collect::<Vec<_>>()),
            ("device", self.devices.keys().collect()),
            ("group", self.groups.keys().collect()),
            ("action", self.actions.keys().collect()),
            ("alias", self.aliases.keys().collect()),
            ("service", self.services.keys().collect()),
        ] {
            for name in names {
                validate_name(kind, name)?;
            }
        }

        let mut target_names = BTreeMap::<&str, &str>::new();
        for (kind, names) in [
            ("site", self.sites.keys().collect::<Vec<_>>()),
            ("device", self.devices.keys().collect()),
            ("group", self.groups.keys().collect()),
        ] {
            for name in names {
                if name == "all" {
                    return Err(validation(format!(
                        "{kind} name `all` is reserved for the all-devices target"
                    )));
                }
                if let Some(previous) = target_names.insert(name, kind) {
                    return Err(validation(format!(
                        "target name `{name}` is used by both a {previous} and a {kind}"
                    )));
                }
            }
        }
        Ok(())
    }

    fn validate_devices_and_membership(&self) -> Result<(), ConfigError> {
        for (device_name, device) in &self.devices {
            if device.ssh.trim().is_empty() {
                return Err(validation(format!(
                    "device `{device_name}` has an empty SSH target"
                )));
            }
            if device.shell.trim().is_empty() {
                return Err(validation(format!(
                    "device `{device_name}` has an empty remote shell"
                )));
            }
            reject_duplicates(
                &device.groups,
                &format!("device `{device_name}` group membership"),
            )?;
            reject_duplicates(
                &device.services,
                &format!("device `{device_name}` services"),
            )?;
            if let Some(site) = &device.site
                && !self.sites.contains_key(site)
            {
                return Err(validation(format!(
                    "device `{device_name}` references unknown site `{site}`"
                )));
            }
            for group in &device.groups {
                let configured_group = self.groups.get(group).ok_or_else(|| {
                    validation(format!(
                        "device `{device_name}` references unknown group `{group}`"
                    ))
                })?;
                if !configured_group
                    .devices
                    .iter()
                    .any(|member| member == device_name)
                {
                    return Err(validation(format!(
                        "conflicting membership: device `{device_name}` lists group `{group}`, but group `{group}` does not list the device"
                    )));
                }
            }
            self.validate_power(device_name, device)?;
        }

        for (group_name, group) in &self.groups {
            reject_duplicates(
                &group.devices,
                &format!("group `{group_name}` device membership"),
            )?;
            for device_name in &group.devices {
                let device = self.devices.get(device_name).ok_or_else(|| {
                    validation(format!(
                        "group `{group_name}` references unknown device `{device_name}`"
                    ))
                })?;
                if !device.groups.iter().any(|member| member == group_name) {
                    return Err(validation(format!(
                        "conflicting membership: group `{group_name}` lists device `{device_name}`, but device `{device_name}` does not list the group"
                    )));
                }
            }
        }
        Ok(())
    }

    fn validate_power(&self, device_name: &str, device: &Device) -> Result<(), ConfigError> {
        match &device.power {
            Some(PowerProvider::Shelly { host, auth }) => {
                if host.trim().is_empty() {
                    return Err(validation(format!(
                        "device `{device_name}` has an empty Shelly host"
                    )));
                }
                if let Some(auth) = auth
                    && auth
                        .username
                        .as_ref()
                        .is_some_and(|username| username.trim().is_empty())
                {
                    return Err(validation(format!(
                        "device `{device_name}` has an empty Shelly auth username"
                    )));
                }
            }
            Some(PowerProvider::Wol {
                broadcast: Some(destination),
                ..
            }) if destination.port() == 0 => {
                return Err(validation(format!(
                    "device `{device_name}` has Wake-on-LAN broadcast destination `{destination}` with invalid port 0"
                )));
            }
            _ => {}
        }
        if let Some(shutdown) = &device.shutdown
            && shutdown.command.trim().is_empty()
        {
            return Err(validation(format!(
                "device `{device_name}` has an empty shutdown command"
            )));
        }
        Ok(())
    }

    fn validate_services(&self) -> Result<(), ConfigError> {
        for (device_name, device) in &self.devices {
            for service in &device.services {
                if !self.services.contains_key(service) {
                    return Err(validation(format!(
                        "device `{device_name}` references unknown service `{service}`"
                    )));
                }
            }
        }
        for (name, service) in &self.services {
            if service.status.is_none() && service.health.is_none() && service.info.is_none() {
                return Err(validation(format!(
                    "service `{name}` must configure status, health, or info"
                )));
            }
            if let Some(status) = &service.status
                && status.command.trim().is_empty()
            {
                return Err(validation(format!(
                    "service `{name}` has an empty status command"
                )));
            }
            for (probe_name, probe) in [
                ("health", service.health.as_ref()),
                ("info", service.info.as_ref()),
            ] {
                if let Some(probe) = probe {
                    validate_http_probe(name, probe_name, probe)?;
                }
            }
        }
        Ok(())
    }

    fn validate_actions(&self) -> Result<(), ConfigError> {
        for (name, action) in &self.actions {
            validate_implementation(name, "default", &action.default_implementation())?;
            for (target, implementation) in &action.overrides {
                if !self.devices.contains_key(target) && !self.groups.contains_key(target) {
                    return Err(validation(format!(
                        "action `{name}` references unknown override target `{target}`"
                    )));
                }
                validate_implementation(name, &format!("override `{target}`"), implementation)?;
            }
            for device_name in self.devices.keys() {
                self.validate_action_ambiguity(name, action, device_name)?;
            }
        }
        Ok(())
    }

    fn validate_action_ambiguity(
        &self,
        action_name: &str,
        action: &Action,
        device_name: &str,
    ) -> Result<(), ConfigError> {
        if action.overrides.contains_key(device_name) {
            return Ok(());
        }
        let device = &self.devices[device_name];
        let defaults = action.default_implementation();
        let applicable = device
            .groups
            .iter()
            .filter_map(|group| {
                action
                    .overrides
                    .get(group)
                    .map(|implementation| (group, implementation.with_defaults(&defaults)))
            })
            .collect::<Vec<_>>();
        let Some((_, first)) = applicable.first() else {
            return Ok(());
        };
        if applicable
            .iter()
            .skip(1)
            .any(|(_, implementation)| implementation != first)
        {
            let groups = applicable
                .iter()
                .map(|(group, _)| format!("`{group}`"))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(validation(format!(
                "action `{action_name}` has conflicting group overrides {groups} for device `{device_name}`; add a device override"
            )));
        }
        Ok(())
    }

    fn validate_aliases(&self) -> Result<(), ConfigError> {
        for (name, alias) in &self.aliases {
            if alias.parallel == Some(0) {
                return Err(validation(format!(
                    "alias `{name}` parallel value must be greater than zero"
                )));
            }
            if alias.target != "all"
                && !self.devices.contains_key(&alias.target)
                && !self.groups.contains_key(&alias.target)
                && !self.sites.contains_key(&alias.target)
            {
                return Err(validation(format!(
                    "alias `{name}` references unknown target `{}`",
                    alias.target
                )));
            }
            if alias.wait && alias.operation != Operation::On {
                return Err(validation(format!(
                    "alias `{name}` operation `{:?}` does not support `wait`",
                    alias.operation
                )));
            }
            if alias.force
                && !matches!(
                    alias.operation,
                    Operation::Off | Operation::PowerOff | Operation::PowerCycle
                )
            {
                return Err(validation(format!(
                    "alias `{name}` operation `{:?}` does not support `force`",
                    alias.operation
                )));
            }
        }
        Ok(())
    }
}

fn validate_name(kind: &str, name: &str) -> Result<(), ConfigError> {
    if name.is_empty()
        || !name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        return Err(validation(format!(
            "invalid {kind} name `{name}`; use ASCII letters, numbers, `.`, `_`, or `-`"
        )));
    }
    Ok(())
}

fn reject_duplicates(values: &[String], context: &str) -> Result<(), ConfigError> {
    let mut unique = BTreeSet::new();
    for value in values {
        if !unique.insert(value) {
            return Err(validation(format!("duplicate `{value}` in {context}")));
        }
    }
    Ok(())
}

fn validate_implementation(
    action_name: &str,
    context: &str,
    implementation: &ActionImplementation,
) -> Result<(), ConfigError> {
    match (&implementation.command, &implementation.exec) {
        (Some(_), Some(_)) => Err(validation(format!(
            "action `{action_name}` {context} defines both `command` and `exec`; choose exactly one"
        ))),
        (None, None) => Err(validation(format!(
            "action `{action_name}` {context} must define either `command` or `exec`"
        ))),
        (Some(command), None) if command.trim().is_empty() => Err(validation(format!(
            "action `{action_name}` {context} has an empty command"
        ))),
        (None, Some(Exec { program, .. })) if program.trim().is_empty() => Err(validation(
            format!("action `{action_name}` {context} has an empty exec program"),
        )),
        _ => Ok(()),
    }
}

fn validate_http_probe(
    service_name: &str,
    probe_name: &str,
    probe: &HttpProbe,
) -> Result<(), ConfigError> {
    let url = url::Url::parse(&probe.url).map_err(|error| {
        validation(format!(
            "service `{service_name}` {probe_name} URL is invalid: {error}"
        ))
    })?;
    if !matches!(url.scheme(), "http" | "https") || url.host().is_none() {
        return Err(validation(format!(
            "service `{service_name}` {probe_name} URL must be an absolute HTTP(S) URL"
        )));
    }
    Ok(())
}

fn validation(message: String) -> ConfigError {
    ConfigError::Validation(message)
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not read configuration `{path}`: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid YAML configuration: {0}")]
    Yaml(serde_yaml::Error),
    #[error("invalid configuration: {0}")]
    Validation(String),
    #[error("{0}")]
    Path(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Linux,
    MacOs,
    Windows,
}

impl Platform {
    pub const fn current() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Linux
        }
    }
}

pub struct ConfigPathOptions<'a> {
    explicit: Option<PathBuf>,
    platform: Platform,
    environment: &'a BTreeMap<String, String>,
}

impl<'a> ConfigPathOptions<'a> {
    pub fn new(
        explicit: Option<PathBuf>,
        platform: Platform,
        environment: &'a BTreeMap<String, String>,
    ) -> Self {
        Self {
            explicit,
            platform,
            environment,
        }
    }

    pub fn resolve(&self) -> Result<PathBuf, ConfigError> {
        if let Some(path) = &self.explicit {
            return Ok(path.clone());
        }
        if let Some(path) = self
            .environment
            .get("OPILIO_CONFIG")
            .filter(|path| !path.is_empty())
        {
            return Ok(PathBuf::from(path));
        }
        match self.platform {
            Platform::Linux => self
                .environment
                .get("XDG_CONFIG_HOME")
                .filter(|path| !path.is_empty())
                .map(PathBuf::from)
                .or_else(|| {
                    self.environment
                        .get("HOME")
                        .filter(|path| !path.is_empty())
                        .map(|home| PathBuf::from(home).join(".config"))
                })
                .map(|base| base.join("opilio").join("config.yaml"))
                .ok_or_else(|| {
                    ConfigError::Path(
                        "cannot determine config path: neither XDG_CONFIG_HOME nor HOME is set"
                            .to_owned(),
                    )
                }),
            Platform::MacOs => self
                .environment
                .get("HOME")
                .filter(|path| !path.is_empty())
                .map(|home| {
                    PathBuf::from(home)
                        .join(".config")
                        .join("opilio")
                        .join("config.yaml")
                })
                .ok_or_else(|| {
                    ConfigError::Path("cannot determine config path: HOME is not set".to_owned())
                }),
            Platform::Windows => self
                .environment
                .get("APPDATA")
                .filter(|path| !path.is_empty())
                .map(|base| PathBuf::from(base).join("opilio").join("config.yaml"))
                .ok_or_else(|| {
                    ConfigError::Path("cannot determine config path: APPDATA is not set".to_owned())
                }),
        }
    }
}

pub fn config_path(explicit: Option<PathBuf>) -> Result<PathBuf, ConfigError> {
    let environment = env::vars().collect::<BTreeMap<_, _>>();
    ConfigPathOptions::new(explicit, Platform::current(), &environment).resolve()
}
