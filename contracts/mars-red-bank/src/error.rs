use thiserror::Error;

use cosmwasm_std::{OverflowError, StdError};

use mars_core::error::MarsError;
use mars_core::math::decimal::Decimal;

use crate::MarketError;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("{0}")]
    Mars(#[from] MarsError),

    #[error("{0}")]
    Overflow(#[from] OverflowError),

    #[error("{0}")]
    Market(#[from] MarketError),

    #[error("Price not found for asset: {label:?}")]
    PriceNotFound { label: String },

    #[error("User has no balance (asset: {asset:?})")]
    UserNoBalance { asset: String },

    #[error(
        "User address {user_address:?} has no balance in specified collateral asset {asset:?}"
    )]
    UserNoCollateralBalance { user_address: String, asset: String },

    #[error(
        "Withdraw amount must be greater than 0 and less or equal user balance (asset: {asset:?})"
    )]
    InvalidWithdrawAmount { asset: String },

    #[error("Sender requires to have an existing user position")]
    ExistingUserPositionRequired {},

    #[error("User's health factor can't be less than 1 after withdraw")]
    InvalidHealthFactorAfterWithdraw {},

    #[error("User's health factor can't be less than 1 after withdraw")]
    AssetAlreadyInitialized {},

    #[error("Asset not initialized")]
    AssetNotInitialized {},

    #[error("Deposit amount must be greater than 0 {asset:?}")]
    InvalidDepositAmount { asset: String },

    #[error("Cannot have 0 as liquidity index")]
    InvalidLiquidityIndex {},

    #[error("Borrow amount must be greater than 0 {asset:?}")]
    InvalidBorrowAmount { asset: String },

    #[error("No borrow market exists with asset: {asset:?}")]
    BorrowMarketNotExists { asset: String },

    #[error("Address has no collateral deposited")]
    UserNoCollateral {},

    #[error("Borrow amount exceeds maximum allowed given current collateral value")]
    BorrowAmountExceedsGivenCollateral {},

    #[error("Borrow amount exceeds uncollateralized loan limit given existing debt")]
    BorrowAmountExceedsUncollateralizedLoanLimit {},

    #[error("Repay amount must be greater than 0 {asset:?}")]
    InvalidRepayAmount { asset: String },

    #[error("Cannot repay 0 debt")]
    CannotRepayZeroDebt {},

    #[error("Amount to repay is greater than total debt")]
    CannotRepayMoreThanDebt {},

    #[error("Amount to repay is greater than total debt")]
    CannotLiquidateWhenPositiveUncollateralizedLoanLimit {},

    #[error("Must send more than 0 {asset:?} in order to liquidate")]
    InvalidLiquidateAmount { asset: String },

    #[error("User has no balance in specified collateral asset to be liquidated")]
    CannotLiquidateWhenNoCollateralBalance {},

    #[error(
        "User has no outstanding debt in the specified debt asset and thus cannot be liquidated"
    )]
    CannotLiquidateWhenNoDebtBalance {},

    #[error("User's health factor is not less than 1 and thus cannot be liquidated")]
    CannotLiquidateHealthyPosition {},

    #[error("Contract does not have enough collateral liquidity to send back underlying asset")]
    CannotLiquidateWhenNotEnoughCollateral {},

    #[error(
        "Cannot make token transfer if it results in a health factor lower than 1 for the sender"
    )]
    CannotTransferTokenWhenInvalidHealthFactor {},


    #[error("Failed to encode asset reference into string")]
    CannotEncodeAssetReferenceIntoString {},

    #[error("Contract current asset balance cannot be less than liquidity taken")]
    OperationExceedsAvailableLiquidity {},

    #[error("Cannot withdraw asset {asset:?}")]
    WithdrawNotAllowed { asset: String },

    #[error("Cannot deposit asset {asset:?}")]
    DepositNotAllowed { asset: String },

    #[error("Cannot borrow asset {asset:?}")]
    BorrowNotAllowed { asset: String },

    #[error("Cannot repay asset {asset:?}")]
    RepayNotAllowed { asset: String },

    #[error("Cannot liquidate. Collateral asset {asset:?}")]
    LiquidationNotAllowedWhenCollateralMarketInactive { asset: String },

    #[error("Cannot liquidate. Debt asset {asset:?}")]
    LiquidationNotAllowedWhenDebtMarketInactive { asset: String },

    #[error("User's health factor can't be less than 1 after disabling collateral")]
    InvalidHealthFactorAfterDisablingCollateral {},
}

impl ContractError {
    pub fn price_not_found<S: Into<String>>(label: S) -> ContractError {
        ContractError::PriceNotFound {
            label: label.into(),
        }
    }
}
