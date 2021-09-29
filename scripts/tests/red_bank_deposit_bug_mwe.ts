/*
MWE for a bug in the red bank: depositing an asset after it has been borrowed fails.
Does not depend on which user makes the deposit, or the interest rate model used for the asset.

Error message:
```
failed to execute message; message index: 0: dispatch: Generic error: addr_validate errored: Input
is empty: execute wasm contract failed
```
*/

import {
  LCDClient,
  LocalTerra,
  Wallet
} from "@terra-money/terra.js"
import {
  deployContract,
  executeContract,
  queryContract,
  setTimeoutDuration,
  uploadContract
} from "../helpers.js"
import {
  borrowNative,
  depositNative,
  queryMaAssetAddress,
  setAssetOraclePriceSource,
} from "./test_helpers.js"

// CONSTS

const USD_COLLATERAL = 100_000_000_000000
const LUNA_COLLATERAL = 100_000_000_000000
const USD_BORROW = 100_000_000_000000
const MA_TOKEN_SCALING_FACTOR = 1_000_000

// HELPERS

async function checkCollateral(
  terra: LCDClient,
  wallet: Wallet,
  redBank: string,
  denom: string,
  enabled: boolean,
) {
  const collateral = await queryContract(terra, redBank,
    { user_collateral: { user_address: wallet.key.accAddress } }
  )

  for (const c of collateral.collateral) {
    if (c.denom == denom && c.enabled == enabled) {
      return true
    }
  }
  return false
}

// MAIN

async function main() {
  setTimeoutDuration(0)

  const terra = new LocalTerra()

  // addresses
  const deployer = terra.wallets.test1
  // mock contract addresses
  const protocolRewardsCollector = terra.wallets.test10.key.accAddress

  console.log("upload contracts")

  const addressProvider = await deployContract(terra, deployer, "../artifacts/address_provider.wasm",
    { owner: deployer.key.accAddress }
  )

  const incentives = await deployContract(terra, deployer, "../artifacts/incentives.wasm",
    {
      owner: deployer.key.accAddress,
      address_provider_address: addressProvider
    }
  )

  const oracle = await deployContract(terra, deployer, "../artifacts/oracle.wasm",
    { owner: deployer.key.accAddress }
  )

  const maTokenCodeId = await uploadContract(terra, deployer, "../artifacts/ma_token.wasm")

  const redBank = await deployContract(terra, deployer, "../artifacts/red_bank.wasm",
    {
      config: {
        owner: deployer.key.accAddress,
        address_provider_address: addressProvider,
        safety_fund_fee_share: "0.1",
        treasury_fee_share: "0.2",
        ma_token_code_id: maTokenCodeId,
        close_factor: "0.5",
      }
    }
  )

  await executeContract(terra, deployer, addressProvider,
    {
      update_config: {
        config: {
          owner: deployer.key.accAddress,
          incentives_address: incentives,
          oracle_address: oracle,
          red_bank_address: redBank,
          protocol_rewards_collector: protocolRewardsCollector,
          protocol_admin_address: deployer.key.accAddress,
        }
      }
    }
  )

  console.log("init assets")

  // uluna
  await executeContract(terra, deployer, redBank,
    {
      init_asset: {
        asset: { native: { denom: "uluna" } },
        asset_params: {
          initial_borrow_rate: "0.1",
          max_loan_to_value: "0.55",
          reserve_factor: "0.2",
          maintenance_margin: "0.65",
          liquidation_bonus: "0.1",
          interest_rate_strategy: {
            dynamic: {
              min_borrow_rate: "0.0",
              max_borrow_rate: "2.0",
              kp_1: "0.02",
              optimal_utilization_rate: "0.7",
              kp_augmentation_threshold: "0.15",
              kp_2: "0.05"
            }
          },
          active: true,
          deposit_enabled: true,
          borrow_enabled: true
        }
      }
    }
  )

  await setAssetOraclePriceSource(terra, deployer, oracle,
    { native: { denom: "uluna" } },
    25
  )

  //   const maUusd = await queryMaAssetAddress(terra, redBank, { native: { denom: "uluna" } })
  //   console.log(maUusd)


  // uusd
  await executeContract(terra, deployer, redBank,
    {
      init_asset: {
        asset: { native: { denom: "uusd" } },
        asset_params: {
          initial_borrow_rate: "0.2",
          max_loan_to_value: "0.75",
          reserve_factor: "0.2",
          maintenance_margin: "0.85",
          liquidation_bonus: "0.1",
          interest_rate_strategy: {
            dynamic: {
              min_borrow_rate: "0.0",
              max_borrow_rate: "1.0",
              kp_1: "0.04",
              optimal_utilization_rate: "0.9",
              kp_augmentation_threshold: "0.15",
              kp_2: "0.07"
            }
            // linear: {
            //   base: "1",
            //   slope_1: "0",
            //   slope_2: "0",
            //   optimal_utilization_rate: "1",
            // }
          },
          active: true,
          deposit_enabled: true,
          borrow_enabled: true
        }
      }
    }
  )

  await setAssetOraclePriceSource(terra, deployer, oracle,
    { native: { denom: "uusd" } },
    1
  )

  const maLuna = await queryMaAssetAddress(terra, redBank, { native: { denom: "uluna" } })

  const provider = terra.wallets.test2
  const borrower = terra.wallets.test3
  const recipient = terra.wallets.test4
  const someone = terra.wallets.test9

  console.log("provider provides USD")

  await depositNative(terra, provider, redBank, "uusd", USD_COLLATERAL)

  console.log("borrower provides Luna")

  await depositNative(terra, borrower, redBank, "uluna", LUNA_COLLATERAL)

  // TODO: uncommenting these two lines makes the test pass
  // await depositNative(terra, provider, redBank, "uusd", USD_COLLATERAL)
  // await depositNative(terra, provider, redBank, "uusd", USD_COLLATERAL)

  console.log("borrower borrows USD")

  await borrowNative(terra, borrower, redBank, "uusd", USD_BORROW)

  console.log("someone deposits USD")

  await depositNative(terra, someone, redBank, "uusd", USD_COLLATERAL)

  console.log("OK")
}

main().catch(err => console.log(err))