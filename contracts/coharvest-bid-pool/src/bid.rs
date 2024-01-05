use cosmwasm_std::{
    to_json_binary, BankMsg, Coin, CosmosMsg, Decimal, DepsMut, Env, MessageInfo, Response,
    StdError, StdResult, Uint128, WasmMsg,
};
use cw20::Cw20ExecuteMsg;
use oraiswap::asset::AssetInfo;

use crate::{
    error::ContractError,
    helper::into_cosmos_msg,
    state::{
        pop_bid_idx, read_bids_by_round, read_or_create_bid_pool, store_bid, Bid, BidPool,
        BiddingInfo, DistributionInfo, BID, BIDDING_INFO, BID_POOL, CONFIG, DISTRIBUTION_INFO,
        LAST_ROUND_ID,
    },
};

pub fn execute_create_new_round(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    total_bid_threshold: Uint128,
    start_time: u64,
    end_time: u64,
    total_distribution: Uint128,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if config.owner != info.sender {
        return Err(ContractError::Unauthorized {});
    }

    let mut last_round = LAST_ROUND_ID.load(deps.storage)?;
    last_round += 1;

    let bidding_info = BiddingInfo {
        round: last_round,
        start_time,
        end_time,
        total_bid_amount: Uint128::zero(),
        total_bid_threshold,
        total_bid_matched: Uint128::zero(),
    };

    let distribution_info = DistributionInfo {
        total_distribution,
        exchange_rate: Decimal::zero(),
        is_released: false,
        actual_distributed: Uint128::zero(),
    };

    if !bidding_info.is_valid_duration(&env) {
        return Err(ContractError::InvalidBiddingTimeRange {});
    }

    // store_bid
    LAST_ROUND_ID.save(deps.storage, &last_round)?;
    BIDDING_INFO.save(deps.storage, last_round, &bidding_info)?;
    DISTRIBUTION_INFO.save(deps.storage, last_round, &distribution_info)?;

    Ok(Response::new().add_attributes(vec![
        ("action", "create_new_bidding_round"),
        ("round", &last_round.to_string()),
        ("start_time", &start_time.to_string()),
        ("end_time", &end_time.to_string()),
        ("total_bid_threshold", &total_bid_threshold.to_string()),
    ]))
}

pub fn execute_submit_bid(
    deps: DepsMut,
    env: Env,
    round: u64,
    premium_slot: u8,
    bidder: String,
    amount: Uint128,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if config.min_deposit_amount > amount {
        return Err(ContractError::Std(StdError::generic_err(format!(
            "Minimum deposit is {}, got {}",
            config.min_deposit_amount, amount
        ))));
    }
    // get bid pool info
    let mut bidding_info: BiddingInfo = BIDDING_INFO.load(deps.storage, round)?;

    if !bidding_info.opening(&env) {
        return Err(ContractError::BidNotOpen {});
    }

    let mut bid_pool = read_or_create_bid_pool(deps.storage, round, premium_slot)?;
    bidding_info.total_bid_amount += amount;
    bid_pool.total_bid_amount += amount;

    let bid_idx = pop_bid_idx(deps.storage)?;
    let bid = Bid {
        idx: bid_idx,
        round,
        timestamp: env.block.time.seconds(),
        premium_slot,
        bidder: deps.api.addr_validate(&bidder)?,
        amount,
        residue_bid: amount,
        amount_received: Uint128::zero(),
        is_distributed: false,
    };

    // store bid info
    BIDDING_INFO.save(deps.storage, round, &bidding_info)?;
    BID_POOL.save(deps.storage, (round, premium_slot), &bid_pool)?;
    store_bid(deps.storage, bid_idx, &bid)?;
    Ok(Response::new().add_attributes(vec![
        ("action", "submit_bid"),
        ("round", &round.to_string()),
        ("bidder", &bidder),
        ("bid_idx", &bid_idx.to_string()),
        ("premium_slot", &premium_slot.to_string()),
        ("amount", &amount.to_string()),
    ]))
}

pub fn execute_release_distribution_info(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    round: u64,
    exchange_rate: Decimal,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    if config.owner != info.sender {
        return Err(ContractError::Unauthorized {});
    }

    let mut bidding_info = BIDDING_INFO.load(deps.storage, round)?;

    // check bidding must be finished
    if !bidding_info.finished(&env) {
        return Err(ContractError::BidNotEnded {});
    }

    let mut distribution_info = DISTRIBUTION_INFO.load(deps.storage, round)?;
    if distribution_info.is_released {
        return Err(ContractError::Std(StdError::generic_err(format!(
            "round {} has been released",
            round
        ))));
    }

    distribution_info.exchange_rate = exchange_rate;
    distribution_info.is_released = true;
    let mut bid_pools = bidding_info.read_all_bid_pool(deps.storage)?;

    let mut distribution_amount = distribution_info.total_distribution;
    let total_matched =
        process_calc_distribution_amount(&mut bid_pools, &mut distribution_amount, exchange_rate)?;

    distribution_info.actual_distributed =
        distribution_info.total_distribution - distribution_amount;
    bidding_info.total_bid_matched = total_matched;

    for bid_pool in bid_pools {
        BID_POOL.save(deps.storage, (round, bid_pool.slot), &bid_pool)?;
    }

    DISTRIBUTION_INFO.save(deps.storage, round, &distribution_info)?;
    BIDDING_INFO.save(deps.storage, round, &bidding_info)?;

    let burn_msg: CosmosMsg = match config.underlying_token {
        AssetInfo::NativeToken { denom } => CosmosMsg::Bank(BankMsg::Burn {
            amount: vec![Coin {
                denom,
                amount: total_matched,
            }],
        }),
        AssetInfo::Token { contract_addr } => CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: contract_addr.to_string(),
            msg: to_json_binary(&Cw20ExecuteMsg::Burn {
                amount: total_matched,
            })?,
            funds: vec![],
        }),
    };

    // burn total_matched
    Ok(Response::new()
        .add_attributes(vec![
            ("action", "release_distribution_info"),
            ("exchange_rate", &exchange_rate.to_string()),
            ("total_matched", &total_matched.to_string()),
            (
                "actual_distributed",
                &distribution_info.actual_distributed.to_string(),
            ),
        ])
        .add_message(burn_msg))
}

pub fn execute_distribute(
    deps: DepsMut,
    round: u64,
    start_after: Option<u64>,
    limit: Option<u64>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let distribution_info = DISTRIBUTION_INFO.load(deps.storage, round)?;

    if !distribution_info.is_released {
        return Err(ContractError::BidNotEnded {});
    }

    let mut index_snapshot = vec![Decimal::zero(); config.max_slot as usize + 1];
    let mut receiver_per_token = vec![Decimal::zero(); config.max_slot as usize + 1];

    for slot in 1..=config.max_slot {
        if let Some(bid_pool) = BID_POOL.may_load(deps.storage, (round, slot))? {
            index_snapshot[slot as usize] = bid_pool.index_snapshot;
            receiver_per_token[slot as usize] = bid_pool.received_per_token;
        }
    }

    let bids_idx = read_bids_by_round(deps.storage, round, start_after, limit)?;
    let mut msgs: Vec<CosmosMsg> = vec![];

    for idx in bids_idx {
        // read bid
        let mut bid = BID.load(deps.storage, idx)?;
        if bid.is_distributed {
            continue;
        }

        let amount_received = index_snapshot[bid.premium_slot as usize]
            * receiver_per_token[bid.premium_slot as usize]
            * Uint128::one();
        let residue_bid = bid.amount * (Decimal::one() - index_snapshot[bid.premium_slot as usize]);

        if amount_received > Uint128::zero() {
            msgs.push(into_cosmos_msg(
                &config.underlying_token,
                bid.bidder.to_string(),
                amount_received,
            ));
        }

        if residue_bid > Uint128::zero() {
            msgs.push(into_cosmos_msg(
                &config.distribution_token,
                bid.bidder.to_string(),
                amount_received,
            ));
        }

        bid.amount_received = amount_received;
        bid.residue_bid = residue_bid;
        bid.is_distributed = true;

        BID.save(deps.storage, idx, &bid)?;
    }

    Ok(Response::new().add_attributes(vec![("action", "distribute")]))
}

pub fn process_calc_distribution_amount(
    bid_pools: &mut Vec<BidPool>,
    distribution_amount: &mut Uint128,
    exchange_rate: Decimal,
) -> StdResult<Uint128> {
    let mut total_matched = Uint128::zero();

    for bid_pool in bid_pools {
        if bid_pool.total_bid_amount.is_zero() {
            continue;
        }

        let desired_amount =
            bid_pool.total_bid_amount * exchange_rate * (Decimal::one() + bid_pool.premium_rate);

        let actual_amount = if desired_amount <= *distribution_amount {
            desired_amount
        } else {
            *distribution_amount
        };

        let index_snapshot = Decimal::from_ratio(desired_amount, actual_amount);
        let received_per_token = Decimal::from_ratio(desired_amount, bid_pool.total_bid_amount);

        total_matched += index_snapshot * bid_pool.total_bid_amount;
        *distribution_amount -= actual_amount;
        bid_pool.index_snapshot = index_snapshot;
        bid_pool.received_per_token = received_per_token;

        if distribution_amount.is_zero() {
            break;
        }
    }

    Ok(total_matched)
}
