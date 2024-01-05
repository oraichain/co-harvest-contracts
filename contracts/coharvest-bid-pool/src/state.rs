use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Decimal, Env, Order, StdResult, Storage, Uint128};
use cw_storage_plus::{Bound, Item, Map};
use oraiswap::asset::AssetInfo;

pub const CONFIG: Item<Config> = Item::new("config");
pub const BID_POOL: Map<(u64, u8), BidPool> = Map::new("bid_pool");
pub const BIDDING_INFO: Map<u64, BiddingInfo> = Map::new("bidding_info");
pub const LAST_ROUND_ID: Item<u64> = Item::new("last_round_id");
pub const BIDS_BY_USER: Map<(u64, Addr), Vec<u64>> = Map::new("bids_by_user");
pub const BIDS_BY_ROUND: Map<(u64, u64), bool> = Map::new("bids_by_round");
pub const BID: Map<u64, Bid> = Map::new("bid");
pub const BID_IDX: Item<u64> = Item::new("bid_idx");
pub const DISTRIBUTION_INFO: Map<u64, DistributionInfo> = Map::new("distribution_info");

const MAX_LIMIT: u64 = 1000;
const DEFAULT_LIMIT: u64 = 30;

#[cw_serde]
pub struct Config {
    pub owner: Addr,
    pub underlying_token: AssetInfo,
    pub distribution_token: AssetInfo,
    pub max_slot: u8,
    pub premium_rate_per_slot: Decimal,
    pub min_deposit_amount: Uint128,
}

#[cw_serde]
pub struct BiddingInfo {
    pub round: u64,
    pub start_time: u64,
    pub end_time: u64,
    pub total_bid_threshold: Uint128,
    pub total_bid_amount: Uint128,
    pub total_bid_matched: Uint128,
}

#[cw_serde]
pub struct DistributionInfo {
    pub total_distribution: Uint128,
    pub exchange_rate: Decimal,
    pub is_released: bool,
    pub actual_distributed: Uint128,
}

#[cw_serde]
pub struct BidPool {
    pub slot: u8,
    pub total_bid_amount: Uint128,
    pub premium_rate: Decimal,
    pub index_snapshot: Decimal,
    pub received_per_token: Decimal,
}

#[cw_serde]
pub struct Bid {
    pub idx: u64,
    pub round: u64,
    pub premium_slot: u8,
    pub bidder: Addr,
    pub amount: Uint128,
    pub residue_bid: Uint128,
    pub amount_received: Uint128,
    pub is_distributed: bool,
}

pub fn pop_bid_idx(storage: &mut dyn Storage) -> StdResult<u64> {
    let last_idx = BID_IDX.load(storage).unwrap_or_else(|_| 1);
    BID_IDX.save(storage, &(last_idx + 1))?;
    Ok(last_idx)
}

pub fn store_bid(storage: &mut dyn Storage, bid_idx: u64, bid: &Bid) -> StdResult<()> {
    BID.save(storage, bid_idx, &bid)?;
    BIDS_BY_USER.update(
        storage,
        (bid.round, bid.bidder.clone()),
        |idxs| -> StdResult<Vec<u64>> {
            let mut idxs = idxs.unwrap_or_default();
            idxs.push(bid_idx);
            Ok(idxs)
        },
    )?;
    BIDS_BY_ROUND.save(storage, (bid.round, bid_idx), &true)?;

    Ok(())
}

pub fn read_or_create_bid_pool(
    storage: &mut dyn Storage,
    round: u64,
    premium_slot: u8,
) -> StdResult<BidPool> {
    let config = CONFIG.load(storage)?;

    match BID_POOL.load(storage, (round, premium_slot)) {
        Ok(bid_pool) => Ok(bid_pool),
        Err(_) => {
            let bid_pool = BidPool {
                slot: premium_slot,
                premium_rate: config.premium_rate_per_slot
                    * Decimal::from_atomics(Uint128::from(premium_slot as u128), 0).unwrap(),
                total_bid_amount: Uint128::zero(),
                index_snapshot: Decimal::zero(),
                received_per_token: Decimal::zero(),
            };
            BID_POOL.save(storage, (round, premium_slot), &bid_pool)?;

            Ok(bid_pool)
        }
    }
}

pub fn read_bids_by_round(
    storage: &dyn Storage,
    round: u64,
    start_after: Option<u64>,
    limit: Option<u64>,
) -> StdResult<Vec<u64>> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT) as usize;

    let start = calc_range_start(start_after)?.map(Bound::ExclusiveRaw);
    BIDS_BY_ROUND
        .prefix(round)
        .range(storage, start, None, Order::Ascending)
        .take(limit)
        .map(|element| {
            let (idx, _) = element?;
            Ok(idx)
        })
        .collect()
}

impl BiddingInfo {
    pub fn is_valid_duration(&self, env: &Env) -> bool {
        return self.start_time < self.end_time && self.start_time >= env.block.time.seconds();
    }

    pub fn opening(&self, env: &Env) -> bool {
        return self.start_time <= env.block.time.seconds()
            && env.block.time.seconds() <= self.end_time;
    }

    pub fn finished(&self, env: &Env) -> bool {
        return self.end_time > env.block.time.seconds();
    }

    pub fn read_all_bid_pool(&self, storage: &dyn Storage) -> StdResult<Vec<BidPool>> {
        let config = CONFIG.load(storage)?;

        let bid_poolss: Vec<BidPool> = (1..=config.max_slot)
            .map(|slot| {
                BID_POOL
                    .load(storage, (self.round, slot))
                    .unwrap_or(BidPool {
                        slot,
                        total_bid_amount: Uint128::zero(),
                        premium_rate: config.premium_rate_per_slot
                            * Decimal::from_atomics(Uint128::from(slot as u128), 0).unwrap(),
                        index_snapshot: Decimal::zero(),
                        received_per_token: Decimal::zero(),
                    })
            })
            .collect();

        Ok(bid_poolss)
    }
}

//  this will set the first key after the provided key, by appending a 1 byte
fn calc_range_start(start_after: Option<u64>) -> StdResult<Option<Vec<u8>>> {
    match start_after {
        Some(start) => {
            let mut v: Vec<u8> = start.to_be_bytes().into();
            v.push(0);
            Ok(Some(v))
        }
        None => Ok(None),
    }
}
