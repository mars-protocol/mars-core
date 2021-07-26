use std::string::FromUtf8Error;

use cosmwasm_std::StdError;
use mars::error::MarsError;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Mars(#[from] MarsError),

    #[error("{0}")]
    Std(#[from] StdError),

    #[error("{0}")]
    FromUtf8(#[from] FromUtf8Error),

    #[error("Start time must be before end time")]
    IndexStartEndTime {},
}