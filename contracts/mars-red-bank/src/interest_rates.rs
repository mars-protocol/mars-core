use std::str;

use cosmwasm_std::{
    to_binary, Addr, CosmosMsg, DepsMut, Env, Event, Response, StdError, StdResult, Uint128,
    WasmMsg,
};
use cw20::Cw20ExecuteMsg;

use mars_core::asset::get_asset_balance;
use mars_core::math::decimal::Decimal;
use mars_core::math::uint128_checked_div_with_ceil;

use crate::error::ContractError;
use crate::interest_rate_models::update_market_interest_rates_with_model;
use crate::Market;

/// Scaling factor used to keep more precision during division / multiplication by index.
pub const SCALING_FACTOR: Uint128 = Uint128::new(1_000_000);

const SECONDS_PER_YEAR: u64 = 31536000u64;

/// Calculates accumulated interest for the time between last time market index was updated
/// and current block.
/// Applies desired side effects:
/// 1. Updates market borrow and liquidity indices.
/// 2. If there are any protocol rewards, builds a mint to the rewards collector and adds it
///    to the returned response
/// NOTE: it does not save the market to store
/// WARNING: For a given block, this function should be called before updating interest rates
/// as it would apply the new interest rates instead of the ones that were valid during
/// the period between indexes_last_updated and current_block
pub fn apply_accumulated_interests(
    env: &Env,
    protocol_rewards_collector_address: Addr,
    market: &mut Market,
    mut response: Response,
) -> StdResult<Response> {
    let current_timestamp = env.block.time.seconds();
    let previous_borrow_index = market.borrow_index;

    // Update market indices
    if market.indexes_last_updated < current_timestamp {
        let time_elapsed = current_timestamp - market.indexes_last_updated;

        if market.borrow_rate > Decimal::zero() {
            market.borrow_index = calculate_applied_linear_interest_rate(
                market.borrow_index,
                market.borrow_rate,
                time_elapsed,
            )?;
        }
        if market.liquidity_rate > Decimal::zero() {
            market.liquidity_index = calculate_applied_linear_interest_rate(
                market.liquidity_index,
                market.liquidity_rate,
                time_elapsed,
            )?;
        }
        market.indexes_last_updated = current_timestamp;
    }

    // Compute accrued protocol rewards
    let previous_debt_total = compute_underlying_amount(
        market.debt_total_scaled,
        previous_borrow_index,
        ScalingOperation::Ceil,
    )?;
    let new_debt_total = compute_underlying_amount(
        market.debt_total_scaled,
        market.borrow_index,
        ScalingOperation::Ceil,
    )?;

    let borrow_interest_accrued = if new_debt_total > previous_debt_total {
        // debt stays constant between the application of the interest rate
        // so the difference between debt at the start and the end is the
        // total borrow interest accrued
        new_debt_total - previous_debt_total
    } else {
        Uint128::zero()
    };

    let accrued_protocol_rewards = borrow_interest_accrued * market.reserve_factor;

    if accrued_protocol_rewards > Uint128::zero() {
        let mint_amount = compute_scaled_amount(
            accrued_protocol_rewards,
            market.liquidity_index,
            ScalingOperation::Truncate,
        )?;
        response = response.add_message(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: market.ma_token_address.clone().into(),
            msg: to_binary(&Cw20ExecuteMsg::Mint {
                recipient: protocol_rewards_collector_address.into(),
                amount: mint_amount,
            })?,
            funds: vec![],
        }))
    }
    Ok(response)
}

pub fn calculate_applied_linear_interest_rate(
    index: Decimal,
    rate: Decimal,
    time_elapsed: u64,
) -> StdResult<Decimal> {
    let rate_factor = rate.checked_mul(Decimal::from_ratio(
        Uint128::from(time_elapsed),
        Uint128::from(SECONDS_PER_YEAR),
    ))?;
    index.checked_mul(Decimal::one() + rate_factor)
}

/// Get scaled liquidity amount from an underlying amount, a Market and timestamp in seconds
/// Liquidity amounts are always truncated to make sure rounding errors accumulate in favor of
/// the protocol
/// NOTE: Calling this function when interests for the market are up to date with the current block
/// and index is not, will use the wrong interest rate to update the index.
/// NOTE: This function should not be used when calculating how much scaled amount is getting
/// burned from given underlying withdraw amount. In that case, all math should be done in underlying
/// amounts then get scaled back again
pub fn get_scaled_liquidity_amount(
    amount: Uint128,
    market: &Market,
    timestamp: u64,
) -> StdResult<Uint128> {
    compute_scaled_amount(
        amount,
        get_updated_liquidity_index(market, timestamp)?,
        ScalingOperation::Truncate,
    )
}

/// Get underlying liquidity amount from a scaled amount, a Market and timestamp in seconds
/// Liquidity amounts are always truncated to make sure rounding errors accumulate in favor of
/// the protocol
/// NOTE: Calling this function when interests for the market are up to date with the current block
/// and index is not, will use the wrong interest rate to update the index.
pub fn get_underlying_liquidity_amount(
    amount_scaled: Uint128,
    market: &Market,
    timestamp: u64,
) -> StdResult<Uint128> {
    compute_underlying_amount(
        amount_scaled,
        get_updated_liquidity_index(market, timestamp)?,
        ScalingOperation::Truncate,
    )
}

/// Get scaled borrow amount from an underlying amount, a Market and timestamp in seconds
/// Debt amounts are always ceiled to make sure rounding errors accumulate in favor of
/// the protocol
/// NOTE: Calling this function when interests for the market are up to date with the current block
/// and index is not, will use the wrong interest rate to update the index.
/// NOTE: This function should not be used when calculating how much scaled amount is getting
/// repaid from a sent underlying amount. In that case, all math should be done in underlying
/// amounts then get scaled back again
pub fn get_scaled_debt_amount(
    amount: Uint128,
    market: &Market,
    timestamp: u64,
) -> StdResult<Uint128> {
    compute_scaled_amount(
        amount,
        get_updated_borrow_index(market, timestamp)?,
        ScalingOperation::Ceil,
    )
}

/// Get underlying borrow amount from a scaled amount, a Market and timestamp in seconds
/// Debt amounts are always ceiled so as for rounding errors to accumulate in favor of
/// the protocol
/// NOTE: Calling this function when interests for the market are up to date with the current block
/// and index is not, will use the wrong interest rate to update the index.
pub fn get_underlying_debt_amount(
    amount_scaled: Uint128,
    market: &Market,
    timestamp: u64,
) -> StdResult<Uint128> {
    compute_underlying_amount(
        amount_scaled,
        get_updated_borrow_index(market, timestamp)?,
        ScalingOperation::Ceil,
    )
}

pub enum ScalingOperation {
    Truncate,
    Ceil,
}

/// Scales the amount dividing by an index in order to compute interest rates. Before dividing,
/// the value is multiplied by SCALING_FACTOR for greater precision.
/// Example:
/// Current index is 10. We deposit 6.123456 UST (6123456 uusd). Scaled amount will be
/// 6123456 / 10 = 612345 so we loose some precision. In order to avoid this situation
/// we scale the amount by SCALING_FACTOR.
pub fn compute_scaled_amount(
    amount: Uint128,
    index: Decimal,
    scaling_operation: ScalingOperation,
) -> StdResult<Uint128> {
    // Scale by SCALING_FACTOR to have better precision
    let scaled_amount = amount.checked_mul(SCALING_FACTOR)?;
    match scaling_operation {
        ScalingOperation::Truncate => Decimal::divide_uint128_by_decimal(scaled_amount, index),
        ScalingOperation::Ceil => Decimal::divide_uint128_by_decimal_and_ceil(scaled_amount, index),
    }
}

/// Descales the amount introduced by `get_scaled_amount`, returning the underlying amount.
/// As interest rate is accumulated the index used to descale the amount should be bigger than the one used to scale it.
pub fn compute_underlying_amount(
    scaled_amount: Uint128,
    index: Decimal,
    scaling_operation: ScalingOperation,
) -> StdResult<Uint128> {
    // Multiply scaled amount by decimal (index)
    let before_scaling_factor = scaled_amount * index;

    // Descale by SCALING_FACTOR which is introduced when scaling the amount
    match scaling_operation {
        ScalingOperation::Truncate => Ok(before_scaling_factor.checked_div(SCALING_FACTOR)?),
        ScalingOperation::Ceil => {
            uint128_checked_div_with_ceil(before_scaling_factor, SCALING_FACTOR)
        }
    }
}

/// Return applied interest rate for borrow index according to passed blocks
/// NOTE: Calling this function when interests for the market are up to date with the current block
/// and index is not, will use the wrong interest rate to update the index.
pub fn get_updated_borrow_index(market: &Market, timestamp: u64) -> StdResult<Decimal> {
    if market.indexes_last_updated < timestamp {
        let time_elapsed = timestamp - market.indexes_last_updated;

        if market.borrow_rate > Decimal::zero() {
            let updated_index = calculate_applied_linear_interest_rate(
                market.borrow_index,
                market.borrow_rate,
                time_elapsed,
            );
            return updated_index;
        }
    }

    Ok(market.borrow_index)
}

/// Return applied interest rate for liquidity index according to passed blocks
/// NOTE: Calling this function when interests for the market are up to date with the current block
/// and index is not, will use the wrong interest rate to update the index.
pub fn get_updated_liquidity_index(market: &Market, timestamp: u64) -> StdResult<Decimal> {
    if market.indexes_last_updated > timestamp {
        return Err(StdError::generic_err(
            format!("Cannot compute updated liquidity index for a timestamp: {} smaller than last updated timestamp for market: {}", timestamp, market.indexes_last_updated)
        ));
    }

    if market.indexes_last_updated < timestamp {
        let time_elapsed = timestamp - market.indexes_last_updated;

        if market.liquidity_rate > Decimal::zero() {
            let updated_index = calculate_applied_linear_interest_rate(
                market.liquidity_index,
                market.liquidity_rate,
                time_elapsed,
            );
            return updated_index;
        }
    }

    Ok(market.liquidity_index)
}

/// Update interest rates for current liquidity and debt levels
/// Note it does not save the market to the store (that is left to the caller)
/// Returns response with appended interest rates updated event
/// NOTE: For a given block, interest rates should not be updated before updating indexes first
/// as it should result in wrong indexes
pub fn update_interest_rates(
    deps: &DepsMut,
    env: &Env,
    market: &mut Market,
    liquidity_taken: Uint128,
    asset_label: &str,
    mut response: Response,
) -> Result<Response, ContractError> {
    // compute utilization rate
    let contract_current_balance = get_asset_balance(
        deps.as_ref(),
        env.contract.address.clone(),
        asset_label.to_string(),
        market.asset_type,
    )?;
    if contract_current_balance < liquidity_taken {
        return Err(ContractError::OperationExceedsAvailableLiquidity {});
    }
    let available_liquidity = contract_current_balance - liquidity_taken;
    let total_debt =
        get_underlying_debt_amount(market.debt_total_scaled, market, env.block.time.seconds())?;
    let current_utilization_rate = if total_debt > Uint128::zero() {
        let liquidity_and_debt = available_liquidity.checked_add(total_debt)?;
        Decimal::from_ratio(total_debt, liquidity_and_debt)
    } else {
        Decimal::zero()
    };

    update_market_interest_rates_with_model(env, market, current_utilization_rate)?;

    response = response.add_event(build_interests_updated_event(asset_label, market));
    Ok(response)
}

pub fn build_interests_updated_event(label: &str, market: &Market) -> Event {
    Event::new("interests_updated")
        .add_attribute("asset", label)
        .add_attribute("borrow_index", market.borrow_index.to_string())
        .add_attribute("liquidity_index", market.liquidity_index.to_string())
        .add_attribute("borrow_rate", market.borrow_rate.to_string())
        .add_attribute("liquidity_rate", market.liquidity_rate.to_string())
}

#[cfg(test)]
mod tests {
    use cosmwasm_std::Uint128;
    use mars_core::math::decimal::Decimal;
    use mars_core::red_bank::Market;

    use crate::interest_rates::{
        calculate_applied_linear_interest_rate, get_scaled_debt_amount,
        get_scaled_liquidity_amount, get_underlying_debt_amount, get_underlying_liquidity_amount,
    };

    #[test]
    fn test_accumulated_index_calculation() {
        let index = Decimal::from_ratio(1u128, 10u128);
        let rate = Decimal::from_ratio(2u128, 10u128);
        let time_elapsed = 15768000; // half a year
        let accumulated =
            calculate_applied_linear_interest_rate(index, rate, time_elapsed).unwrap();

        assert_eq!(accumulated, Decimal::from_ratio(11u128, 100u128));
    }

    #[test]
    fn test_liquidity_and_debt_rounding() {
        let start = Uint128::from(100_000_000_000_u128);
        let mut market = Market::default();
        market.liquidity_index = Decimal::from_ratio(3_u128, 1_u128);
        market.borrow_index = Decimal::from_ratio(3_u128, 1_u128);
        market.indexes_last_updated = 1;

        let scaled_amount_liquidity = get_scaled_liquidity_amount(start, &market, 1).unwrap();
        let scaled_amount_debt = get_scaled_debt_amount(start, &market, 1).unwrap();
        assert_eq!(
            Uint128::from(33_333_333_333_333_333_u128),
            scaled_amount_liquidity
        );
        assert_eq!(
            Uint128::from(33_333_333_333_333_334_u128),
            scaled_amount_debt
        );

        let back_to_underlying_liquidity =
            get_underlying_liquidity_amount(scaled_amount_liquidity, &market, 1).unwrap();
        let back_to_underlying_debt =
            get_underlying_debt_amount(scaled_amount_debt, &market, 1).unwrap();
        assert_eq!(
            Uint128::from(99_999_999_999_u128),
            back_to_underlying_liquidity
        );
        assert_eq!(Uint128::from(100_000_000_001_u128), back_to_underlying_debt);
    }
}
