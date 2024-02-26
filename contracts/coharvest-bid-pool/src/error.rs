use cosmwasm_std::StdError;
use cw_utils::PaymentError;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),
    #[error("{0}")]
    Payment(#[from] PaymentError),

    #[error("OverflowError")]
    Overflow {},

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Invalid bidding time range")]
    InvalidBiddingTimeRange {},

    #[error("Invalid funds")]
    InvalidFunds {},

    #[error("Bidding round is not opening")]
    BidNotOpen {},

    #[error("Bidding round has not ended yet")]
    BidNotEnded {},
}
