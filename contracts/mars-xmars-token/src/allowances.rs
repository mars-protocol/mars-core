use cosmwasm_std::{attr, Binary, DepsMut, Env, MessageInfo, Response, Uint128};
use cw20::Cw20ReceiveMsg;
use cw20_base::allowances::deduct_allowance;
use cw20_base::ContractError;

use crate::core;

pub fn execute_transfer_from(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    owner: String,
    recipient: String,
    amount: Uint128,
) -> Result<Response, ContractError> {
    let rcpt_addr = deps.api.addr_validate(&recipient)?;
    let owner_addr = deps.api.addr_validate(&owner)?;

    // deduct allowance before doing anything else have enough allowance
    deduct_allowance(deps.storage, &owner_addr, &info.sender, &env.block, amount)?;

    core::transfer(
        deps.storage,
        &env,
        Some(&owner_addr),
        Some(&rcpt_addr),
        amount,
    )?;

    let res = Response::new().add_attributes(vec![
        attr("action", "transfer_from"),
        attr("from", owner),
        attr("to", recipient),
        attr("by", info.sender),
        attr("amount", amount),
    ]);
    Ok(res)
}

pub fn execute_burn_from(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    owner: String,
    amount: Uint128,
) -> Result<Response, ContractError> {
    let owner_addr = deps.api.addr_validate(&owner)?;

    // deduct allowance before doing anything else have enough allowance
    deduct_allowance(deps.storage, &owner_addr, &info.sender, &env.block, amount)?;

    core::burn(deps.storage, &env, &owner_addr, amount)?;

    let res = Response::new().add_attributes(vec![
        attr("action", "burn_from"),
        attr("from", owner),
        attr("by", info.sender),
        attr("amount", amount),
    ]);
    Ok(res)
}

pub fn execute_send_from(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    owner: String,
    contract: String,
    amount: Uint128,
    msg: Binary,
) -> Result<Response, ContractError> {
    let rcpt_addr = deps.api.addr_validate(&contract)?;
    let owner_addr = deps.api.addr_validate(&owner)?;

    // deduct allowance before doing anything else have enough allowance
    deduct_allowance(deps.storage, &owner_addr, &info.sender, &env.block, amount)?;

    // move the tokens to the contract
    core::transfer(
        deps.storage,
        &env,
        Some(&owner_addr),
        Some(&rcpt_addr),
        amount,
    )?;

    let attrs = vec![
        attr("action", "send_from"),
        attr("from", &owner),
        attr("to", &contract),
        attr("by", &info.sender),
        attr("amount", amount),
    ];

    // create a send message
    let msg = Cw20ReceiveMsg {
        sender: info.sender.into(),
        amount,
        msg,
    }
    .into_cosmos_msg(contract)?;

    let res = Response::new().add_message(msg).add_attributes(attrs);
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{CosmosMsg, Deps, StdError, SubMsg, WasmMsg};
    use cw20::{AllowanceResponse, Cw20Coin, Expiration, TokenInfoResponse};
    use cw20_base::allowances::query_allowance;
    use cw20_base::contract::{query_balance, query_token_info};

    use crate::contract::{execute, instantiate, query_balance_at, query_total_supply_at};
    use crate::msg::{ExecuteMsg, InstantiateMsg};

    fn get_balance<T: Into<String>>(deps: Deps, address: T) -> Uint128 {
        query_balance(deps, address.into()).unwrap().balance
    }

    // this will set up the instantiation for other tests
    fn do_instantiate<T: Into<String>>(
        mut deps: DepsMut,
        addr: T,
        amount: Uint128,
    ) -> TokenInfoResponse {
        let instantiate_msg = InstantiateMsg {
            name: "Auto Gen".to_string(),
            symbol: "AUTO".to_string(),
            decimals: 3,
            initial_balances: vec![Cw20Coin {
                address: addr.into(),
                amount,
            }],
            mint: None,
            marketing: None,
        };
        let info = mock_info("creator", &[]);
        let env = mock_env();
        instantiate(deps.branch(), env, info, instantiate_msg).unwrap();
        query_token_info(deps.as_ref()).unwrap()
    }

    #[test]
    fn transfer_from_respects_limits() {
        let mut deps = mock_dependencies(&[]);
        let owner = String::from("addr0001");
        let spender = String::from("addr0002");
        let rcpt = String::from("addr0003");

        let start = Uint128::new(999999);
        do_instantiate(deps.as_mut(), &owner, start);

        // provide an allowance
        let allow1 = Uint128::new(77777);
        let msg = ExecuteMsg::IncreaseAllowance {
            spender: spender.clone(),
            amount: allow1,
            expires: None,
        };
        let info = mock_info(owner.as_ref(), &[]);
        let env = mock_env();
        execute(deps.as_mut(), env, info, msg).unwrap();

        // valid transfer of part of the allowance
        let transfer = Uint128::new(44444);
        let msg = ExecuteMsg::TransferFrom {
            owner: owner.clone(),
            recipient: rcpt.clone(),
            amount: transfer,
        };
        let info = mock_info(spender.as_ref(), &[]);
        let env = mock_env();
        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        assert_eq!(res.attributes[0], attr("action", "transfer_from"));
        // make sure money arrived
        assert_eq!(
            get_balance(deps.as_ref(), owner.clone()),
            start.checked_sub(transfer).unwrap()
        );
        assert_eq!(get_balance(deps.as_ref(), rcpt.clone()), transfer);
        assert_eq!(
            query_balance_at(deps.as_ref(), owner.clone(), env.block.height)
                .unwrap()
                .balance,
            start.checked_sub(transfer).unwrap()
        );
        assert_eq!(
            query_balance_at(deps.as_ref(), rcpt.clone(), env.block.height)
                .unwrap()
                .balance,
            transfer
        );

        // ensure it looks good
        let allowance = query_allowance(deps.as_ref(), owner.clone(), spender.clone()).unwrap();
        let expect = AllowanceResponse {
            allowance: allow1.checked_sub(transfer).unwrap(),
            expires: Expiration::Never {},
        };
        assert_eq!(expect, allowance);

        // cannot send more than the allowance
        let msg = ExecuteMsg::TransferFrom {
            owner: owner.clone(),
            recipient: rcpt.clone(),
            amount: Uint128::new(33443),
        };
        let info = mock_info(spender.as_ref(), &[]);
        let env = mock_env();
        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert!(matches!(err, ContractError::Std(StdError::Overflow { .. })));

        // let us increase limit, but set the expiration (default env height is 12_345)
        let info = mock_info(owner.as_ref(), &[]);
        let env = mock_env();
        let msg = ExecuteMsg::IncreaseAllowance {
            spender: spender.clone(),
            amount: Uint128::new(1000),
            expires: Some(Expiration::AtHeight(env.block.height)),
        };
        execute(deps.as_mut(), env, info, msg).unwrap();

        // we should now get the expiration error
        let msg = ExecuteMsg::TransferFrom {
            owner,
            recipient: rcpt,
            amount: Uint128::new(33443),
        };
        let info = mock_info(spender.as_ref(), &[]);
        let env = mock_env();
        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert_eq!(err, ContractError::Expired {});
    }

    #[test]
    fn burn_from_respects_limits() {
        let mut deps = mock_dependencies(&[]);
        let owner = String::from("addr0001");
        let spender = String::from("addr0002");

        let start = Uint128::new(999999);
        do_instantiate(deps.as_mut(), &owner, start);

        // provide an allowance
        let allow1 = Uint128::new(77777);
        let msg = ExecuteMsg::IncreaseAllowance {
            spender: spender.clone(),
            amount: allow1,
            expires: None,
        };
        let info = mock_info(owner.as_ref(), &[]);
        let env = mock_env();
        execute(deps.as_mut(), env, info, msg).unwrap();

        // valid burn of part of the allowance
        let transfer = Uint128::new(44444);
        let msg = ExecuteMsg::BurnFrom {
            owner: owner.clone(),
            amount: transfer,
        };
        let info = mock_info(spender.as_ref(), &[]);
        let env = mock_env();
        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        assert_eq!(res.attributes[0], attr("action", "burn_from"));

        // make sure money burnt
        assert_eq!(
            get_balance(deps.as_ref(), owner.clone()),
            start.checked_sub(transfer).unwrap()
        );
        assert_eq!(
            query_balance_at(deps.as_ref(), owner.clone(), env.block.height)
                .unwrap()
                .balance,
            start.checked_sub(transfer).unwrap()
        );
        assert_eq!(
            query_total_supply_at(deps.as_ref(), env.block.height)
                .unwrap()
                .total_supply,
            start.checked_sub(transfer).unwrap()
        );

        // ensure it looks good
        let allowance = query_allowance(deps.as_ref(), owner.clone(), spender.clone()).unwrap();
        let expect = AllowanceResponse {
            allowance: allow1.checked_sub(transfer).unwrap(),
            expires: Expiration::Never {},
        };
        assert_eq!(expect, allowance);

        // cannot burn more than the allowance
        let msg = ExecuteMsg::BurnFrom {
            owner: owner.clone(),
            amount: Uint128::new(33443),
        };
        let info = mock_info(spender.as_ref(), &[]);
        let env = mock_env();
        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert!(matches!(err, ContractError::Std(StdError::Overflow { .. })));

        // let us increase limit, but set the expiration (default env height is 12_345)
        let info = mock_info(owner.as_ref(), &[]);
        let env = mock_env();
        let msg = ExecuteMsg::IncreaseAllowance {
            spender: spender.clone(),
            amount: Uint128::new(1000),
            expires: Some(Expiration::AtHeight(env.block.height)),
        };
        execute(deps.as_mut(), env, info, msg).unwrap();

        // we should now get the expiration error
        let msg = ExecuteMsg::BurnFrom {
            owner,
            amount: Uint128::new(33443),
        };
        let info = mock_info(spender.as_ref(), &[]);
        let env = mock_env();
        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert_eq!(err, ContractError::Expired {});
    }

    #[test]
    fn send_from_respects_limits() {
        let mut deps = mock_dependencies(&[]);
        let owner = String::from("addr0001");
        let spender = String::from("addr0002");
        let contract = String::from("cool-dex");
        let send_msg = Binary::from(r#"{"some":123}"#.as_bytes());

        let start = Uint128::new(999999);
        do_instantiate(deps.as_mut(), &owner, start);

        // provide an allowance
        let allow1 = Uint128::new(77777);
        let msg = ExecuteMsg::IncreaseAllowance {
            spender: spender.clone(),
            amount: allow1,
            expires: None,
        };
        let info = mock_info(owner.as_ref(), &[]);
        let env = mock_env();
        execute(deps.as_mut(), env, info, msg).unwrap();

        // valid send of part of the allowance
        let transfer = Uint128::new(44444);
        let msg = ExecuteMsg::SendFrom {
            owner: owner.clone(),
            amount: transfer,
            contract: contract.clone(),
            msg: send_msg.clone(),
        };
        let info = mock_info(spender.as_ref(), &[]);
        let env = mock_env();
        let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        assert_eq!(res.attributes[0], attr("action", "send_from"));
        assert_eq!(1, res.messages.len());

        // we record this as sent by the one who requested, not the one who was paying
        let binary_msg = Cw20ReceiveMsg {
            sender: spender.clone(),
            amount: transfer,
            msg: send_msg.clone(),
        }
        .into_binary()
        .unwrap();
        assert_eq!(
            res.messages[0],
            SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: contract.clone(),
                msg: binary_msg,
                funds: vec![],
            }))
        );

        // make sure money sent
        assert_eq!(
            get_balance(deps.as_ref(), owner.clone()),
            start.checked_sub(transfer).unwrap()
        );
        assert_eq!(get_balance(deps.as_ref(), contract.clone()), transfer);
        assert_eq!(
            query_balance_at(deps.as_ref(), owner.clone(), env.block.height)
                .unwrap()
                .balance,
            start.checked_sub(transfer).unwrap()
        );
        assert_eq!(
            query_balance_at(deps.as_ref(), contract.clone(), env.block.height)
                .unwrap()
                .balance,
            transfer
        );

        // ensure it looks good
        let allowance = query_allowance(deps.as_ref(), owner.clone(), spender.clone()).unwrap();
        let expect = AllowanceResponse {
            allowance: allow1.checked_sub(transfer).unwrap(),
            expires: Expiration::Never {},
        };
        assert_eq!(expect, allowance);

        // cannot send more than the allowance
        let msg = ExecuteMsg::SendFrom {
            owner: owner.clone(),
            amount: Uint128::new(33443),
            contract: contract.clone(),
            msg: send_msg.clone(),
        };
        let info = mock_info(spender.as_ref(), &[]);
        let env = mock_env();
        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert!(matches!(err, ContractError::Std(StdError::Overflow { .. })));

        // let us increase limit, but set the expiration to current block (expired)
        let info = mock_info(owner.as_ref(), &[]);
        let env = mock_env();
        let msg = ExecuteMsg::IncreaseAllowance {
            spender: spender.clone(),
            amount: Uint128::new(1000),
            expires: Some(Expiration::AtHeight(env.block.height)),
        };
        execute(deps.as_mut(), env, info, msg).unwrap();

        // we should now get the expiration error
        let msg = ExecuteMsg::SendFrom {
            owner,
            amount: Uint128::new(33443),
            contract,
            msg: send_msg,
        };
        let info = mock_info(spender.as_ref(), &[]);
        let env = mock_env();
        let err = execute(deps.as_mut(), env, info, msg).unwrap_err();
        assert_eq!(err, ContractError::Expired {});
    }
}
