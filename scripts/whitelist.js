import 'dotenv/config.js';
import {queryContract} from "./helpers.mjs";
import {LCDClient, LocalTerra} from "@terra-money/terra.js";
import {writeFileSync} from 'fs';

async function main() {
  let terra;
  let lpContractAddress = process.env.LP_ADDRESS;

  if (process.env.NETWORK === "testnet") {
    terra = new LCDClient({
      URL: 'https://tequila-lcd.terra.dev',
      chainID: 'tequila-0004'
    })
  } else {
    terra = new LocalTerra();
  }

  const reservesListResult = await queryContract(terra, lpContractAddress, {"reserves_list": {}});
  const { reserves_list } = reservesListResult;
  const reserveInfo = {};

  for (let reserve of reserves_list) {
    const {denom, ma_token_address} = reserve;
    reserveInfo[ma_token_address] = {denom}
  }

  const output = {};
  output.contracts = {lpContractAddress};
  output.whitelist = reserveInfo;

  const json = JSON.stringify(output);
  writeFileSync('artifacts/whitelist.json', json, {'encoding': 'utf8'});
}

main().catch(err => console.log(err));