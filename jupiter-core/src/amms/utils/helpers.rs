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
