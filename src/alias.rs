//! Deterministic expansion of configured one-operation aliases.

use std::num::NonZeroUsize;

use crate::{config::Config, domain::Operation};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AliasInvocation {
    pub operation: Operation,
    pub target: String,
    pub parallelism: NonZeroUsize,
    pub wait: bool,
    pub force: bool,
}

pub fn expand_alias(config: &Config, name: &str) -> Result<AliasInvocation, AliasError> {
    let alias = config
        .aliases()
        .get(name)
        .ok_or_else(|| AliasError::Unknown(name.to_owned()))?;
    let default_parallelism = if alias.operation == Operation::Status {
        4
    } else {
        1
    };
    Ok(AliasInvocation {
        operation: alias.operation.clone(),
        target: alias.target.clone(),
        parallelism: NonZeroUsize::new(alias.parallel.unwrap_or(default_parallelism))
            .expect("configuration validation rejects zero parallelism"),
        wait: alias.wait,
        force: alias.force,
    })
}

#[derive(Debug, thiserror::Error)]
pub enum AliasError {
    #[error("unknown alias `{0}`")]
    Unknown(String),
    #[error("alias operation `{0:?}` is not available yet")]
    Unsupported(Operation),
}
