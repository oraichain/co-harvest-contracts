#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    from_json, to_json_binary, Addr, Binary, Decimal, Deps, DepsMut, Env, MessageInfo, Response,
    StdResult, Uint128,
};
use cw20::Cw20ReceiveMsg;
use cw_utils::one_coin;
use oraiswap::asset::AssetInfo;

use crate::{
    bid::{
        execute_create_new_round, execute_distribute, execute_finalize_bidding_round_result,
        execute_submit_bid, process_calc_distribution_amount,
    },
    error::ContractError,
    msg::{
        BiddingInfoResponse, Cw20HookMsg, EstimateAmountReceiveOfBidResponse, ExecuteMsg,
        InstantiateMsg, MigrateMsg, QueryMsg,
    },
    state::{
        count_number_bids_in_round, read_bids_by_round, Bid, BidPool, Config, BID, BIDDING_INFO,
        BIDS_BY_USER, BID_POOL, CONFIG, DISTRIBUTION_INFO, LAST_ROUND_ID,
    },
};

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let config = Config {
        owner: msg.owner,
        underlying_token: msg.underlying_token,
        distribution_token: msg.distribution_token,
        max_slot: msg.max_slot,
        premium_rate_per_slot: msg.premium_rate_per_slot,
        min_deposit_amount: msg.min_deposit_amount,
    };

    // store config
    CONFIG.save(deps.storage, &config)?;
    LAST_ROUND_ID.save(deps.storage, &0)?;
    Ok(Response::default())
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::Receive(msg) => receive_cw20(deps, env, info, msg),
        ExecuteMsg::UpdateConfig {
            owner,
            underlying_token,
            distribution_token,
            max_slot,
            premium_rate_per_slot,
            min_deposit_amount,
        } => execute_update_config(
            deps,
            info,
            owner,
            underlying_token,
            distribution_token,
            max_slot,
            premium_rate_per_slot,
            min_deposit_amount,
        ),
        ExecuteMsg::CreateNewRound {
            start_time,
            end_time,
            total_distribution,
        } => execute_create_new_round(deps, env, info, start_time, end_time, total_distribution),
        ExecuteMsg::FinalizeBiddingRoundResult {
            round,
            exchange_rate,
        } => execute_finalize_bidding_round_result(deps, env, info, round, exchange_rate),
        ExecuteMsg::Distribute {
            round,
            start_after,
            limit,
        } => execute_distribute(deps, round, start_after, limit),
        ExecuteMsg::SubmitBid {
            round,
            premium_slot,
        } => {
            let config = CONFIG.load(deps.storage)?;
            let funds = one_coin(&info).unwrap();

            // check the token participating in the bidding is valid
            if !config
                .underlying_token
                .eq(&AssetInfo::NativeToken { denom: funds.denom })
            {
                return Err(ContractError::InvalidBiddingToken {});
            }

            execute_submit_bid(
                deps,
                env,
                round,
                premium_slot,
                info.sender.to_string(),
                funds.amount,
            )
        }
    }
}

fn receive_cw20(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    cw20_msg: Cw20ReceiveMsg,
) -> Result<Response, ContractError> {
    match from_json(&cw20_msg.msg)? {
        Cw20HookMsg::SubmitBid {
            round,
            premium_slot,
        } => {
            let config: Config = CONFIG.load(deps.storage)?;
            // check the token participating in the bidding is valid
            if !config.underlying_token.eq(&AssetInfo::Token {
                contract_addr: info.sender,
            }) {
                return Err(ContractError::InvalidBiddingToken {});
            }

            execute_submit_bid(
                deps,
                env,
                round,
                premium_slot,
                cw20_msg.sender,
                cw20_msg.amount,
            )
        }
    }
}

fn execute_update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<Addr>,
    underlying_token: Option<AssetInfo>,
    distribution_token: Option<AssetInfo>,
    max_slot: Option<u8>,
    premium_rate_per_slot: Option<Decimal>,
    min_deposit_amount: Option<Uint128>,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;

    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }
    if let Some(owner) = owner {
        config.owner = owner;
    }
    if let Some(underlying_token) = underlying_token {
        config.underlying_token = underlying_token;
    }
    if let Some(distribution_token) = distribution_token {
        config.distribution_token = distribution_token;
    }
    if let Some(max_slot) = max_slot {
        config.max_slot = max_slot;
    }
    if let Some(premium_rate_per_slot) = premium_rate_per_slot {
        config.premium_rate_per_slot = premium_rate_per_slot;
    }
    if let Some(min_deposit_amount) = min_deposit_amount {
        config.min_deposit_amount = min_deposit_amount;
    }

    Ok(Response::default().add_attribute("action", "update_config"))
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::Bid { idx } => to_json_binary(&BID.load(deps.storage, idx)?),
        QueryMsg::BiddingInfo { round } => to_json_binary(&query_bidding_info(deps, round)?),
        QueryMsg::LastRoundId {} => to_json_binary(&LAST_ROUND_ID.load(deps.storage)?),
        QueryMsg::BidPool { round, slot } => {
            to_json_binary(&BID_POOL.load(deps.storage, (round, slot))?)
        }
        QueryMsg::AllBidPoolInRound { round } => {
            to_json_binary(&query_all_bid_pool_in_round(deps, round)?)
        }
        QueryMsg::BidsIdxByUser { round, user } => {
            to_json_binary(&BIDS_BY_USER.load(deps.storage, (round, user))?)
        }
        QueryMsg::EstimateAmountReceiveOfBid {
            round,
            idx,
            exchange_rate,
        } => to_json_binary(&query_estimate_amount_receive_of_bid(
            deps,
            round,
            idx,
            exchange_rate,
        )?),
        QueryMsg::EstimateAmountReceive {
            round,
            slot,
            bid_amount,
            exchange_rate,
        } => to_json_binary(&query_estimate_amount_receive(
            deps,
            round,
            slot,
            bid_amount,
            exchange_rate,
        )?),
        QueryMsg::AllBidInRound {
            round,
            start_after,
            limit,
        } => to_json_binary(&read_bids_by_round(
            deps.storage,
            round,
            start_after,
            limit,
        )?),
        QueryMsg::BidsByUser { round, user } => {
            to_json_binary(&query_bids_by_user(deps, round, user)?)
        }
        QueryMsg::NumbersBidInRound { round } => {
            to_json_binary(&count_number_bids_in_round(deps.storage, round))
        }
    }
}

fn query_bidding_info(deps: Deps, round: u64) -> StdResult<BiddingInfoResponse> {
    let bid_info = BIDDING_INFO.load(deps.storage, round)?;
    let distribution_info = DISTRIBUTION_INFO.load(deps.storage, round)?;

    Ok(BiddingInfoResponse {
        bid_info,
        distribution_info,
    })
}

fn query_bids_by_user(deps: Deps, round: u64, user: Addr) -> StdResult<Vec<Bid>> {
    let bids_idx = BIDS_BY_USER.load(deps.storage, (round, user))?;

    let bids: Vec<Bid> = bids_idx
        .iter()
        .map(|idx| BID.load(deps.storage, *idx))
        .collect::<StdResult<_>>()?;

    Ok(bids)
}

fn query_all_bid_pool_in_round(deps: Deps, round: u64) -> StdResult<Vec<BidPool>> {
    let bid_info = BIDDING_INFO.load(deps.storage, round)?;

    bid_info.read_all_bid_pool(deps.storage)
}

fn query_estimate_amount_receive_of_bid(
    deps: Deps,
    round: u64,
    idx: u64,
    exchange_rate: Decimal,
) -> StdResult<EstimateAmountReceiveOfBidResponse> {
    let distribution_info = DISTRIBUTION_INFO.load(deps.storage, round)?;
    let config = CONFIG.load(deps.storage)?;
    let bid = BID.load(deps.storage, idx)?;
    let bidding_info = BIDDING_INFO.load(deps.storage, round)?;
    let mut bid_pools = bidding_info.read_all_bid_pool(deps.storage)?;
    let mut distribution_amount = distribution_info.total_distribution;

    process_calc_distribution_amount(&mut bid_pools, &mut distribution_amount, exchange_rate)?;

    let mut index_snapshot = vec![Decimal::zero(); config.max_slot as usize + 1];
    let mut receiver_per_token = vec![Decimal::zero(); config.max_slot as usize + 1];

    for bid_pool in bid_pools {
        index_snapshot[bid_pool.slot as usize] = bid_pool.index_snapshot;
        receiver_per_token[bid_pool.slot as usize] = bid_pool.received_per_token;
    }

    let amount_received =
        bid.amount * receiver_per_token[bid.premium_slot as usize] * Uint128::one();
    let residue_bid = bid.amount * (Decimal::one() - index_snapshot[bid.premium_slot as usize]);

    Ok(EstimateAmountReceiveOfBidResponse {
        receive: amount_received,
        residue_bid,
    })
}

fn query_estimate_amount_receive(
    deps: Deps,
    round: u64,
    slot: u8,
    bid_amount: Uint128,
    exchange_rate: Decimal,
) -> StdResult<EstimateAmountReceiveOfBidResponse> {
    let distribution_info = DISTRIBUTION_INFO.load(deps.storage, round)?;
    let config = CONFIG.load(deps.storage)?;
    let bidding_info = BIDDING_INFO.load(deps.storage, round)?;
    let mut distribution_amount = distribution_info.total_distribution;
    let mut bid_pools = bidding_info.read_all_bid_pool(deps.storage)?;
    for id in 0..bid_pools.len() {
        if bid_pools[id].slot == slot {
            bid_pools[id].total_bid_amount += bid_amount;
            break;
        }
    }

    process_calc_distribution_amount(&mut bid_pools, &mut distribution_amount, exchange_rate)?;

    let mut index_snapshot = vec![Decimal::zero(); config.max_slot as usize + 1];
    let mut receiver_per_token = vec![Decimal::zero(); config.max_slot as usize + 1];

    for bid_pool in bid_pools {
        index_snapshot[bid_pool.slot as usize] = bid_pool.index_snapshot;
        receiver_per_token[bid_pool.slot as usize] = bid_pool.received_per_token;
    }

    let amount_received = bid_amount * receiver_per_token[slot as usize] * Uint128::one();
    let residue_bid = bid_amount * (Decimal::one() - index_snapshot[slot as usize]);

    Ok(EstimateAmountReceiveOfBidResponse {
        receive: amount_received,
        residue_bid,
    })
}
#[entry_point]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {
    Ok(Response::default())
}
