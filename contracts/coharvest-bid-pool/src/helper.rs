use cosmwasm_std::{to_json_binary, BankMsg, Coin, CosmosMsg, StdResult, Uint128, WasmMsg};
use cw20::Cw20ExecuteMsg;
use oraiswap::asset::AssetInfo;

pub fn into_cosmos_msg(
    asset_info: &AssetInfo,
    receiver: String,
    amount: Uint128,
) -> StdResult<CosmosMsg> {
    match asset_info {
        AssetInfo::Token { contract_addr } => Ok(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: contract_addr.to_string(),
            msg: to_json_binary(&Cw20ExecuteMsg::Transfer {
                recipient: receiver,
                amount,
            })?,
            funds: vec![],
        })),
        AssetInfo::NativeToken { denom } => Ok(CosmosMsg::Bank(BankMsg::Send {
            to_address: receiver,
            amount: vec![Coin {
                denom: denom.to_owned(),
                amount,
            }],
        })),
    }
}
