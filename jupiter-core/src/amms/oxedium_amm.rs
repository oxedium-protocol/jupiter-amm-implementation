use std::collections::HashSet;

use anchor_lang::prelude::AccountMeta;
use anchor_lang::system_program;
use anyhow::Result;
use jupiter_amm_interface::{
    AccountMap, Amm, AmmContext, AmmLabel, AmmProgramIdToLabel, KeyedAccount, Quote, QuoteParams, Swap, SwapAndAccountMetas, SwapParams
};
use rust_decimal::Decimal;
use solana_sdk::pubkey::*;
use spl_associated_token_account::get_associated_token_address;

use crate::{components::compute_swap_math, states::{Vault}, utils::{OXEDIUM_SEED, TREASURY_SEED, VAULT_SEED}
};

// =======================================================
// Program ID
// =======================================================
pub const OXEDIUM_PROGRAM_ID: Pubkey =
    Pubkey::from_str_const("oxe1SKL52HMLBDT2JQvdxscA1LbVc4EEwwSdNZcnDVH");

// =======================================================
// Supported mints
// =======================================================
pub const SOL_MINT: Pubkey = Pubkey::from_str_const("So11111111111111111111111111111111111111112");
pub const USDC_MINT: Pubkey = Pubkey::from_str_const("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");

const ALL_POSSIBLE_MINTS: &[Pubkey] = &[SOL_MINT, USDC_MINT];

// =======================================================
// AMM (STATEFUL)
// =======================================================
pub struct OxediumAmm {
    pub key: Pubkey,
    pub label: String,
    pub program_id: Pubkey,
}

// =======================================================
// ProgramId → Label
// =======================================================
impl AmmProgramIdToLabel for OxediumAmm {
    const PROGRAM_ID_TO_LABELS: &[(Pubkey, AmmLabel)] = &[(OXEDIUM_PROGRAM_ID, "Oxedium")];
}

// =======================================================
// AMM implementation
// =======================================================
impl Amm for OxediumAmm {
    // ---------- Metadata ----------
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
        ALL_POSSIBLE_MINTS.to_vec()
    }

    fn has_dynamic_accounts(&self) -> bool {
        true
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
        16
    }

    fn underlying_liquidities(&self) -> Option<HashSet<Pubkey>> {
        None
    }

    fn is_active(&self) -> bool {
        true
    }

    // ---------- Lifecycle ----------
    fn from_keyed_account(_keyed: &KeyedAccount, _ctx: &AmmContext) -> Result<Self> {
        println!(">>> from_keyed_account called");
        let program_id = OXEDIUM_PROGRAM_ID;
        let treasury = Pubkey::find_program_address(
            &[OXEDIUM_SEED.as_bytes(), TREASURY_SEED.as_bytes()],
            &program_id,
        )
        .0;

        Ok(Self {
            key: treasury,
            label: "Oxedium".to_string(),
            program_id,
        })
    }

    // ---------- Accounts to load ----------
    fn get_accounts_to_update(&self) -> Vec<Pubkey> {
        println!(">>> get_accounts_to_update called");
        let mut accounts = vec![];

        // load vaults + mints
        for mint in ALL_POSSIBLE_MINTS {
            let vault = Pubkey::find_program_address(
                &[VAULT_SEED.as_bytes(), mint.as_ref()],
                &self.program_id,
            )
            .0;
            accounts.push(vault);
            accounts.push(*mint);
        }

        accounts
    }

    // ---------- Update ----------
    fn update(&mut self, account_map: &AccountMap) -> Result<()> {
        println!(">>> update called, {} accounts loaded", account_map.len());

        Ok(())
    }

    // ---------- Quote ----------
    fn quote(&self, params: &QuoteParams) -> Result<Quote> {
        println!(">>> quote called");
        // get vaults
        let vault_in = &Vault { create_at_ts: 0, is_active: true, base_fee: 1, token_mint: params.input_mint, pyth_price_account: Pubkey::from_str_const("7UVimffxr9ow1uXYxsr4LHAcV58mLzhmwaeKvJ1pjLiE"), max_age_price: 300, lp_mint: Pubkey::from_str_const("59434gmHUQWvKuTrThNxzGHie7MGUw6nh51BCQPinwN8"), initial_liquidity: 1000000000000, current_liquidity: 1000000000000, max_liquidity: 1000000000000, cumulative_yield_per_lp: 0, protocol_yield: 0 };

        let vault_out = &Vault { create_at_ts: 0, is_active: true, base_fee: 1, token_mint: params.input_mint, pyth_price_account: Pubkey::from_str_const("Dpw1EAVrSB1ibxiDQyTAW6Zip3J4Btk2x4SgApQCeFbX"), max_age_price: 300, lp_mint: Pubkey::from_str_const("BkQnoZDBKBGuTUT9CQF3FiJKdi9NgnBbETYDsnEELzkD"), initial_liquidity: 1000000000000, current_liquidity: 1000000000000, max_liquidity: 1000000000000, cumulative_yield_per_lp: 0, protocol_yield: 0 };

        // get decimals
        let in_decimals = 9;
        let out_decimals = 6;

        // get prices
        let price_in = 14000000000;
        let price_out = 100000000;

        // compute swap
        let result = compute_swap_math(
            params.amount,
            price_in,
            price_out,
            in_decimals,
            out_decimals,
            vault_in,
            vault_out,
            0,
            0,
        )?;

        let total_fee = result.lp_fee_amount + result.protocol_fee_amount + result.partner_fee_amount;

        let fee_pct = Decimal::from(total_fee) / Decimal::from(params.amount);

        Ok(Quote {
            in_amount: params.amount,
            out_amount: result.net_amount_out,
            fee_amount: total_fee,
            fee_mint: params.output_mint,
            fee_pct,
        })
    }

    // ---------- Swap ix ----------
    fn get_swap_and_account_metas(&self, params: &SwapParams) -> Result<SwapAndAccountMetas> {
         println!(">>> get_swap_and_account_metas called: {} -> {} amount={}",
        params.source_mint, params.destination_mint, params.in_amount
        );
        let treasury_pda = Pubkey::find_program_address(
            &[OXEDIUM_SEED.as_bytes(), TREASURY_SEED.as_bytes()],
            &OXEDIUM_PROGRAM_ID,
        )
        .0;

        let vault_in = Pubkey::find_program_address(
            &[VAULT_SEED.as_bytes(), params.source_mint.as_ref()],
            &OXEDIUM_PROGRAM_ID,
        )
        .0;

        let vault_out = Pubkey::find_program_address(
            &[VAULT_SEED.as_bytes(), params.destination_mint.as_ref()],
            &OXEDIUM_PROGRAM_ID,
        )
        .0;

        let treasury_source_ata = get_associated_token_address(&treasury_pda, &params.source_mint);
        let treasury_destination_ata = get_associated_token_address(&treasury_pda, &params.destination_mint);

        // const DISCRIMINATOR: [u8; 8] = [248, 198, 158, 145, 225, 117, 135, 200];
        // let mut data = Vec::with_capacity(16);
        // data.extend_from_slice(&DISCRIMINATOR);

        // SwapIxData {
        //     amount_in: params.in_amount,
        //     partner_fee_bps: 0,
        // }
        // .serialize(&mut data)?;

        let metas = vec![
            AccountMeta::new(params.token_transfer_authority, false),
            AccountMeta::new(params.source_mint, false),
            AccountMeta::new(params.destination_mint, false),
            AccountMeta::new(Pubkey::from_str_const("7UVimffxr9ow1uXYxsr4LHAcV58mLzhmwaeKvJ1pjLiE"), false),
            AccountMeta::new(Pubkey::from_str_const("Dpw1EAVrSB1ibxiDQyTAW6Zip3J4Btk2x4SgApQCeFbX"), false),

            AccountMeta::new(params.source_token_account, false),
            AccountMeta::new(params.destination_token_account, false),

            AccountMeta::new(vault_in, false),
            AccountMeta::new(vault_out, false),
            AccountMeta::new(treasury_pda, false),
            AccountMeta::new(treasury_source_ata, false),
            AccountMeta::new(treasury_destination_ata, false),

            AccountMeta::new(OXEDIUM_PROGRAM_ID, false),

            AccountMeta::new_readonly(spl_associated_token_account::ID, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(system_program::ID, false),
        ];

        Ok(SwapAndAccountMetas {
            swap: Swap::Oxedium,
            account_metas: metas,
        })
    }

    fn clone_amm(&self) -> Box<dyn Amm + Send + Sync> {
        Box::new(Self {
            key: self.key,
            label: self.label.clone(),
            program_id: self.program_id,
        })
    }
}

// =======================================================
// Helper: parse Pyth price account
// =======================================================
// fn parse_pyth_price(acc: &solana_sdk::account::Account) -> Result<u64> {
//     use pyth_solana_receiver_sdk::price_update::PriceUpdateV2;

//     let price_data: &PriceUpdateV2 = &PriceUpdateV2::try_from_slice(acc.data.as_slice())
//         .map_err(|e| anyhow!("Failed to parse Pyth price: {:?}", e))?;

//     Ok(price_data.price_message.price as u64)
// }
