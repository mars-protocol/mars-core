use astroport::DecimalCheckedOps;
use cosmwasm_std::{
    attr, entry_point, from_binary, to_binary, Addr, Binary, CosmosMsg, Decimal, Deps, DepsMut,
    Env, MessageInfo, Response, StdError, StdResult, Uint128, WasmMsg,
};
use cw20::{BalanceResponse, Cw20ExecuteMsg, Cw20QueryMsg, Cw20ReceiveMsg};

use crate::error::ContractError;
use crate::state::{Config, State, UserInfo, CONFIG, STATE, USERS};
use astroport::asset::addr_validate_to_lower;
use astroport::generator::{
    Cw20HookMsg as AstroGeneratorCw20HookMsg, ExecuteMsg as AstroGeneratorExecuteMsg,
    PendingTokenResponse, QueryMsg as AstroGeneratorQueryMsg,
};
use cw2::set_contract_version;
use mars_core::staking_proxy::{
    CallbackMsg, ConfigResponse, Cw20HookMsg, ExecuteMsg, ExecuteOnCallback, InstantiateMsg,
    MigrateMsg, QueryMsg, StateResponse, UserInfoResponse,
};

// version info for migration info
const CONTRACT_NAME: &str = "mars-lp-staking-proxy";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let config = Config {
        owner: msg.owner,
        redbank_addr: msg.redbank_addr,
        astro_generator_addr: msg.astro_generator_addr,
        redbank_treasury: msg.redbank_treasury,
        lp_token_addr: msg.token_addr,
        ma_token_addr: None,
        pool_addr: msg.pool_addr,
        astro_token: msg.astro_token,
        proxy_token: msg.proxy_token,
        astro_treasury_fee: Decimal::zero(),
        proxy_treasury_fee: Decimal::zero(),
    };

    let state = State {
        emergency_withdraw_executed: false,
        is_stakable: msg.is_stakable,
        total_underlying_tokens_staked: Uint128::zero(),
        total_ma_shares_staked: Uint128::zero(),
        astro_balance_before_claim: Uint128::zero(),
        proxy_balance_before_claim: Uint128::zero(),
        global_astro_per_ma_share_index: Decimal::zero(),
        global_proxy_per_ma_share_index: Decimal::zero(),
    };

    CONFIG.save(deps.storage, &config)?;
    STATE.save(deps.storage, &state)?;

    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    let cfg = CONFIG.load(deps.storage)?;

    match msg {
        ExecuteMsg::Receive(msg) => receive_cw20(deps, env, info, msg),
        ExecuteMsg::Withdraw {
            user_addr,
            ma_token_share,
            lp_token_amount,
            claim_rewards,
        } => {
            // Checks if the transaction is authorized
            if info.sender != cfg.redbank_addr {
                return Err(ContractError::Unauthorized {});
            }

            claim_rewards_and_execute(
                deps,
                env,
                ExecuteOnCallback::Unstake {
                    user_addr,
                    ma_token_share,
                    lp_token_amount,
                    claim_rewards,
                },
            )
        }
        ExecuteMsg::SetMaToken { ma_token_addr } => {
            if info.sender != cfg.redbank_addr {
                return Err(ContractError::Unauthorized {});
            }
            set_ma_token_address(deps, env, ma_token_addr)
        }
        ExecuteMsg::UpdateFee {
            astro_treasury_fee,
            proxy_treasury_fee,
        } => {
            // Checks if the transaction is authorized
            if info.sender != cfg.redbank_addr {
                return Err(ContractError::Unauthorized {});
            }
            claim_rewards_and_execute(
                deps,
                env,
                ExecuteOnCallback::UpdateFee {
                    astro_treasury_fee,
                    proxy_treasury_fee,
                },
            )
        }
        ExecuteMsg::UpdateOnTransfer {
            from_user_addr,
            to_user_addr,
            underlying_amount,
            ma_token_share,
        } => {
            // Checks if the transaction is authorized (Called by RB when checks are performend, Called by ma_token when Liquidation transfer is executed)
            if info.sender != cfg.redbank_addr || info.sender != cfg.ma_token_addr.unwrap() {
                return Err(ContractError::Unauthorized {});
            }

            claim_rewards_and_execute(
                deps,
                env,
                ExecuteOnCallback::UpdateOnTransfer {
                    from_user_addr,
                    to_user_addr,
                    underlying_amount,
                    ma_token_share,
                },
            )
        }
        ExecuteMsg::UnstakeBeforeBurn {
            user_address,
            ma_shares_to_burn,
        } => {
            // Checks if the transaction is authorized
            if info.sender != cfg.ma_token_addr.unwrap() {
                return Err(ContractError::Unauthorized {});
            }

            claim_rewards_and_execute(
                deps,
                env,
                ExecuteOnCallback::UnstakeBeforeBurn {
                    user_address,
                    ma_shares_to_burn,
                },
            )
        }
        ExecuteMsg::SwitchStakingStatus { is_staking_allowed } => {
            // Checks if the transaction is authorized
            if info.sender != cfg.owner {
                return Err(ContractError::Unauthorized {});
            }

            claim_rewards_and_execute(
                deps,
                env,
                ExecuteOnCallback::SwitchStakingStatus { is_staking_allowed },
            )
        }
        ExecuteMsg::EmergencyWithdraw {} => {
            // Checks if the transaction is authorized
            if info.sender != cfg.owner {
                return Err(ContractError::Unauthorized {});
            }

            claim_rewards_and_execute(deps, env, ExecuteOnCallback::EmergencyWithdraw {})
        }
        ExecuteMsg::Callback(msg) => handle_callback(deps, env, info, msg),
    }
}

pub fn handle_callback(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: CallbackMsg,
) -> Result<Response, ContractError> {
    // Callback functions can only be called this contract itself
    if info.sender != env.contract.address {
        return Err(ContractError::Unauthorized {});
    }
    match msg {
        CallbackMsg::UpdateIndexesAndExecute { execute_msg } => {
            update_indexes_and_execute(deps, env, execute_msg)
        }
        CallbackMsg::TransferLpTokensToRedBank {
            prev_lp_balance,
            lp_tokens_to_transfer,
        } => transfer_lp_tokens_to_rb(deps, env, prev_lp_balance, lp_tokens_to_transfer),
        CallbackMsg::TransferTokensAfterWithdraw {
            account,
            prev_balance,
        } => transfer_tokens_after_withdraw(deps, env, account, prev_balance),
    }
}

/// @dev Receives tokens sent by Red Bank MM contract
/// Stakes them with the AstroGenerator contract
fn receive_cw20(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    cw20_msg: Cw20ReceiveMsg,
) -> Result<Response, ContractError> {
    let cfg = CONFIG.load(deps.storage)?;

    match from_binary(&cw20_msg.msg)? {
        Cw20HookMsg::DepositWithProxy {
            user_addr,
            ma_token_share,
        } => {
            // Checks if the transaction is authorized
            if cw20_msg.sender != cfg.redbank_addr || info.sender != cfg.lp_token_addr {
                return Err(ContractError::Unauthorized {});
            }
            claim_rewards_and_execute(
                deps,
                env,
                ExecuteOnCallback::Stake {
                    user_addr,
                    ma_token_share,
                    lp_token_amount: cw20_msg.amount,
                },
            )
        }
    }
}

pub fn set_ma_token_address(
    deps: DepsMut,
    _env: Env,
    ma_token_addr: Addr,
) -> Result<Response, ContractError> {
    let mut cfg = CONFIG.load(deps.storage)?;
    cfg.ma_token_addr = Some(ma_token_addr.clone());

    Ok(Response::new().add_attributes(vec![
        attr("action", "LP_Staking_Proxy::ExecuteMsg::SetMaToken"),
        attr("ma_token_addr", ma_token_addr.to_string()),
    ]))
}

/// ## Description
/// Claims pending rewards on AstroGenerator (if any) and calls the `UpdateIndexesAndExecute` Callback function. Returns a [`ContractError`] on failure, otherwise returns a [`Response`] object with
/// the specified attributes.
///
/// ## Params
/// * **deps** is an object of type [`DepsMut`].
/// * **env** is an object of type [`Env`].
/// * **on_callback** is an object of type [`ExecuteOnCallback`]. This is the action to be performed after indexes are updated in callback.
fn claim_rewards_and_execute(
    deps: DepsMut,
    env: Env,
    on_callback: ExecuteOnCallback,
) -> Result<Response, ContractError> {
    let cfg = CONFIG.load(deps.storage)?;
    let mut state = STATE.load(deps.storage)?;

    let mut response = Response::new();

    // QUERY :: Check if there are any pending rewards claimable with AstroGenerator, and if so add Msg to claim them
    let pending_rewards: PendingTokenResponse = deps.querier.query_wasm_smart(
        &cfg.astro_generator_addr,
        &AstroGeneratorQueryMsg::PendingToken {
            lp_token: cfg.lp_token_addr.clone(),
            user: env.contract.address.clone(),
        },
    )?;

    // Msg :: If rewards are claimable on AstroGenerator, add ExecuteMsg to claim them
    if !pending_rewards.pending.is_zero()
        || (pending_rewards.pending_on_proxy.is_some()
            && !pending_rewards.pending_on_proxy.unwrap().is_zero())
    {
        // Store current ASTRO balance in the state for index calculation later
        let cur_astro_balance = {
            let res: BalanceResponse = deps.querier.query_wasm_smart(
                &cfg.astro_token,
                &Cw20QueryMsg::Balance {
                    address: env.contract.address.clone().to_string(),
                },
            )?;
            res.balance
        };
        state.astro_balance_before_claim = cur_astro_balance;

        // Store current PROXY balance in the state for index calculation later
        let cur_proxy_balance = {
            let res: BalanceResponse = deps.querier.query_wasm_smart(
                &cfg.proxy_token.unwrap(),
                &Cw20QueryMsg::Balance {
                    address: env.contract.address.to_string(),
                },
            )?;
            res.balance
        };
        state.proxy_balance_before_claim = cur_proxy_balance;

        response = response.add_message(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: cfg.astro_generator_addr.to_string(),
            funds: vec![],
            msg: to_binary(&AstroGeneratorExecuteMsg::Withdraw {
                lp_token: cfg.lp_token_addr,
                amount: Uint128::zero(),
            })?,
        }));

        STATE.save(deps.storage, &state)?;
    }

    // MSG :: Add CallbackMsg to Update Indexes (in-case rewards were claimed) and Execute staking / unstaking on AstroGenerator
    response = response.add_message(
        CallbackMsg::UpdateIndexesAndExecute {
            execute_msg: on_callback,
        }
        .to_cosmos_msg(&env)?,
    );

    Ok(response)
}

/// ## Description
/// Updates global indexes after rewards are claimed from AstroGenerator and then executes `ExecuteOnCallback` type function. Returns a [`ContractError`] on failure, otherwise returns a [`Response`] object with
/// the specified attributes.
///
/// ## Params
/// * **deps** is an object of type [`DepsMut`].
/// * **env** is an object of type [`Env`].
/// * **on_callback** is an object of type [`ExecuteOnCallback`]. This is the action to be performed after indexes are updated in callback.
fn update_indexes_and_execute(
    deps: DepsMut,
    env: Env,
    on_callback: ExecuteOnCallback,
) -> Result<Response, ContractError> {
    match on_callback {
        ExecuteOnCallback::Stake {
            user_addr,
            ma_token_share,
            lp_token_amount,
        } => stake_with_astro_generator(deps, env, user_addr, ma_token_share, lp_token_amount),
        ExecuteOnCallback::Unstake {
            user_addr,
            ma_token_share,
            lp_token_amount,
            claim_rewards,
        } => unstake_from_astro_generator(
            deps,
            env,
            user_addr,
            ma_token_share,
            lp_token_amount,
            claim_rewards,
        ),
        ExecuteOnCallback::UpdateFee {
            astro_treasury_fee,
            proxy_treasury_fee,
        } => update_fee(deps, env, astro_treasury_fee, proxy_treasury_fee),
        ExecuteOnCallback::UpdateOnTransfer {
            from_user_addr,
            to_user_addr,
            underlying_amount,
            ma_token_share,
        } => update_on_transfer(
            deps,
            env,
            from_user_addr,
            to_user_addr,
            underlying_amount,
            ma_token_share,
        ),
        ExecuteOnCallback::UnstakeBeforeBurn {
            user_address,
            ma_shares_to_burn,
        } => unstake_before_burn(deps, env, user_address, ma_shares_to_burn),
        ExecuteOnCallback::SwitchStakingStatus { is_staking_allowed } => {
            switch_staking_status(deps, env, is_staking_allowed)
        }
        ExecuteOnCallback::EmergencyWithdraw {} => execute_emergency_withdraw(deps, env),
    }
}

fn stake_with_astro_generator(
    mut deps: DepsMut,
    env: Env,
    user_addr: Addr,
    ma_token_share: Uint128,
    lp_token_amount: Uint128,
) -> Result<Response, ContractError> {
    let cfg = CONFIG.load(deps.storage)?;
    let mut state = STATE.load(deps.storage)?;
    let mut user_info = USERS.load(deps.storage, &user_addr).unwrap_or_default();

    let fee_msgs = update_rewards_per_share(deps.branch(), &env)?;

    // Check if staking is allowed or not
    if !state.is_stakable {
        return Err(ContractError::StakingNotAllowed {});
    }

    let mut response = Response::new();

    // Update global state and userInfo state
    update_user_rewards(user_info.clone(), &state)?;
    state.total_ma_shares_staked = state.total_ma_shares_staked.checked_add(ma_token_share)?;
    state.total_underlying_tokens_staked = state
        .total_underlying_tokens_staked
        .checked_add(lp_token_amount)?;

    user_info.underlying_tokens_staked = user_info
        .underlying_tokens_staked
        .checked_add(lp_token_amount)?;
    user_info.ma_tokens_staked = user_info.ma_tokens_staked.checked_add(ma_token_share)?;

    STATE.save(deps.storage, &state)?;
    USERS.save(deps.storage, &user_addr, &user_info)?;

    // Add fee transfer Msgs to Response
    if !fee_msgs.is_empty() {
        response = response.add_messages(fee_msgs);
    }

    response = response.add_message(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: cfg.lp_token_addr.to_string(),
        funds: vec![],
        msg: to_binary(&Cw20ExecuteMsg::Send {
            contract: cfg.astro_generator_addr.to_string(),
            amount: lp_token_amount,
            msg: to_binary(&AstroGeneratorCw20HookMsg::Deposit {})?,
        })?,
    }));

    Ok(response)
}

fn unstake_from_astro_generator(
    mut deps: DepsMut,
    env: Env,
    user_addr: Addr,
    ma_token_share: Uint128,
    lp_token_amount: Uint128,
    claim_rewards: bool,
) -> Result<Response, ContractError> {
    let cfg = CONFIG.load(deps.storage)?;
    let mut state = STATE.load(deps.storage)?;
    let mut user_info = USERS.load(deps.storage, &user_addr).unwrap_or_default();

    let fee_msgs = update_rewards_per_share(deps.branch(), &env)?;
    let mut response = Response::new();

    // Add fee transfer Msgs to Response
    if !fee_msgs.is_empty() {
        response = response.add_messages(fee_msgs);
    }

    // Update global state and userInfo state
    update_user_rewards(user_info.clone(), &state)?;

    state.total_ma_shares_staked = state.total_ma_shares_staked.checked_sub(ma_token_share)?;
    state.total_underlying_tokens_staked = state
        .total_underlying_tokens_staked
        .checked_sub(lp_token_amount)?;

    user_info.ma_tokens_staked = user_info.ma_tokens_staked.checked_sub(ma_token_share)?;
    user_info.underlying_tokens_staked = user_info
        .underlying_tokens_staked
        .checked_sub(lp_token_amount)?;

    if claim_rewards {
        // Transfer accrued ASTRO rewards to the user
        if !user_info.claimable_astro.is_zero() {
            response = response.add_message(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: cfg.astro_token.to_string(),
                funds: vec![],
                msg: to_binary(&Cw20ExecuteMsg::Transfer {
                    recipient: user_addr.to_string(),
                    amount: user_info.claimable_astro,
                })?,
            }));
            user_info.claimable_astro = Uint128::zero();
        }
        // Transfer accrued PROXY rewards to the user
        if !user_info.claimable_proxy.clone().is_zero() {
            response = response.add_message(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: cfg.proxy_token.unwrap().to_string(),
                funds: vec![],
                msg: to_binary(&Cw20ExecuteMsg::Transfer {
                    recipient: user_addr.to_string(),
                    amount: user_info.claimable_proxy,
                })?,
            }));
            user_info.claimable_proxy = Uint128::zero();
        }
    }

    STATE.save(deps.storage, &state)?;
    USERS.save(deps.storage, &user_addr, &user_info)?;

    // Unstake LP tokens from the AstroGenerator
    if !lp_token_amount.is_zero() {
        if !state.emergency_withdraw_executed {
            response = Response::new().add_message(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: cfg.astro_generator_addr.to_string(),
                funds: vec![],
                msg: to_binary(&AstroGeneratorExecuteMsg::Withdraw {
                    lp_token: cfg.lp_token_addr.clone(),
                    amount: lp_token_amount,
                })?,
            }));
        }

        // Current LP balance (to calculate how many LP tokens were withdrawn from the Generator contract)
        let cur_lp_balance = {
            let res: BalanceResponse = deps.querier.query_wasm_smart(
                &cfg.lp_token_addr,
                &Cw20QueryMsg::Balance {
                    address: env.contract.address.to_string(),
                },
            )?;
            res.balance
        };

        // MSG :: Add CallbackMsg to transfer unstaked LP Tokens to Red Bank
        response = response.add_message(
            CallbackMsg::TransferLpTokensToRedBank {
                prev_lp_balance: cur_lp_balance,
                lp_tokens_to_transfer: lp_token_amount,
            }
            .to_cosmos_msg(&env)?,
        );
    }

    Ok(response)
}

fn unstake_before_burn(
    mut deps: DepsMut,
    env: Env,
    user_addr: Addr,
    ma_shares_to_burn: Uint128,
) -> Result<Response, ContractError> {
    let cfg = CONFIG.load(deps.storage)?;
    let mut state = STATE.load(deps.storage)?;
    let mut user_info = USERS.load(deps.storage, &user_addr).unwrap_or_default();

    let fee_msgs = update_rewards_per_share(deps.branch(), &env)?;

    let mut response = Response::new();

    // Add fee transfer Msgs to Response
    if !fee_msgs.is_empty() {
        response = response.add_messages(fee_msgs);
    }

    // Update global state and userInfo state
    update_user_rewards(user_info.clone(), &state)?;

    // Get number of underlying tokens to be unstake and returned to the rd bank
    let underlying_tokens_to_unstake: Uint128 = deps.querier.query_wasm_smart(
        &cfg.redbank_addr,
        &mars_core::red_bank::msg::QueryMsg::UnderlyingLiquidityAmount {
            ma_token_address: cfg.lp_token_addr.to_string(),
            amount_scaled: ma_shares_to_burn,
        },
    )?;

    // Update state : Subtract ma_shares to be burnt
    state.total_ma_shares_staked = state
        .total_ma_shares_staked
        .checked_sub(ma_shares_to_burn)?;
    state.total_underlying_tokens_staked = state
        .total_underlying_tokens_staked
        .checked_sub(underlying_tokens_to_unstake)?;

    user_info.ma_tokens_staked = user_info.ma_tokens_staked.checked_sub(ma_shares_to_burn)?;
    user_info.underlying_tokens_staked = user_info
        .underlying_tokens_staked
        .checked_sub(underlying_tokens_to_unstake)?;

    // Transfer accrued ASTRO rewards to the user
    if !user_info.claimable_astro.is_zero() {
        response = response.add_message(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: cfg.astro_token.to_string(),
            funds: vec![],
            msg: to_binary(&Cw20ExecuteMsg::Transfer {
                recipient: user_addr.to_string(),
                amount: user_info.claimable_astro,
            })?,
        }));
        user_info.claimable_astro = Uint128::zero();
    }
    // Transfer accrued PROXY rewards to the user
    if !user_info.claimable_proxy.clone().is_zero() {
        response = response.add_message(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: cfg.proxy_token.unwrap().to_string(),
            funds: vec![],
            msg: to_binary(&Cw20ExecuteMsg::Transfer {
                recipient: user_addr.to_string(),
                amount: user_info.claimable_proxy,
            })?,
        }));
        user_info.claimable_proxy = Uint128::zero();
    }

    STATE.save(deps.storage, &state)?;
    USERS.save(deps.storage, &user_addr, &user_info)?;

    // Unstake LP tokens from the AstroGenerator
    if !underlying_tokens_to_unstake.is_zero() {
        // if emergency withdraw has been executed, we don't need to unstake from AstroGenerator
        if !state.emergency_withdraw_executed {
            response = Response::new().add_message(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: cfg.astro_generator_addr.to_string(),
                funds: vec![],
                msg: to_binary(&AstroGeneratorExecuteMsg::Withdraw {
                    lp_token: cfg.lp_token_addr.clone(),
                    amount: underlying_tokens_to_unstake,
                })?,
            }));
        }

        // Current LP balance (to calculate how many LP tokens were withdrawn from the Generator contract)
        let cur_lp_balance = {
            let res: BalanceResponse = deps.querier.query_wasm_smart(
                &cfg.lp_token_addr,
                &Cw20QueryMsg::Balance {
                    address: env.contract.address.to_string(),
                },
            )?;
            res.balance
        };

        // MSG :: Add CallbackMsg to transfer unstaked LP Tokens to Red Bank
        response = response.add_message(
            CallbackMsg::TransferLpTokensToRedBank {
                prev_lp_balance: cur_lp_balance,
                lp_tokens_to_transfer: underlying_tokens_to_unstake,
            }
            .to_cosmos_msg(&env)?,
        );
    }

    Ok(response)
}

fn switch_staking_status(
    mut deps: DepsMut,
    env: Env,
    is_staking_allowed: bool,
) -> Result<Response, ContractError> {
    let mut state = STATE.load(deps.storage)?;

    let fee_msgs = update_rewards_per_share(deps.branch(), &env)?;

    // Check if staking is allowed or not
    state.is_stakable = is_staking_allowed;

    let mut response = Response::new();

    STATE.save(deps.storage, &state)?;

    // Add fee transfer Msgs to Response
    if !fee_msgs.is_empty() {
        response = response.add_messages(fee_msgs);
    }
    Ok(response)
}

fn execute_emergency_withdraw(mut deps: DepsMut, env: Env) -> Result<Response, ContractError> {
    let cfg = CONFIG.load(deps.storage)?;
    let mut state = STATE.load(deps.storage)?;

    let fee_msgs = update_rewards_per_share(deps.branch(), &env)?;
    state.is_stakable = false;

    let mut response = Response::new();

    response = response.add_message(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: cfg.astro_generator_addr.to_string(),
        funds: vec![],
        msg: to_binary(&AstroGeneratorExecuteMsg::Withdraw {
            lp_token: cfg.lp_token_addr,
            amount: state.total_underlying_tokens_staked,
        })?,
    }));

    STATE.save(deps.storage, &state)?;

    // Add fee transfer Msgs to Response
    if !fee_msgs.is_empty() {
        response = response.add_messages(fee_msgs);
    }
    Ok(response)
}

/// @dev Admin function to update fee charged by Red Bank on the rewards
fn update_fee(
    mut deps: DepsMut,
    env: Env,
    astro_treasury_fee: Decimal,
    proxy_treasury_fee: Decimal,
) -> Result<Response, ContractError> {
    let mut cfg = CONFIG.load(deps.storage)?;
    let fee_msgs = update_rewards_per_share(deps.branch(), &env)?;

    cfg.astro_treasury_fee = astro_treasury_fee;
    cfg.proxy_treasury_fee = proxy_treasury_fee;

    CONFIG.save(deps.storage, &cfg)?;

    let mut response = Response::new()
        .add_attribute("action", "fee_updated")
        .add_attribute("astro_treasury_fee", astro_treasury_fee.to_string())
        .add_attribute("proxy_treasury_fee", proxy_treasury_fee.to_string());

    if !fee_msgs.is_empty() {
        response = response.add_messages(fee_msgs);
    }

    Ok(response)
}

fn update_on_transfer(
    mut deps: DepsMut,
    env: Env,
    from_user_addr: Addr,
    to_user_addr: Addr,
    underlying_amount: Uint128,
    ma_token_share: Uint128,
) -> Result<Response, ContractError> {
    let state = STATE.load(deps.storage)?;
    let mut from_user_info = USERS
        .load(deps.storage, &from_user_addr)
        .unwrap_or_default();
    let mut to_user_info = USERS.load(deps.storage, &to_user_addr).unwrap_or_default();

    let fee_msgs = update_rewards_per_share(deps.branch(), &env)?;
    let mut response = Response::new();

    // Update global state and from and to userInfo state
    update_user_rewards(from_user_info.clone(), &state)?;
    from_user_info.ma_tokens_staked = from_user_info
        .ma_tokens_staked
        .checked_sub(ma_token_share)?;
    to_user_info.ma_tokens_staked = to_user_info.ma_tokens_staked.checked_add(ma_token_share)?;

    from_user_info.underlying_tokens_staked = from_user_info
        .underlying_tokens_staked
        .checked_sub(underlying_amount)?;
    to_user_info.underlying_tokens_staked = to_user_info
        .underlying_tokens_staked
        .checked_add(underlying_amount)?;

    STATE.save(deps.storage, &state)?;
    USERS.save(deps.storage, &from_user_addr, &from_user_info)?;
    USERS.save(deps.storage, &to_user_addr, &to_user_info)?;

    // Add fee transfer Msgs to Response
    if !fee_msgs.is_empty() {
        response = response.add_messages(fee_msgs);
    }

    Ok(response)
}
pub fn transfer_tokens_after_withdraw(
    deps: DepsMut,
    env: Env,
    account: Addr,
    prev_balance: Uint128,
) -> Result<Response, ContractError> {
    let cfg = CONFIG.load(deps.storage)?;

    // Calculate number of LP Tokens withdrawn from the staking contract
    let amount = {
        let res: BalanceResponse = deps.querier.query_wasm_smart(
            &cfg.lp_token_addr,
            &Cw20QueryMsg::Balance {
                address: env.contract.address.to_string(),
            },
        )?;
        res.balance - prev_balance
    };

    Ok(Response::new().add_message(WasmMsg::Execute {
        contract_addr: cfg.lp_token_addr.to_string(),
        funds: vec![],
        msg: to_binary(&Cw20ExecuteMsg::Transfer {
            recipient: account.to_string(),
            amount,
        })?,
    }))
}

pub fn transfer_lp_tokens_to_rb(
    deps: DepsMut,
    env: Env,
    prev_lp_balance: Uint128,
    lp_tokens_to_transfer: Uint128,
) -> Result<Response, ContractError> {
    let cfg = CONFIG.load(deps.storage)?;
    let state = STATE.load(deps.storage)?;

    if !state.emergency_withdraw_executed {
        // Calculate number of LP Tokens withdrawn from the AstroGenerator contract
        let cur_lp_balance = {
            let res: BalanceResponse = deps.querier.query_wasm_smart(
                &cfg.lp_token_addr,
                &Cw20QueryMsg::Balance {
                    address: env.contract.address.to_string(),
                },
            )?;
            res.balance
        };

        Ok(Response::new().add_message(WasmMsg::Execute {
            contract_addr: cfg.lp_token_addr.to_string(),
            funds: vec![],
            msg: to_binary(&Cw20ExecuteMsg::Transfer {
                recipient: cfg.redbank_addr.to_string(),
                amount: cur_lp_balance.checked_sub(prev_lp_balance)?,
            })?,
        }))
    }
    // If emergency withdrawal has been executed, tokens are held by the proxy contract and we transfer them directly
    else {
        Ok(Response::new().add_message(WasmMsg::Execute {
            contract_addr: cfg.lp_token_addr.to_string(),
            funds: vec![],
            msg: to_binary(&Cw20ExecuteMsg::Transfer {
                recipient: cfg.redbank_addr.to_string(),
                amount: lp_tokens_to_transfer,
            })?,
        }))
    }
}

pub fn update_user_rewards(mut user_info: UserInfo, state: &State) -> StdResult<()> {
    // Update claimable ASTRO rewards
    let accrued_astro = state
        .global_astro_per_ma_share_index
        .checked_mul(user_info.ma_tokens_staked)?
        .checked_sub(
            user_info
                .user_astro_per_ma_share_index
                .checked_mul(user_info.ma_tokens_staked)?,
        )?;
    user_info.claimable_astro = user_info.claimable_astro.checked_add(accrued_astro)?;
    user_info.user_astro_per_ma_share_index = state.global_astro_per_ma_share_index;

    // Update claimable PROXY rewards
    let accrued_proxy = state
        .global_proxy_per_ma_share_index
        .checked_mul(user_info.ma_tokens_staked)?
        .checked_sub(
            user_info
                .user_proxy_per_ma_share_index
                .checked_mul(user_info.ma_tokens_staked)?,
        )?;
    user_info.claimable_proxy = user_info.claimable_proxy.checked_add(accrued_proxy)?;
    user_info.user_proxy_per_ma_share_index = state.global_proxy_per_ma_share_index;

    Ok(())
}

/// @dev : Calculates number of tokens (ASTRO, PROXY) claimed from AstroGenerator as rewards, calculates fee charged by RB, and updates the global share indexes for reward tokens
pub fn update_rewards_per_share(deps: DepsMut, env: &Env) -> Result<Vec<WasmMsg>, ContractError> {
    let cfg = CONFIG.load(deps.storage)?;
    let mut state = STATE.load(deps.storage)?;
    let mut msgs: Vec<WasmMsg> = vec![];

    // Check :: Total ma_tokens share should be non-zero
    if !state.total_ma_shares_staked.is_zero() {
        // Index Update :: Update ASTRO rewards per ma_share index
        let astro_balance = {
            let res: BalanceResponse = deps.querier.query_wasm_smart(
                &cfg.astro_token,
                &Cw20QueryMsg::Balance {
                    address: env.contract.address.to_string(),
                },
            )?;
            res.balance
        };
        let mut astro_rewards = astro_balance.checked_sub(state.astro_balance_before_claim)?;

        // If fee is charged on ASTRO rewards, deduct and transfer the fee (ASTRO Tokens) to MARS Treasury
        if !cfg.astro_treasury_fee.is_zero() {
            let astro_fee = astro_rewards * cfg.astro_treasury_fee;
            msgs.push(WasmMsg::Execute {
                contract_addr: cfg.astro_token.to_string(),
                funds: vec![],
                msg: to_binary(&Cw20ExecuteMsg::Transfer {
                    recipient: cfg.redbank_treasury.to_string(),
                    amount: astro_fee,
                })?,
            });
            astro_rewards = astro_rewards.checked_sub(astro_fee)?;
        }

        state.global_astro_per_ma_share_index = state.global_astro_per_ma_share_index
            + Decimal::from_ratio(astro_rewards, state.total_ma_shares_staked);

        // Index Update :: Update PROXY rewards per ma_share index
        if cfg.proxy_token.clone().is_some() {
            let proxy_balance = {
                let res: BalanceResponse = deps.querier.query_wasm_smart(
                    &cfg.proxy_token.clone().unwrap(),
                    &Cw20QueryMsg::Balance {
                        address: env.contract.address.to_string(),
                    },
                )?;
                res.balance
            };
            let mut proxy_rewards = proxy_balance.checked_sub(state.proxy_balance_before_claim)?;

            // If fee is charged on PROXY rewards, deduct and transfer the fee (ASTRO Tokens) to MARS Treasury
            if !cfg.proxy_treasury_fee.is_zero() {
                let proxy_fee = proxy_rewards * cfg.proxy_treasury_fee;
                msgs.push(WasmMsg::Execute {
                    contract_addr: cfg.proxy_token.unwrap().to_string(),
                    funds: vec![],
                    msg: to_binary(&Cw20ExecuteMsg::Transfer {
                        recipient: cfg.redbank_treasury.to_string(),
                        amount: proxy_fee,
                    })?,
                });
                proxy_rewards = proxy_rewards.checked_sub(proxy_fee)?;
            }

            state.global_proxy_per_ma_share_index = state.global_proxy_per_ma_share_index
                + Decimal::from_ratio(proxy_rewards, state.total_ma_shares_staked);
        }

        STATE.save(deps.storage, &state)?;
    }

    Ok(msgs)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&query_config(deps)?),
        QueryMsg::State {} => to_binary(&query_state(deps, env)?),
        QueryMsg::UserInfo { user_address } => {
            to_binary(&query_user_info(deps, env, user_address)?)
        }
    }
}

pub fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    let cfg = CONFIG.load(deps.storage)?;
    let state = STATE.load(deps.storage)?;

    Ok(ConfigResponse {
        redbank_addr: cfg.redbank_addr,
        astro_generator_addr: cfg.astro_generator_addr,
        redbank_treasury: cfg.redbank_treasury,
        lp_token_addr: cfg.lp_token_addr,
        ma_token_addr: cfg.ma_token_addr,
        pool_addr: cfg.pool_addr,
        astro_token: cfg.astro_token,
        astro_treasury_fee: cfg.astro_treasury_fee,
        proxy_token: cfg.proxy_token,
        proxy_treasury_fee: cfg.proxy_treasury_fee,
        is_stakable: state.is_stakable,
    })
}

pub fn query_state(deps: Deps, env: Env) -> StdResult<StateResponse> {
    let cfg = CONFIG.load(deps.storage)?;
    let mut state = STATE.load(deps.storage)?;

    // QUERY :: Check if there are any pending rewards claimable with AstroGenerator
    let pending_rewards: PendingTokenResponse = deps.querier.query_wasm_smart(
        &cfg.astro_generator_addr,
        &AstroGeneratorQueryMsg::PendingToken {
            lp_token: cfg.lp_token_addr,
            user: env.contract.address,
        },
    )?;

    // ASTRO rewards are claimable
    if !pending_rewards.pending.is_zero() {
        let mut astro_rewards = pending_rewards.pending;

        // If fee is charged, deduct the fee
        if !cfg.astro_treasury_fee.is_zero() {
            let astro_fee = astro_rewards * cfg.astro_treasury_fee;
            astro_rewards = astro_rewards.checked_sub(astro_fee)?;
        }

        state.global_astro_per_ma_share_index = state.global_astro_per_ma_share_index
            + Decimal::from_ratio(astro_rewards, state.total_ma_shares_staked);
    }

    // PROXY rewards are claimable
    if pending_rewards.pending_on_proxy.is_some()
        && !pending_rewards.pending_on_proxy.unwrap().is_zero()
    {
        let mut total_proxy_rewards = pending_rewards.pending_on_proxy.unwrap();

        // If fee is charged, deduct the fee
        if !cfg.proxy_treasury_fee.is_zero() {
            let proxy_fee = total_proxy_rewards * cfg.astro_treasury_fee;
            total_proxy_rewards = total_proxy_rewards.checked_sub(proxy_fee)?;
        }

        state.global_proxy_per_ma_share_index = state.global_proxy_per_ma_share_index
            + Decimal::from_ratio(total_proxy_rewards, state.total_ma_shares_staked);
    }

    Ok(StateResponse {
        emergency_withdraw_executed: state.emergency_withdraw_executed,
        is_stakable: state.is_stakable,
        total_ma_shares_staked: state.total_ma_shares_staked,
        total_underlying_tokens_staked: state.total_underlying_tokens_staked,
        global_astro_per_ma_share_index: state.global_astro_per_ma_share_index,
        global_proxy_per_ma_share_index: state.global_proxy_per_ma_share_index,
    })
}

pub fn query_user_info(deps: Deps, env: Env, user_address: Addr) -> StdResult<UserInfoResponse> {
    let cfg = CONFIG.load(deps.storage)?;
    let mut state = STATE.load(deps.storage)?;
    let mut user_info = USERS.load(deps.storage, &user_address).unwrap_or_default();

    let underlying_tokens_staked = deps.querier.query_wasm_smart(
        &cfg.redbank_addr,
        &mars_core::red_bank::msg::QueryMsg::UnderlyingLiquidityAmount {
            ma_token_address: cfg.lp_token_addr.to_string(),
            amount_scaled: user_info.ma_tokens_staked,
        },
    )?;

    if state.total_ma_shares_staked.is_zero() || user_info.ma_tokens_staked.is_zero() {
        return Ok(UserInfoResponse {
            ma_tokens_staked: user_info.ma_tokens_staked,
            underlying_tokens_staked: underlying_tokens_staked,
            claimable_astro: user_info.claimable_astro,
            claimable_proxy: user_info.claimable_proxy,
        });
    }

    // QUERY :: Check if there are any pending rewards claimable with AstroGenerator
    let pending_rewards: PendingTokenResponse = deps.querier.query_wasm_smart(
        &cfg.astro_generator_addr,
        &AstroGeneratorQueryMsg::PendingToken {
            lp_token: cfg.lp_token_addr,
            user: env.contract.address,
        },
    )?;

    // ASTRO rewards are claimable
    if !pending_rewards.pending.is_zero() {
        let mut astro_rewards = pending_rewards.pending;

        // If fee is charged, deduct the fee
        if !cfg.astro_treasury_fee.is_zero() {
            let astro_fee = astro_rewards * cfg.astro_treasury_fee;
            astro_rewards = astro_rewards.checked_sub(astro_fee)?;
        }

        state.global_astro_per_ma_share_index = state.global_astro_per_ma_share_index
            + Decimal::from_ratio(astro_rewards, state.total_ma_shares_staked);

        let add_accrued_astro = (state.global_astro_per_ma_share_index
            * user_info.ma_tokens_staked)
            .checked_sub(user_info.user_proxy_per_ma_share_index * user_info.ma_tokens_staked)?;
        user_info.claimable_astro = user_info.claimable_astro.checked_add(add_accrued_astro)?;
    }

    // PROXY rewards are claimable
    if pending_rewards.pending_on_proxy.is_some()
        && !pending_rewards.pending_on_proxy.unwrap().is_zero()
    {
        let mut total_proxy_rewards = pending_rewards.pending_on_proxy.unwrap();

        // If fee is charged, deduct the fee
        if !cfg.proxy_treasury_fee.is_zero() {
            let proxy_fee = total_proxy_rewards * cfg.astro_treasury_fee;
            total_proxy_rewards = total_proxy_rewards.checked_sub(proxy_fee)?;
        }

        state.global_proxy_per_ma_share_index = state.global_proxy_per_ma_share_index
            + Decimal::from_ratio(total_proxy_rewards, state.total_ma_shares_staked);

        let add_accrued_proxy = (state.global_proxy_per_ma_share_index
            * user_info.ma_tokens_staked)
            .checked_sub(user_info.user_proxy_per_ma_share_index * user_info.ma_tokens_staked)?;
        user_info.claimable_proxy = user_info.claimable_proxy.checked_add(add_accrued_proxy)?;
    }

    Ok(UserInfoResponse {
        ma_tokens_staked: user_info.ma_tokens_staked,
        underlying_tokens_staked: underlying_tokens_staked,
        claimable_astro: user_info.claimable_astro,
        claimable_proxy: user_info.claimable_proxy,
    })
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> StdResult<Response> {
    Ok(Response::default())
}
