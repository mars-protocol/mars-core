use cosmwasm_std::{to_binary, Addr, CosmosMsg, Decimal, Env, StdResult, Uint128, WasmMsg};
use cw20::Cw20ReceiveMsg;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub owner: Addr,
    pub redbank_addr: Addr,
    pub astro_generator_addr: Addr,
    pub redbank_treasury: Addr,
    pub token_addr: Addr,
    pub ma_token_addr: Option<Addr>,
    pub pool_addr: Addr,
    pub astro_token: Addr,
    pub proxy_token: Option<Addr>,
    pub is_stakable: bool,
}

/// ## Description
/// This structure describes the execute messages of the contract.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    /// Receives a message of type [`Cw20ReceiveMsg`]
    Receive(Cw20ReceiveMsg),
    /// Unstake LP Tokens
    Withdraw {
        user_addr: Addr,
        ma_token_share: Uint128,
        lp_token_amount: Uint128,
        claim_rewards: bool,
    },
    SetMaToken {
        ma_token_addr: Addr,
    },
    /// Admin function to Update fees charged on rewards
    UpdateFee {
        astro_treasury_fee: Decimal,
        proxy_treasury_fee: Decimal,
    },
    UpdateOnTransfer {
        from_user_addr: Addr,
        to_user_addr: Addr,
        underlying_amount: Uint128,
        ma_token_share: Uint128,
    },
    /// Admin function (callable only by ma_token) to claim rewards and unstake (if needed) when burning ma_shares
    UnstakeBeforeBurn {
        user_address: Addr,
        ma_shares_to_burn: Uint128,
    },
    /// Admin function (callable only by owner) to enable / disable staking
    SwitchStakingStatus {
        is_staking_allowed: bool,
    },
    /// Admin function (callable only by owner) to unstake all tokens from AstroGenerator
    EmergencyWithdraw {},
    /// the callback of type [`CallbackMsg`]
    Callback(CallbackMsg),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Cw20HookMsg {
    DepositWithProxy {
        user_addr: Addr,
        ma_token_share: Uint128,
    },
}

/// ## Description
/// This structure describes the callback messages of the contract.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CallbackMsg {
    UpdateIndexesAndExecute {
        execute_msg: ExecuteOnCallback,
    },
    TransferLpTokensToRedBank {
        prev_lp_balance: Uint128,
        lp_tokens_to_transfer: Uint128,
    },
    TransferTokensAfterWithdraw {
        /// the recipient
        account: Addr,
        /// the previous lp balance to calculate withdrawn amount
        prev_balance: Uint128,
    },
}

// Modified from https://github.com/CosmWasm/cosmwasm-plus/blob/v0.2.3/packages/cw20/src/receiver.rs#L15
impl CallbackMsg {
    pub fn to_cosmos_msg(self, env: &Env) -> StdResult<CosmosMsg> {
        Ok(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_binary(&ExecuteMsg::Callback(self))?,
            funds: vec![],
        }))
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub enum ExecuteOnCallback {
    /// Stakes LP tokens with AstroGenerator
    Stake {
        user_addr: Addr,
        ma_token_share: Uint128,
        lp_token_amount: Uint128,
    },
    /// Unstakes LP tokens from AstroGenerator
    Unstake {
        user_addr: Addr,
        ma_token_share: Uint128,
        lp_token_amount: Uint128,
        claim_rewards: bool,
    },
    UpdateFee {
        astro_treasury_fee: Decimal,
        proxy_treasury_fee: Decimal,
    },
    UpdateOnTransfer {
        from_user_addr: Addr,
        to_user_addr: Addr,
        underlying_amount: Uint128,
        ma_token_share: Uint128,
    },
    UnstakeBeforeBurn {
        user_address: Addr,
        ma_shares_to_burn: Uint128,
    },
    SwitchStakingStatus {
        is_staking_allowed: bool,
    },
    EmergencyWithdraw {},
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct ConfigResponse {
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
    /// Boolean value which if True imples staking is allowed
    pub is_stakable: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct StateResponse {
    pub emergency_withdraw_executed: bool,
    /// Boolean value which if True imples staking is allowed
    pub is_stakable: bool,
    /// Total number of ma_tokens for which the underlying liquidity is staked
    pub total_ma_shares_staked: Uint128,
    /// Total number of underlying tokens which have been staked
    pub total_underlying_tokens_staked: Uint128,
    /// Ratio of Generator ASTRO rewards accured per maToken share
    pub global_astro_per_ma_share_index: Decimal,
    /// Ratio of Generator Proxy rewards accured per maToken share
    pub global_proxy_per_ma_share_index: Decimal,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct UserInfoResponse {
    pub ma_tokens_staked: Uint128,
    pub underlying_tokens_staked: Uint128,
    pub claimable_astro: Uint128,
    pub claimable_proxy: Uint128,
}

/// ## Description
/// This structure describes the query messages of the contract.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    /// Returns the contract's configuration struct
    Config {},
    State {},
    UserInfo {
        user_address: Addr,
    },
}

/// ## Description
/// This structure describes a migration message.
/// We currently take no arguments for migrations
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct MigrateMsg {}
