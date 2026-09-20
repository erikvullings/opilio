//! Device, group, site, and all-target resolution.

use std::collections::BTreeSet;

use crate::config::Config;

pub struct Target;

impl Target {
    pub fn resolve(name: &str, config: &Config) -> Result<Vec<String>, TargetError> {
        if name == "all" {
            return Ok(config.devices().keys().cloned().collect());
        }
        if config.devices().contains_key(name) {
            return Ok(vec![name.to_owned()]);
        }
        if let Some(group) = config.groups().get(name) {
            return Ok(group.devices.clone());
        }
        if config.sites().contains_key(name) {
            return Ok(config
                .devices()
                .iter()
                .filter(|(_, device)| device.site.as_deref() == Some(name))
                .map(|(device_name, _)| device_name.clone())
                .collect());
        }
        Err(TargetError::Unknown(name.to_owned()))
    }

    pub fn resolve_many<'a>(
        names: impl IntoIterator<Item = &'a str>,
        config: &Config,
    ) -> Result<Vec<String>, TargetError> {
        let mut devices = BTreeSet::new();
        for name in names {
            devices.extend(Self::resolve(name, config)?);
        }
        Ok(devices.into_iter().collect())
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TargetError {
    #[error("unknown target `{0}`; expected a device, group, site, or `all`")]
    Unknown(String),
}
