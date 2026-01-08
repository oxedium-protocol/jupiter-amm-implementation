use std::collections::HashMap;
use anchor_lang::system_program;
use anchor_lang::prelude::AccountMeta;
use anyhow::{anyhow, Result};
use borsh::{BorshDeserialize, BorshSerialize};
use jupiter_amm_interface::{
    AccountMap, Amm, AmmContext, AmmLabel, AmmProgramIdToLabel, KeyedAccount, Quote, QuoteParams,
    Swap, SwapAndAccountMetas, SwapParams,
};

use rust_decimal::Decimal;
use solana_sdk::pubkey::Pubkey;
use spl_associated_token_account::get_associated_token_address;

use crate::{
    components::compute_swap_math,
    states::{SwapIxData, Treasury, Vault},
    utils::helpers::parse_mint_decimals,
};

pub mod spl_token_swap_programs {
    use solana_sdk::pubkey;

    use super::*;
    pub const OXEDIUM: Pubkey = pubkey!("oxe1hGoyJ41PATPA6ycEYMCyMXWZ33Xwo8rBK8vRCXQ");
}

pub struct OxediumAmm {
    pub label: String,
    pub key: Pubkey,
    pub vaults: HashMap<Pubkey, Vault>,
    pub treasury: Option<(Pubkey, Treasury)>,
    pub prices: HashMap<Pubkey, i64>,
    pub decimals: HashMap<Pubkey, u8>,
    pub program_id: Pubkey,
}

impl AmmProgramIdToLabel for OxediumAmm {
    const PROGRAM_ID_TO_LABELS: &[(Pubkey, AmmLabel)] = &[
        (spl_token_swap_programs::OXEDIUM, "Oxedium"),
    ];
}

impl Amm for OxediumAmm {
    fn label(&self) -> String {
        self.label.clone()
    }

    fn program_id(&self) -> Pubkey {
        self.program_id
    }

    fn key(&self) -> Pubkey {
        self.key
    }

    fn get_reserve_mints(&self) -> Vec<Pubkey> {
        self.vaults
            .values()
            .filter(|vault| vault.is_active)
            .map(|vault| vault.token_mint)
            .collect()
    }

    fn from_keyed_account(keyed_account: &KeyedAccount, _ctx: &AmmContext) -> Result<Self> {
        let program_id = keyed_account.account.owner;
        let amm_key = keyed_account.key;
        Ok(Self {
            key: amm_key,
            label: "Oxedium".to_string(),
            vaults: HashMap::default(),
            treasury: None,
            prices: HashMap::default(),
            decimals: HashMap::default(),
            program_id,
        })
    }

    fn update(&mut self, account_map: &AccountMap) -> anyhow::Result<()> {
        self.vaults.clear();
        self.prices.clear();
        self.decimals.clear();
        self.treasury = None;

        for (pk, acc) in account_map.iter() {
            if acc.owner != self.program_id {
                continue;
            }

            if let Ok(vault) = Vault::try_from_slice(&acc.data) {
                self.vaults.insert(*pk, vault.clone());

                self.prices.insert(vault.pyth_price_account, 0);

                if let Ok(dec) = parse_mint_decimals(
                    account_map
                        .get(&vault.token_mint)
                        .ok_or_else(|| anyhow!("mint account not found"))?,
                ) {
                    self.decimals.insert(vault.token_mint, dec);
                }
            }

            if let Ok(treasury) = Treasury::try_from_slice(&acc.data) {
                self.treasury = Some((*pk, treasury));
            }
        }

        Ok(())
    }

    fn quote(&self, params: &QuoteParams) -> Result<Quote> {
        let treasury = self
            .treasury
            .as_ref()
            .ok_or_else(|| anyhow!("treasury not found"))?;
        if treasury.1.stoptap {
            return Err(anyhow!("Oxedium AMM is currently disabled due to stoptap."));
        }

        let vault_in = self
            .vaults
            .values()
            .find(|v| v.token_mint == params.input_mint)
            .ok_or_else(|| anyhow!("vault_in not found for mint {}", params.input_mint))?;

        let vault_out = self
            .vaults
            .values()
            .find(|v| v.token_mint == params.output_mint)
            .ok_or_else(|| anyhow!("vault_out not found for mint {}", params.output_mint))?;

        let price_in = *self
            .prices
            .get(&vault_in.pyth_price_account)
            .ok_or_else(|| anyhow!("price_in not loaded"))? as u64;

        let price_out = *self
            .prices
            .get(&vault_out.pyth_price_account)
            .ok_or_else(|| anyhow!("price_out not loaded"))? as u64;

        // Берём decimals из internal state
        let in_decimals = *self
            .decimals
            .get(&vault_in.token_mint)
            .ok_or_else(|| anyhow!("in_decimals missing"))?;

        let out_decimals = *self
            .decimals
            .get(&vault_out.token_mint)
            .ok_or_else(|| anyhow!("out_decimals missing"))?;

        // --- swap math ---
        let result = compute_swap_math(
            params.amount,
            price_in,
            price_out,
            in_decimals,
            out_decimals,
            vault_in,
            vault_out,
            self.treasury.clone().unwrap().1.fee_bps,
            0,
        )
        .map_err(|e| anyhow!("compute_swap_math failed: {:?}", e))?;

        let total_fee =
            result.lp_fee_amount + result.protocol_fee_amount + result.partner_fee_amount;
        let fee_pct = Decimal::from(total_fee) / Decimal::from(params.amount);

        Ok(Quote {
            in_amount: params.amount,
            out_amount: result.net_amount_out,
            fee_amount: total_fee,
            fee_mint: vault_out.token_mint,
            fee_pct: fee_pct,
        })
    }

    fn get_swap_and_account_metas(&self, params: &SwapParams) -> Result<SwapAndAccountMetas> {
        let user = params.token_transfer_authority;
        let amount_in = params.in_amount;
        let partner_fee_bps = 0;

        let vault_in = self.vaults
            .iter()
            .find(|(_, v)| v.token_mint == params.source_mint && v.is_active)
            .ok_or_else(|| anyhow!("vault_in not found"))?;

        let vault_out = self.vaults
            .iter()
            .find(|(_, v)| v.token_mint == params.destination_mint && v.is_active)
            .ok_or_else(|| anyhow!("vault_out not found"))?;

        // ---------- Instruction data ----------
        const SWAP_DISCRIMINATOR: [u8; 8] = [248, 198, 158, 145, 225, 117, 135, 200];
        let mut ix_data = Vec::with_capacity(8 + 16);

        // discriminator
        ix_data.extend_from_slice(&SWAP_DISCRIMINATOR);

        // instruction args
        SwapIxData {
            amount_in: amount_in,
            partner_fee_bps: partner_fee_bps,
        }
        .serialize(&mut ix_data)
        .map_err(|e| anyhow!("borsh serialize failed: {}", e))?;

        let treasury_ata_in =
            get_associated_token_address(&self.treasury.clone().unwrap().0, &vault_in.1.token_mint);

        let treasury_ata_out =
            get_associated_token_address(&self.treasury.clone().unwrap().0, &vault_out.1.token_mint);

        // ---------- Accounts ----------
        let mut metas = vec![
            // signer
            AccountMeta::new(user, true),
            // mints
            AccountMeta::new_readonly(params.source_mint, false),
            AccountMeta::new_readonly(params.destination_mint, false),
            // pyth
            AccountMeta::new_readonly(vault_in.1.pyth_price_account, false),
            AccountMeta::new_readonly(vault_out.1.pyth_price_account, false),
            // user ATAs
            AccountMeta::new(params.source_token_account, false),
            AccountMeta::new(params.destination_token_account, false),
            // vaults
            AccountMeta::new(*vault_in.0, false),
            AccountMeta::new(*vault_out.0, false),
            // treasury
            AccountMeta::new(self.treasury.clone().unwrap().0, false),
            // treasury ATAs
            AccountMeta::new(treasury_ata_in, false),
            AccountMeta::new(treasury_ata_out, false),
        ];

        // optional partner fee ATA
        if let Some(ref quote_map) = params.quote_mint_to_referrer {
            if let Some(partner_ata) = quote_map.get(&vault_out.1.token_mint) {
                metas.push(AccountMeta::new(*partner_ata, false));
            } else {
                metas.push(AccountMeta::new_readonly(self.program_id, false));
            }
        } else {
            metas.push(AccountMeta::new_readonly(self.program_id, false));
        }

        // programs
        metas.extend_from_slice(&[
            AccountMeta::new_readonly(spl_associated_token_account::ID, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ]);

        Ok(SwapAndAccountMetas {
            swap: Swap::Oxedium,
            account_metas: metas,
        })
    }

    fn get_accounts_to_update(&self) -> Vec<Pubkey> {
        vec![]
    }

    fn clone_amm(&self) -> Box<dyn Amm + Send + Sync> {
        Box::new(Self {
            key: self.key,
            label: "Oxedium".to_string(),
            prices: HashMap::default(),
            decimals: HashMap::default(),
            vaults: HashMap::default(),
            treasury: None,
            program_id: self.program_id,
        })
    }

    fn has_dynamic_accounts(&self) -> bool {
        false
    }

    fn requires_update_for_reserve_mints(&self) -> bool {
        false
    }

    fn supports_exact_out(&self) -> bool {
        false
    }

    fn unidirectional(&self) -> bool {
        false
    }

    fn program_dependencies(&self) -> Vec<(Pubkey, String)> {
        vec![]
    }

    fn get_accounts_len(&self) -> usize {
        32
    }

    fn underlying_liquidities(&self) -> Option<std::collections::HashSet<Pubkey>> {
        None
    }

    fn is_active(&self) -> bool {
        true
    }
}
