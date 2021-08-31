import {
  Coin,
  isTxError,
  LCDClient,
  MnemonicKey,
  Msg,
  MsgExecuteContract,
  MsgInstantiateContract,
  MsgMigrateContract,
  MsgStoreCode,
  StdTx,
  Wallet
} from '@terra-money/terra.js';
import { readFileSync } from 'fs';
import { CustomError } from 'ts-custom-error'

// Tequila lcd is load balanced, so txs can't be sent too fast, otherwise account sequence queries
// may resolve an older state depending on which lcd you end up with. Generally 1000 ms is is enough
// for all nodes to sync up.
let TIMEOUT = 1000

export function setTimeoutDuration(t: number) {
  TIMEOUT = t
}

export function getTimeoutDuration() {
  return TIMEOUT
}

export async function sleep(timeout: number) {
  await new Promise(resolve => setTimeout(resolve, timeout))
}

export class TransactionError extends CustomError {
  public constructor(
    public code: number,
    public codespace: string | undefined,
    public rawLog: string,
  ) {
    super("transaction failed")
  }
}

export async function createTransaction(wallet: Wallet, msg: Msg) {
  return await wallet.createTx({ msgs: [msg] })
}

export async function broadcastTransaction(terra: LCDClient, signedTx: StdTx) {
  const result = await terra.tx.broadcast(signedTx)
  await sleep(TIMEOUT)
  return result
}

export async function performTransaction(terra: LCDClient, wallet: Wallet, msg: Msg) {
  const tx = await createTransaction(wallet, msg)
  const signedTx = await wallet.key.signTx(tx)
  const result = await broadcastTransaction(terra, signedTx)
  if (isTxError(result)) {
    throw new TransactionError(result.code, result.codespace, result.raw_log)
  }
  return result
}

export async function uploadContract(terra: LCDClient, wallet: Wallet, filepath: string) {
  const contract = readFileSync(filepath, 'base64');
  const uploadMsg = new MsgStoreCode(wallet.key.accAddress, contract);
  let result = await performTransaction(terra, wallet, uploadMsg);
  return Number(result.logs[0].eventsByType.store_code.code_id[0]) // code_id
}

export async function instantiateContract(terra: LCDClient, wallet: Wallet, codeId: number, msg: object) {
  const instantiateMsg = new MsgInstantiateContract(wallet.key.accAddress, wallet.key.accAddress, codeId, msg, undefined);
  let result = await performTransaction(terra, wallet, instantiateMsg)
  const attributes = result.logs[0].events[0].attributes
  return attributes[attributes.length - 1].value // contract address
}

export async function executeContract(terra: LCDClient, wallet: Wallet, contractAddress: string, msg: object, coins?: string) {
  const executeMsg = new MsgExecuteContract(wallet.key.accAddress, contractAddress, msg, coins);
  return await performTransaction(terra, wallet, executeMsg);
}

export async function queryContract(terra: LCDClient, contractAddress: string, query: object): Promise<any> {
  return await terra.wasm.contractQuery(contractAddress, query)
}

export async function deployContract(terra: LCDClient, wallet: Wallet, filepath: string, initMsg: object) {
  const codeId = await uploadContract(terra, wallet, filepath);
  return await instantiateContract(terra, wallet, codeId, initMsg);
}

export async function migrate(terra: LCDClient, wallet: Wallet, contractAddress: string, newCodeId: number) {
  const migrateMsg = new MsgMigrateContract(wallet.key.accAddress, contractAddress, newCodeId, {});
  return await performTransaction(terra, wallet, migrateMsg);
}

export function recover(terra: LCDClient, mnemonic: string) {
  const mk = new MnemonicKey({ mnemonic: mnemonic });
  return terra.wallet(mk);
}

export function initialize(terra: LCDClient) {
  const mk = new MnemonicKey();

  console.log(`Account Address: ${mk.accAddress}`);
  console.log(`MnemonicKey: ${mk.mnemonic}`);

  return terra.wallet(mk);
}

export async function setupOracle(
  terra: LCDClient, wallet: Wallet, contractAddress: string, initialAssets: Asset[], oracleFactoryAddress: string, isTestnet: boolean
) {
  console.log("Setting up oracle assets...");

  for (let asset of initialAssets) {
    console.log(`Setting price source for ${asset.denom || asset.symbol || asset.contract_addr}`);

    let assetType
    let assetPriceSource

    if (asset.denom) {
      assetType = {
        "native": {
          "denom": asset.denom,
        }
      }
      assetPriceSource = assetType
    } else if (asset.contract_addr) {
      assetType = {
        "cw20": {
          "contract_addr": asset.contract_addr,
        }
      }

      const pairQueryMsg = {
        pair: {
          asset_infos: [
            {
              token: {
                contract_addr: asset.contract_addr,
              },
            },
            {
              native_token: {
                denom: "uusd",
              },
            },
          ],
        },
      }

      let pairQueryResponse
      try {
        pairQueryResponse = await queryContract(terra, oracleFactoryAddress, pairQueryMsg)
      } catch (error) {
        if (error.response.data.error.includes("PairInfoRaw not found")) {
          console.log("Pair not found, creating pair...");

          const createPairMsg = {
            "create_pair": {
              "asset_infos": [
                {
                  "token": {
                    "contract_addr": asset.contract_addr
                  }
                },
                {
                  "native_token": {
                    "denom": "uusd"
                  }
                }
              ]
            }
          }
          await executeContract(terra, wallet, oracleFactoryAddress, createPairMsg);
          console.log("Pair created");

          pairQueryResponse = await queryContract(terra, oracleFactoryAddress, pairQueryMsg)
        } else {
          console.log(error.response?.data?.error || "Error: pair contract query failed")
          continue
        }
      }

      if (!pairQueryResponse.contract_addr) {
        console.log("Error: something bad happened while trying to get oracle pairs contract address")
      }

      assetPriceSource = {
        "terraswap_uusd_pair": {
          "pair_address": pairQueryResponse.contract_addr
        }
      }
    } else {
      console.log(`INVALID ASSET: no denom or contract_addr`);
      return
    }

    let setAssetMsg = {
      "set_asset": {
        "asset": assetType,
        "price_source": assetPriceSource,
      },
    };

    await executeContract(terra, wallet, contractAddress, setAssetMsg);
    console.log(`Set ${asset.denom || asset.symbol || asset.contract_addr}`);
  }
}

export async function setupRedBank(terra: LCDClient, wallet: Wallet, contractAddress: string, options: any) {
  console.log("Setting up initial asset liquidity pools...");

  const initialAssets = options.initialAssets ?? [];
  const initialDeposits = options.initialDeposits ?? [];
  const initialBorrows = options.initialBorrows ?? [];

  for (let asset of initialAssets) {
    console.log(`Initializing ${asset.denom || asset.symbol || asset.contract_addr}`);

    let assetType = asset.denom
      ? {
        "native": {
          "denom": asset.denom,
        }
      }
      : asset.contract_addr
        ? {
          "cw20": {
            "contract_addr": asset.contract_addr,
          }
        }
        : undefined

    let initAssetMsg = {
      "init_asset": {
        "asset": assetType,
        "asset_params": asset.init_params,
      },
    };

    await executeContract(terra, wallet, contractAddress, initAssetMsg);
    console.log(`Initialized ${asset.denom || asset.symbol || asset.contract_addr}`);
  }

  for (let deposit of initialDeposits) {
    const { account, assets } = deposit;
    console.log(`### Deposits for account ${account.key.accAddress}: `);
    for (const asset of Object.keys(assets)) {
      const amount = assets[asset]
      const coins = new Coin(asset, amount);
      const depositMsg = { "deposit_native": { "denom": asset } };
      const executeDepositMsg = new MsgExecuteContract(account.key.accAddress, contractAddress, depositMsg, [coins]);
      await performTransaction(terra, account, executeDepositMsg);
      console.log(`Deposited ${amount} ${asset}`);
    }
  }

  for (let borrow of initialBorrows) {
    const { account, assets } = borrow;
    console.log(`### Borrows for account ${account.key.accAddress}: `);
    for (const asset of Object.keys(assets)) {
      const amount = assets[asset]
      const borrowMsg = {
        "borrow": {
          "asset": {
            "native": {
              "denom": asset
            }
          },
          "amount": amount.toString()
        }
      };
      const executeBorrowMsg = new MsgExecuteContract(account.key.accAddress, contractAddress, borrowMsg);
      await performTransaction(terra, account, executeBorrowMsg);
      console.log(`Borrowed ${amount} ${asset}`);
    }
  }
}

export function toEncodedBinary(object: any) {
  return Buffer.from(JSON.stringify(object)).toString('base64');
}
