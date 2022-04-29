use std::collections::HashMap;

use cosmwasm_std::{to_binary, Addr, QuerierResult};

use chainlink_terra::state::Round;

#[derive(Default)]
pub struct ChainlinkQuerier {
    /// maps asset contract address to decimals
    pub assets_decimals: HashMap<Addr, u8>,
    /// maps asset contract address to latest price (latest round data)
    pub assets_latest_round_data: HashMap<Addr, Round>,
}

impl ChainlinkQuerier {
    pub fn handle_query(
        &self,
        contract_addr: &Addr,
        query: chainlink_terra::msg::QueryMsg,
    ) -> QuerierResult {
        match query {
            chainlink_terra::msg::QueryMsg::Decimals {} => {
                match self.assets_decimals.get(&Addr::unchecked(contract_addr)) {
                    Some(decimals) => Ok(to_binary(decimals).into()).into(),
                    None => panic!("[mock]: no decimals for the contract"),
                }
            }

            chainlink_terra::msg::QueryMsg::LatestRoundData {} => {
                match self
                    .assets_latest_round_data
                    .get(&Addr::unchecked(contract_addr))
                {
                    Some(round) => Ok(to_binary(round).into()).into(),
                    None => panic!("[mock]: no price (latest round data) for the contract"),
                }
            }

            _ => panic!("[mock]: unimplemented"),
        }
    }
}
