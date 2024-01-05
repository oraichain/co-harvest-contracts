use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("OverflowError")]
    Overflow {},

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Invalid bidding time range")]
    InvalidBiddingTimeRange {},

    #[error("Invalid bidding token")]
    InvalidBiddingToken {},

    #[error("Bidding round is not opening")]
    BidNotOpen {},

    #[error("Bidding round has not ended yet")]
    BidNotEnded {},
}
