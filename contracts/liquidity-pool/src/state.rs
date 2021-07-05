use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_bignumber::{Decimal256, Uint256};
use cosmwasm_std::{CanonicalAddr, StdError, StdResult, Storage, Uint128};
use cosmwasm_storage::{
    bucket, bucket_read, singleton, singleton_read, Bucket, ReadonlyBucket, ReadonlySingleton,
    Singleton,
};
use mars::helpers::all_conditions_valid;
use mars::liquidity_pool::msg::{AssetType, InitOrUpdateAssetParams};

// keys (for singleton)
pub static CONFIG_KEY: &[u8] = b"config";
pub static MONEY_MARKET_KEY: &[u8] = b"money_market";

// namespaces (for buckets)
pub static RESERVES_NAMESPACE: &[u8] = b"reserves";
pub static DEBTS_NAMESPACE: &[u8] = b"debts";
pub static USERS_NAMESPACE: &[u8] = b"users";
pub static RESERVE_REFERENCES_NAMESPACE: &[u8] = b"reserve_references";
pub static RESERVE_MA_TOKENS_NAMESPACE: &[u8] = b"reserve_ma_tokens";
pub static UNCOLLATERALIZED_LOAN_LIMITS_NAMESPACE: &[u8] = b"uncollateralized_loan_limits";

/// Lending pool global configuration
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Config {
    /// Contract owner
    pub owner: CanonicalAddr,
    /// Address provider returns addresses for all protocol contracts
    pub address_provider_address: CanonicalAddr,
    /// maToken code id used to instantiate new tokens
    pub ma_token_code_id: u64,
    /// Maximum percentage of outstanding debt that can be covered by a liquidator
    pub close_factor: Decimal256,
    /// Percentage of fees that are sent to the insurance fund
    pub insurance_fund_fee_share: Decimal256,
    /// Percentage of fees that are sent to the treasury
    pub treasury_fee_share: Decimal256,
}

impl Config {
    pub fn validate(&self) -> StdResult<()> {
        let conditions_and_names = vec![
            (Self::less_or_equal_one(&self.close_factor), "close_factor"),
            (
                Self::less_or_equal_one(&self.insurance_fund_fee_share),
                "insurance_fund_fee_share",
            ),
            (
                Self::less_or_equal_one(&self.treasury_fee_share),
                "treasury_fee_share",
            ),
        ];
        all_conditions_valid(conditions_and_names)?;

        let combined_fee_share = self.insurance_fund_fee_share + self.treasury_fee_share;
        // Combined fee shares cannot exceed one
        if combined_fee_share > Decimal256::one() {
            return Err(StdError::generic_err(
                "Invalid fee share amounts. Sum of insurance and treasury fee shares exceed one",
            ));
        }

        Ok(())
    }

    fn less_or_equal_one(value: &Decimal256) -> bool {
        value.le(&Decimal256::one())
    }
}

pub fn config_state<S: Storage>(storage: &mut S) -> Singleton<S, Config> {
    singleton(storage, CONFIG_KEY)
}

pub fn config_state_read<S: Storage>(storage: &S) -> ReadonlySingleton<S, Config> {
    singleton_read(storage, CONFIG_KEY)
}

/// MoneyMarket global state
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct MoneyMarket {
    /// Reserve count
    pub reserve_count: u32,
}

pub fn money_market_state<S: Storage>(storage: &mut S) -> Singleton<S, MoneyMarket> {
    singleton(storage, MONEY_MARKET_KEY)
}

pub fn money_market_state_read<S: Storage>(storage: &S) -> ReadonlySingleton<S, MoneyMarket> {
    singleton_read(storage, MONEY_MARKET_KEY)
}

/// Asset reserves
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Reserve {
    /// Reserve index (Bit position on data)
    pub index: u32,
    /// maToken contract address
    pub ma_token_address: CanonicalAddr,

    /// Borrow index (Used to compute borrow interest)
    pub borrow_index: Decimal256,
    /// Liquidity index (Used to compute deposit interest)
    pub liquidity_index: Decimal256,
    /// Rate charged to borrowers
    pub borrow_rate: Decimal256,
    /// Minimum borrow rate
    pub min_borrow_rate: Decimal256,
    /// Maximum borrow rate
    pub max_borrow_rate: Decimal256,
    /// Rate paid to depositors
    pub liquidity_rate: Decimal256,

    /// Max percentage of collateral that can be borrowed
    pub loan_to_value: Decimal256,

    /// Portion of the borrow rate that is sent to the treasury, insurance fund, and rewards
    pub reserve_factor: Decimal256,

    /// Timestamp (seconds) where indexes and rates where last updated
    pub interests_last_updated: u64,
    /// Total debt scaled for the reserve's currency
    pub debt_total_scaled: Uint256,

    /// Indicated whether the asset is native or a cw20 token
    pub asset_type: AssetType,

    /// Percentage at which the loan is defined as under-collateralized
    pub liquidation_threshold: Decimal256,
    /// Bonus on the price of assets of the collateral when liquidators purchase it
    pub liquidation_bonus: Decimal256,

    /// Income to be distributed to other protocol contracts
    pub protocol_income_to_distribute: Uint256,

    /// PID parameters
    pub pid_parameters: PidParameters,
}

/// PID parameters
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct PidParameters {
    /// Proportional parameter for the PID controller
    pub kp: Decimal256,
    /// Optimal utilization rate targeted by the PID controller. Interest rate will decrease when lower and increase when higher
    pub optimal_utilization_rate: Decimal256,
    /// Min error that triggers Kp augmentation
    pub kp_augmentation_threshold: Decimal256,
    /// Kp multiplier when error threshold is exceeded
    pub kp_multiplier: Decimal256,
}

impl Reserve {
    /// Initialize new reserve
    pub fn create(
        block_time: u64,
        index: u32,
        asset_type: AssetType,
        params: InitOrUpdateAssetParams,
    ) -> StdResult<Self> {
        // Destructuring a struct’s fields into separate variables in order to force
        // compile error if we add more params
        let InitOrUpdateAssetParams {
            initial_borrow_rate: borrow_rate,
            min_borrow_rate,
            max_borrow_rate,
            loan_to_value,
            reserve_factor,
            liquidation_threshold,
            liquidation_bonus,
            kp,
            optimal_utilization_rate: u_optimal,
            kp_augmentation_threshold,
            kp_multiplier,
        } = params;

        // All fields should be available
        let available = borrow_rate.is_some()
            && min_borrow_rate.is_some()
            && max_borrow_rate.is_some()
            && loan_to_value.is_some()
            && reserve_factor.is_some()
            && liquidation_threshold.is_some()
            && liquidation_bonus.is_some()
            && kp.is_some()
            && u_optimal.is_some()
            && kp_augmentation_threshold.is_some()
            && kp_multiplier.is_some();

        if !available {
            return Err(StdError::generic_err(
                "All params should be available during initialization",
            ));
        }

        // Unwraps on params are save (validated with `validate_availability_of_all_params`)
        let new_pid_params = PidParameters {
            kp: kp.unwrap(),
            optimal_utilization_rate: u_optimal.unwrap(),
            kp_augmentation_threshold: kp_augmentation_threshold.unwrap(),
            kp_multiplier: kp_multiplier.unwrap(),
        };
        let new_reserve = Reserve {
            index,
            asset_type,
            ma_token_address: CanonicalAddr::default(),
            borrow_index: Decimal256::one(),
            liquidity_index: Decimal256::one(),
            borrow_rate: borrow_rate.unwrap(),
            min_borrow_rate: min_borrow_rate.unwrap(),
            max_borrow_rate: max_borrow_rate.unwrap(),
            liquidity_rate: Decimal256::zero(),
            loan_to_value: loan_to_value.unwrap(),
            reserve_factor: reserve_factor.unwrap(),
            interests_last_updated: block_time,
            debt_total_scaled: Uint256::zero(),
            liquidation_threshold: liquidation_threshold.unwrap(),
            liquidation_bonus: liquidation_bonus.unwrap(),
            protocol_income_to_distribute: Uint256::zero(),
            pid_parameters: new_pid_params,
        };

        new_reserve.validate()?;

        Ok(new_reserve)
    }

    fn validate(&self) -> StdResult<()> {
        if self.min_borrow_rate >= self.max_borrow_rate {
            return Err(StdError::generic_err(format!(
                "max_borrow_rate should be greater than min_borrow_rate. \
                    max_borrow_rate: {}, \
                    min_borrow_rate: {}",
                self.max_borrow_rate, self.min_borrow_rate
            )));
        }

        if self.pid_parameters.optimal_utilization_rate > Decimal256::one() {
            return Err(StdError::generic_err(
                "Optimal utilization rate can't be greater than one",
            ));
        }

        // loan_to_value, reserve_factor, liquidation_threshold and liquidation_bonus should be less or equal 1
        let conditions_and_names = vec![
            (self.loan_to_value.le(&Decimal256::one()), "loan_to_value"),
            (self.reserve_factor.le(&Decimal256::one()), "reserve_factor"),
            (
                self.liquidation_threshold.le(&Decimal256::one()),
                "liquidation_threshold",
            ),
            (
                self.liquidation_bonus.le(&Decimal256::one()),
                "liquidation_bonus",
            ),
        ];
        all_conditions_valid(conditions_and_names)?;

        // liquidation_threshold should be greater than loan_to_value
        if self.liquidation_threshold <= self.loan_to_value {
            return Err(StdError::generic_err(format!(
                "liquidation_threshold should be greater than loan_to_value. \
                    liquidation_threshold: {}, \
                    loan_to_value: {}",
                self.liquidation_threshold, self.loan_to_value
            )));
        }

        Ok(())
    }

    /// Update reserve based on new params
    pub fn update_with(self, params: InitOrUpdateAssetParams) -> StdResult<Self> {
        // Destructuring a struct’s fields into separate variables in order to force
        // compile error if we add more params
        let InitOrUpdateAssetParams {
            initial_borrow_rate: _,
            min_borrow_rate,
            max_borrow_rate,
            loan_to_value,
            reserve_factor,
            liquidation_threshold,
            liquidation_bonus,
            kp,
            optimal_utilization_rate: u_optimal,
            kp_augmentation_threshold,
            kp_multiplier,
        } = params;

        let updated_pid_params = PidParameters {
            kp: kp.unwrap_or(self.pid_parameters.kp),
            optimal_utilization_rate: u_optimal
                .unwrap_or(self.pid_parameters.optimal_utilization_rate),
            kp_augmentation_threshold: kp_augmentation_threshold
                .unwrap_or(self.pid_parameters.kp_augmentation_threshold),
            kp_multiplier: kp_multiplier.unwrap_or(self.pid_parameters.kp_multiplier),
        };
        let updated_reserve = Reserve {
            min_borrow_rate: min_borrow_rate.unwrap_or(self.min_borrow_rate),
            max_borrow_rate: max_borrow_rate.unwrap_or(self.max_borrow_rate),
            loan_to_value: loan_to_value.unwrap_or(self.loan_to_value),
            reserve_factor: reserve_factor.unwrap_or(self.reserve_factor),
            liquidation_threshold: liquidation_threshold.unwrap_or(self.liquidation_threshold),
            liquidation_bonus: liquidation_bonus.unwrap_or(self.liquidation_bonus),
            pid_parameters: updated_pid_params,
            ..self
        };

        updated_reserve.validate()?;

        Ok(updated_reserve)
    }
}

pub fn reserves_state<S: Storage>(storage: &mut S) -> Bucket<S, Reserve> {
    bucket(RESERVES_NAMESPACE, storage)
}

pub fn reserves_state_read<S: Storage>(storage: &S) -> ReadonlyBucket<S, Reserve> {
    bucket_read(RESERVES_NAMESPACE, storage)
}

/// Data for individual users
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct User {
    /// bitmap representing borrowed asset. 1 on the corresponding bit means asset is
    /// being borrowed
    pub borrowed_assets: Uint128,
    pub collateral_assets: Uint128,
}

impl Default for User {
    fn default() -> Self {
        User {
            borrowed_assets: Uint128::zero(),
            collateral_assets: Uint128::zero(),
        }
    }
}

pub fn users_state<S: Storage>(storage: &mut S) -> Bucket<S, User> {
    bucket(USERS_NAMESPACE, storage)
}

pub fn users_state_read<S: Storage>(storage: &S) -> ReadonlyBucket<S, User> {
    bucket_read(USERS_NAMESPACE, storage)
}

/// Debt for each asset and user
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct Debt {
    /// Scaled debt amount
    // TODO(does this amount always have six decimals? How do we manage this?)
    pub amount_scaled: Uint256,
}

pub fn debts_asset_state<'a, S: Storage>(storage: &'a mut S, asset: &[u8]) -> Bucket<'a, S, Debt> {
    Bucket::multilevel(&[DEBTS_NAMESPACE, asset], storage)
}

pub fn debts_asset_state_read<'a, S: Storage>(
    storage: &'a S,
    asset: &[u8],
) -> ReadonlyBucket<'a, S, Debt> {
    ReadonlyBucket::multilevel(&[DEBTS_NAMESPACE, asset], storage)
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
// TODO: If we do not use the struct for anything else this struct should be deleted and
// the bucket should just store Vec<u8>
pub struct ReserveReferences {
    /// Reference of reserve
    pub reference: Vec<u8>,
}

pub fn reserve_references_state<S: Storage>(storage: &mut S) -> Bucket<S, ReserveReferences> {
    bucket(RESERVE_REFERENCES_NAMESPACE, storage)
}

pub fn reserve_references_state_read<S: Storage>(
    storage: &S,
) -> ReadonlyBucket<S, ReserveReferences> {
    bucket_read(RESERVE_REFERENCES_NAMESPACE, storage)
}

pub fn reserve_ma_tokens_state<S: Storage>(storage: &mut S) -> Bucket<S, Vec<u8>> {
    bucket(RESERVE_MA_TOKENS_NAMESPACE, storage)
}

pub fn reserve_ma_tokens_state_read<S: Storage>(storage: &S) -> ReadonlyBucket<S, Vec<u8>> {
    bucket_read(RESERVE_MA_TOKENS_NAMESPACE, storage)
}

/// Uncollateralized loan limits
pub fn uncollateralized_loan_limits<'a, S: Storage>(
    storage: &'a mut S,
    asset: &[u8],
) -> Bucket<'a, S, Uint128> {
    Bucket::multilevel(&[UNCOLLATERALIZED_LOAN_LIMITS_NAMESPACE, asset], storage)
}

pub fn uncollateralized_loan_limits_read<'a, S: Storage>(
    storage: &'a S,
    asset: &[u8],
) -> ReadonlyBucket<'a, S, Uint128> {
    ReadonlyBucket::multilevel(&[UNCOLLATERALIZED_LOAN_LIMITS_NAMESPACE, asset], storage)
}
