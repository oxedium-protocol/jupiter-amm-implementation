use std::collections::HashSet;

use ahash::HashMap;
use anchor_lang::system_program;
use anchor_lang::{prelude::AccountMeta, AnchorDeserialize};
use anyhow::Result;
use borsh::BorshDeserialize;
use jupiter_amm_interface::{
    AccountMap, Amm, AmmContext, AmmLabel, AmmProgramIdToLabel, KeyedAccount, Quote, QuoteParams,
    Swap, SwapAndAccountMetas, SwapParams,
};
use pyth_solana_receiver_sdk::price_update::PriceUpdateV2;
use rust_decimal::Decimal;
use solana_sdk::program_pack::Pack;
use solana_sdk::pubkey::*;
use spl_associated_token_account::get_associated_token_address;
use spl_token::state::Mint;

use crate::states::Treasury;
use crate::{
    components::compute_swap_math,
    states::Vault,
    utils::{OXEDIUM_SEED, TREASURY_SEED, VAULT_SEED},
};

pub const OXEDIUM_PROGRAM_ID: Pubkey = Pubkey::from_str_const("oxe1SKL52HMLBDT2JQvdxscA1LbVc4EEwwSdNZcnDVH");

pub const SOL_MINT: Pubkey = Pubkey::from_str_const("So11111111111111111111111111111111111111112");
pub const USDC_MINT: Pubkey =
    Pubkey::from_str_const("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");

const ALL_POSSIBLE_MINTS: &[Pubkey] = &[SOL_MINT, USDC_MINT];

pub const MINT_ORACLES: &[(Pubkey, Pubkey)] = &[
    (SOL_MINT, Pubkey::from_str_const("7UVimffxr9ow1uXYxsr4LHAcV58mLzhmwaeKvJ1pjLiE")),
    (USDC_MINT, Pubkey::from_str_const("Dpw1EAVrSB1ibxiDQyTAW6Zip3J4Btk2x4SgApQCeFbX")),
];

pub struct OxediumAmm {
    pub key: Pubkey,
    pub label: String,
    pub vaults: HashMap<Pubkey, Vault>,
    pub mints: HashMap<Pubkey, Mint>,
    pub oracles: HashMap<Pubkey, PriceUpdateV2>,
    pub treasury: Treasury,
    pub program_id: Pubkey,
}

impl AmmProgramIdToLabel for OxediumAmm {
    const PROGRAM_ID_TO_LABELS: &[(Pubkey, AmmLabel)] = &[(OXEDIUM_PROGRAM_ID, "Oxedium")];
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
        15
    }

    fn underlying_liquidities(&self) -> Option<HashSet<Pubkey>> {
        None
    }

    fn is_active(&self) -> bool {
        true
    }

    fn from_keyed_account(keyed: &KeyedAccount, _ctx: &AmmContext) -> Result<Self> {
        Ok(Self {
            key: keyed.key,
            label: "Oxedium".to_string(),
            program_id: OXEDIUM_PROGRAM_ID,
            vaults: Default::default(),
            mints: Default::default(),
            oracles: Default::default(),
            treasury: Default::default(),
        })
    }

    fn get_accounts_to_update(&self) -> Vec<Pubkey> {
        let mut accounts = vec![];

        for mint in ALL_POSSIBLE_MINTS {
            let vault = Pubkey::find_program_address(
                &[VAULT_SEED.as_bytes(), mint.as_ref()],
                &self.program_id,
            )
            .0;
            accounts.push(vault);
            accounts.push(*mint);

            if let Some((_, oracle)) = MINT_ORACLES.iter().find(|(m, _)| m == mint) {
                accounts.push(*oracle);
            }
        }

        accounts
    }

    fn update(&mut self, account_map: &AccountMap) -> Result<()> {
        for mint in ALL_POSSIBLE_MINTS {
            // Vault PDA
            let vault_pda = Pubkey::find_program_address(
                &[VAULT_SEED.as_bytes(), mint.as_ref()],
                &self.program_id,
            )
            .0;

            if let Some(vault_account) = account_map.get(&vault_pda) {

                if vault_account.data.len() >= std::mem::size_of::<Vault>() {
                    if let Ok(vault) = Vault::deserialize(&mut &vault_account.data[8..]) {
                        self.vaults.insert(*mint, vault);
                    } else {
                        println!(">>> warning: failed to deserialize vault {:?}", vault_pda);
                    }
                } else {
                    println!(">>> warning: vault account data too small {:?}", vault_pda);
                }
            }

            if let Some(mint_account) = account_map.get(mint) {
                if mint_account.data.len() >= spl_token::state::Mint::LEN {
                    if let Ok(mint_data) = Mint::unpack(&mint_account.data) {
                        self.mints.insert(*mint, mint_data);
                    } else {
                        println!(">>> warning: failed to unpack mint {:?}", mint);
                    }
                } else {
                    println!(">>> warning: mint account data too small {:?}", mint);
                }
            }
        }

        for vault in self.vaults.values() {
            if let Some(oracle_account) = account_map.get(&vault.pyth_price_account) {
                if let Ok(price_data) = PriceUpdateV2::deserialize(&mut &oracle_account.data[8..]) {
                    self.oracles.insert(vault.pyth_price_account, price_data);
                } else {
                    println!(
                        ">>> warning: failed to deserialize oracle {:?}",
                        vault.pyth_price_account
                    );
                }
            }
        }
        Ok(())
    }

    fn quote(&self, params: &QuoteParams) -> Result<Quote> {

        let vault_in = self
            .vaults
            .get(&params.input_mint)
            .ok_or_else(|| anyhow::anyhow!("Vault for input mint not found"))?;

        let vault_out = self
            .vaults
            .get(&params.output_mint)
            .ok_or_else(|| anyhow::anyhow!("Vault for output mint not found"))?;

        let in_mint = self
            .mints
            .get(&params.input_mint)
            .ok_or_else(|| anyhow::anyhow!("Mint info for input not found"))?;
        let out_mint = self
            .mints
            .get(&params.output_mint)
            .ok_or_else(|| anyhow::anyhow!("Mint info for output not found"))?;

        let in_decimals = in_mint.decimals as u32;
        let out_decimals = out_mint.decimals as u32;

        let price_in_data = self
            .oracles
            .get(&vault_in.pyth_price_account)
            .ok_or_else(|| anyhow::anyhow!("Oracle for input mint not found"))?;
        let price_out_data = self
            .oracles
            .get(&vault_out.pyth_price_account)
            .ok_or_else(|| anyhow::anyhow!("Oracle for output mint not found"))?;

        let price_in = price_in_data.price_message.price as u64;
        let price_out = price_out_data.price_message.price as u64;

        let result = compute_swap_math(
            params.amount,
            price_in,
            price_out,
            in_decimals,
            out_decimals,
            vault_in,
            vault_out,
            0,
        )?;

        let total_fee = result.lp_fee_amount + result.protocol_fee_amount;

        let fee_pct = Decimal::from(total_fee) / Decimal::from(params.amount);

        Ok(Quote {
            in_amount: params.amount,
            out_amount: result.net_amount_out,
            fee_amount: total_fee,
            fee_mint: params.output_mint,
            fee_pct,
        })
    }

    fn get_swap_and_account_metas(&self, params: &SwapParams) -> Result<SwapAndAccountMetas> {
        let vault_in = self
            .vaults
            .get(&params.source_mint)
            .ok_or_else(|| anyhow::anyhow!("Vault for source mint not found"))?;
        let vault_out = self
            .vaults
            .get(&params.destination_mint)
            .ok_or_else(|| anyhow::anyhow!("Vault for destination mint not found"))?;

        let vault_in_pda = Pubkey::find_program_address(
            &[VAULT_SEED.as_bytes(), params.source_mint.as_ref()],
            &self.program_id,
        )
        .0;
        let vault_out_pda = Pubkey::find_program_address(
            &[VAULT_SEED.as_bytes(), params.destination_mint.as_ref()],
            &self.program_id,
        )
        .0;

        let treasury_pda = Pubkey::find_program_address(
            &[OXEDIUM_SEED.as_bytes(), TREASURY_SEED.as_bytes()],
            &self.program_id,
        )
        .0;
        let treasury_source_ata = get_associated_token_address(&treasury_pda, &params.source_mint);
        let treasury_destination_ata = get_associated_token_address(&treasury_pda, &params.destination_mint);

        let oracle_in = vault_in.pyth_price_account;
        let oracle_out = vault_out.pyth_price_account;

        let metas = vec![
            AccountMeta::new(params.token_transfer_authority, true),
            AccountMeta::new(params.source_mint, false),
            AccountMeta::new(params.destination_mint, false),
            AccountMeta::new(oracle_in, false),
            AccountMeta::new(oracle_out, false),
            AccountMeta::new(params.source_token_account, false),
            AccountMeta::new(params.destination_token_account, false),
            AccountMeta::new(vault_in_pda, false),
            AccountMeta::new(vault_out_pda, false),
            AccountMeta::new(treasury_pda, false),
            AccountMeta::new(treasury_source_ata, false),
            AccountMeta::new(treasury_destination_ata, false),
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
            vaults: self.vaults.clone(),
            mints: self.mints.clone(),
            oracles: self.oracles.clone(),
            treasury: self.treasury.clone(),
        })
    }
}
