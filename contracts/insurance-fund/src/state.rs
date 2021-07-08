use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{CanonicalAddr, Decimal, Storage};
use cosmwasm_storage::{singleton, singleton_read, ReadonlySingleton, Singleton};

// keys (for singleton)
pub static CONFIG_KEY: &[u8] = b"config";

/// Insurance fund global configuration
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Config {
    pub owner: CanonicalAddr,
    /// Terraswap factory contract address
    pub terraswap_factory_address: CanonicalAddr,
    /// Terraswap max spread
    pub terraswap_max_spread: Decimal,
}

pub fn config<S: Storage>(storage: &mut S) -> Singleton<S, Config> {
    singleton(storage, CONFIG_KEY)
}

pub fn config_read<S: Storage>(storage: &S) -> ReadonlySingleton<S, Config> {
    singleton_read(storage, CONFIG_KEY)
}