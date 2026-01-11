use std::collections::{HashMap, HashSet};

use anchor_lang::system_program;
use anchor_lang::{prelude::AccountMeta, AnchorDeserialize};
use anyhow::{anyhow, Result};
use borsh::{BorshDeserialize, BorshSerialize};
use jupiter_amm_interface::{
    AccountMap, Amm, AmmContext, AmmLabel, AmmProgramIdToLabel, KeyedAccount, Quote, QuoteParams,
    Swap, SwapAndAccountMetas, SwapParams,
};
use pyth_solana_receiver_sdk::price_update::PriceUpdateV2;
use rust_decimal::Decimal;
use solana_sdk::{account::Account, pubkey::*};
use spl_associated_token_account::get_associated_token_address;

use crate::{
    components::compute_swap_math,
    states::{SwapIxData, Treasury, Vault},
    utils::{helpers::parse_mint_decimals, OXEDIUM_SEED, TREASURY_SEED, VAULT_SEED},
};

/// =======================================================
/// Program IDs
/// =======================================================

pub mod spl_token_swap_programs {
    use solana_sdk::pubkey::Pubkey;

    pub const OXEDIUM: Pubkey = Pubkey::from_str_const("oxe1SKL52HMLBDT2JQvdxscA1LbVc4EEwwSdNZcnDVH");
}

/// =======================================================
/// All possible mints
/// =======================================================

const ALL_POSSIBLE_MINTS: &[Pubkey] = &[
    Pubkey::from_str_const("So11111111111111111111111111111111111111112"),
    Pubkey::from_str_const("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"),
    // USDT - Pubkey::from_str_const("Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"),
    // USD1 - Pubkey::from_str_const("USD1ttGY1N17NEEHLmELoaybftRBUSErhqYiQzvEmuB"),
];

/// =======================================================
/// AMM state
/// =======================================================

pub struct OxediumAmm {
    pub key: Pubkey,
    pub label: String,
    pub program_id: Pubkey,

    /// vault PDA -> Vault
    pub vaults: HashMap<Pubkey, Vault>,

    /// treasury PDA
    pub treasury: Option<(Pubkey, Treasury)>,

    /// mint -> decimals
    pub decimals: HashMap<Pubkey, u8>,

    /// mint -> price
    pub prices: HashMap<Pubkey, u64>,
}

/// =======================================================
/// ProgramId → Label
/// =======================================================

impl AmmProgramIdToLabel for OxediumAmm {
    const PROGRAM_ID_TO_LABELS: &[(Pubkey, AmmLabel)] =
        &[(spl_token_swap_programs::OXEDIUM, "Oxedium")];
}

/// =======================================================
/// AMM implementation
/// =======================================================

impl Amm for OxediumAmm {
    /// ---------- Metadata ----------

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
        32
    }

    fn underlying_liquidities(&self) -> Option<HashSet<Pubkey>> {
        None
    }

    fn is_active(&self) -> bool {
        true
    }

    /// ---------- Lifecycle ----------

    fn from_keyed_account(_keyed: &KeyedAccount, _ctx: &AmmContext) -> Result<Self> {
        let program_id = spl_token_swap_programs::OXEDIUM;

        let treasury = Pubkey::find_program_address(
            &[OXEDIUM_SEED.as_bytes(), TREASURY_SEED.as_bytes()],
            &program_id,
        )
        .0;

        Ok(Self {
            key: treasury,
            label: "Oxedium".to_string(),
            program_id,
            vaults: HashMap::new(),
            treasury: None,
            decimals: HashMap::new(),
            prices: HashMap::new(),
        })
    }

    /// ---------- Accounts to load ----------

    fn get_accounts_to_update(&self) -> Vec<Pubkey> {
        let mut accounts = vec![];

        // treasury
        let treasury = Pubkey::find_program_address(
            &[OXEDIUM_SEED.as_bytes(), TREASURY_SEED.as_bytes()],
            &self.program_id,
        )
        .0;
        accounts.push(treasury);

        // vaults + mint decimals
        for mint in ALL_POSSIBLE_MINTS {
            let vault = Pubkey::find_program_address(
                &[VAULT_SEED.as_bytes(), mint.as_ref()],
                &self.program_id,
            )
            .0;

            accounts.push(vault);
            accounts.push(*mint);
        }

        // 🔥 dynamic: oracle accounts (only after vaults parsed)
        for vault in self.vaults.values() {
            accounts.push(vault.pyth_price_account);
        }

        accounts
    }

    /// ---------- Update ----------

    fn update(&mut self, account_map: &AccountMap) -> Result<()> {
        self.vaults.clear();
        self.decimals.clear();
        self.prices.clear();
        self.treasury = None;

        // 1️⃣ parse vaults + treasury
        for (pk, acc) in account_map.iter() {
            if acc.owner != self.program_id {
                continue;
            }

            if let Ok(vault) = Vault::try_from_slice(&acc.data) {
                self.vaults.insert(*pk, vault);
                continue;
            }

            if let Ok(treasury) = Treasury::try_from_slice(&acc.data) {
                self.treasury = Some((*pk, treasury));
                continue;
            }
        }

        // treasury is REQUIRED
        let (_, treasury) = match self.treasury.as_ref() {
            Some(t) => t,
            None => return Ok(()),
        };

        if treasury.stoptap {
            return Ok(());
        }

        // 2️⃣ mint decimals (best-effort)
        for vault in self.vaults.values() {
            if let Some(mint_acc) = account_map.get(&vault.token_mint) {
                if let Ok(dec) = parse_mint_decimals(mint_acc) {
                    self.decimals.insert(vault.token_mint, dec);
                }
            }
        }

        // 3️⃣ oracle prices (best-effort)
        for vault in self.vaults.values() {
            if let Some(oracle_acc) = account_map.get(&vault.pyth_price_account) {
                if let Ok(price) = parse_pyth_price(oracle_acc) {
                    self.prices.insert(vault.token_mint, price);
                }
            }
        }

        Ok(())
    }

    /// ---------- Quote ----------

    fn quote(&self, params: &QuoteParams) -> Result<Quote> {
        if !self.prices.contains_key(&params.input_mint)
            || !self.prices.contains_key(&params.output_mint)
        {
            return Err(anyhow!("oracle not ready"));
        }

        let treasury_fee_bps = self.treasury.as_ref().map(|(_, t)| t.fee_bps).unwrap_or(0);

        let vault_in = self
            .vaults
            .values()
            .find(|v| v.token_mint == params.input_mint)
            .ok_or_else(|| anyhow!("vault_in not found"))?;

        let vault_out = self
            .vaults
            .values()
            .find(|v| v.token_mint == params.output_mint)
            .ok_or_else(|| anyhow!("vault_out not found"))?;

        let price_in = *self
            .prices
            .get(&vault_in.token_mint)
            .ok_or_else(|| anyhow!("price_in missing"))?;

        let price_out = *self
            .prices
            .get(&vault_out.token_mint)
            .ok_or_else(|| anyhow!("price_out missing"))?;

        let in_decimals = *self.decimals.get(&vault_in.token_mint).unwrap_or(&0);
        let out_decimals = *self.decimals.get(&vault_out.token_mint).unwrap_or(&0);

        let result = compute_swap_math(
            params.amount,
            price_in,
            price_out,
            in_decimals,
            out_decimals,
            vault_in,
            vault_out,
            treasury_fee_bps,
            0,
        )?;

        let total_fee =
            result.lp_fee_amount + result.protocol_fee_amount + result.partner_fee_amount;

        let fee_pct = Decimal::from(total_fee) / Decimal::from(params.amount);

        Ok(Quote {
            in_amount: params.amount,
            out_amount: result.net_amount_out,
            fee_amount: total_fee,
            fee_mint: vault_out.token_mint,
            fee_pct,
        })
    }

    /// ---------- Swap ix ----------

    fn get_swap_and_account_metas(&self, params: &SwapParams) -> Result<SwapAndAccountMetas> {
        let treasury_pda = Pubkey::find_program_address(
            &[OXEDIUM_SEED.as_bytes(), TREASURY_SEED.as_bytes()],
            &self.program_id,
        )
        .0;

        let vault_in = Pubkey::find_program_address(
            &[VAULT_SEED.as_bytes(), params.source_mint.as_ref()],
            &self.program_id,
        )
        .0;

        let vault_out = Pubkey::find_program_address(
            &[VAULT_SEED.as_bytes(), params.destination_mint.as_ref()],
            &self.program_id,
        )
        .0;

        // instruction data
        const DISCRIMINATOR: [u8; 8] = [248, 198, 158, 145, 225, 117, 135, 200];
        let mut data = Vec::with_capacity(16);
        data.extend_from_slice(&DISCRIMINATOR);

        SwapIxData {
            amount_in: params.in_amount,
            partner_fee_bps: 0,
        }
        .serialize(&mut data)?;

        let metas = vec![
            AccountMeta::new(params.token_transfer_authority, true),
            AccountMeta::new_readonly(params.source_mint, false),
            AccountMeta::new_readonly(params.destination_mint, false),
            AccountMeta::new(vault_in, false),
            AccountMeta::new(vault_out, false),
            AccountMeta::new(params.source_token_account, false),
            AccountMeta::new(params.destination_token_account, false),
            AccountMeta::new(treasury_pda, false),
            AccountMeta::new(
                get_associated_token_address(&treasury_pda, &params.source_mint),
                false,
            ),
            AccountMeta::new(
                get_associated_token_address(&treasury_pda, &params.destination_mint),
                false,
            ),
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
            vaults: HashMap::new(),
            treasury: None,
            decimals: HashMap::new(),
            prices: HashMap::new(),
        })
    }
}

/// =======================================================
/// Stub Pyth parser
/// =======================================================

/// Parse a Pyth price account and return a u64 price scaled appropriately
fn parse_pyth_price(acc: &Account) -> Result<u64> {
    // Try to deserialize the account data as a Pyth Price struct
    let price_data: &PriceUpdateV2 = &PriceUpdateV2::try_from_slice(acc.data.as_slice())
        .map_err(|e| anyhow!("Failed to parse Pyth price: {:?}", e))?;

    // Extract the raw aggregated price and the exponent
    let raw_price = price_data.price_message.price as u64; // u64, e.g., 135_000_000_000

    Ok(raw_price)
}
