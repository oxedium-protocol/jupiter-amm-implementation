use anchor_lang::AnchorDeserialize;
use pyth_solana_receiver_sdk::price_update::PriceUpdateV2;
use solana_sdk::{account::Account, program_pack::Pack};
use spl_token::state::Mint as SplMint;
use spl_token_2022::state::Mint as SplMint2022;
use anyhow::anyhow;

pub fn parse_mint_decimals(account: &Account) -> anyhow::Result<u8> {
    if account.owner == spl_token::ID {
        let mint = SplMint::unpack(&account.data)
            .map_err(|e| anyhow!("SPL mint unpack failed: {:?}", e))?;
        Ok(mint.decimals)
    } else if account.owner == spl_token_2022::ID {
        let mint = SplMint2022::unpack(&account.data)
            .map_err(|e| anyhow!("Token2022 mint unpack failed: {:?}", e))?;
        Ok(mint.decimals)
    } else {
        Err(anyhow!("account is not a token mint"))
    }
}

/// Parse a Pyth price account and return a u64 price scaled appropriately
pub fn parse_pyth_price(acc: &Account) -> anyhow::Result<u64> {
    // Try to deserialize the account data as a Pyth Price struct
    let price_data: &PriceUpdateV2 = &PriceUpdateV2::try_from_slice(acc.data.as_slice())
        .map_err(|e| anyhow!("Failed to parse Pyth price: {:?}", e))?;

    // Extract the raw aggregated price and the exponent
    let raw_price = price_data.price_message.price as u64; // u64, e.g., 135_000_000_000

    Ok(raw_price)
}
