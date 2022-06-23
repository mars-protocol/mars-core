use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Addr, Decimal, Uint128};
use cw_storage_plus::{Item, Map};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Config {
    pub owner: Addr,
    pub redbank_addr: Addr,
    pub astro_generator_addr: Addr,
    pub redbank_treasury: Addr,
    pub lp_token_addr: Addr,
    pub ma_token_addr: Option<Addr>,
    pub pool_addr: Addr,
    pub astro_token: Addr,
    pub proxy_token: Option<Addr>,
    pub astro_treasury_fee: Decimal,
    pub proxy_treasury_fee: Decimal,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct State {
    pub emergency_withdraw_executed: bool,
    /// Boolean value which if True imples staking is allowed
    pub is_stakable: bool,
    /// Total number of ma_tokens for which the underlying liquidity is staked
    pub total_ma_shares_staked: Uint128,
    /// Total number of underlying tokens which have been staked
    pub total_underlying_tokens_staked: Uint128,
    /// ASTRO token balance before the rewards were claimed from the AstroGenerator
    pub astro_balance_before_claim: Uint128,
    /// Ratio of Generator ASTRO rewards accured per maToken share
    pub global_astro_per_ma_share_index: Decimal,
    /// Proxy token balance before the rewards were claimed from the AstroGenerator
    pub proxy_balance_before_claim: Uint128,
    /// Ratio of Generator Proxy rewards accured per maToken share
    pub global_proxy_per_ma_share_index: Decimal,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct UserInfo {
    /// Number of maTokens staked by the user
    pub ma_tokens_staked: Uint128,
    /// Number of underlying Tokens staked by the user
    pub underlying_tokens_staked: Uint128,
    /// Ratio to keep track of ASTRO tokens accrued as rewards by the user
    pub user_astro_per_ma_share_index: Decimal,
    /// Generator ASTRO tokens accrued as rewards by the user
    pub claimable_astro: Uint128,
    /// Ratio to keep track of Proxy tokens accrued as rewards by the user
    pub user_proxy_per_ma_share_index: Decimal,
    /// Generator Proxy tokens accrued as rewards by the user
    pub claimable_proxy: Uint128,
}

impl Default for UserInfo {
    fn default() -> Self {
        UserInfo {
            ma_tokens_staked: Uint128::zero(),
            underlying_tokens_staked: Uint128::zero(),
            user_astro_per_ma_share_index: Decimal::zero(),
            claimable_astro: Uint128::zero(),
            user_proxy_per_ma_share_index: Decimal::zero(),
            claimable_proxy: Uint128::zero(),
        }
    }
}

pub const CONFIG: Item<Config> = Item::new("config");
pub const STATE: Item<State> = Item::new("state");
pub const USERS: Map<&Addr, UserInfo> = Map::new("users");
