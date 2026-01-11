#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use jupiter_core::{oxedium_amm::OxediumAmm, states::{Treasury, Vault}};
    use solana_sdk::pubkey::Pubkey;
    use jupiter_amm_interface::{Amm, QuoteParams, Swap, SwapMode, SwapParams};

    #[test]
    fn test_oxedium_amm_quote_direct() {
        let vault_in_pubkey = Pubkey::new_unique();
        let vault_out_pubkey = Pubkey::new_unique();
        let token_in = Pubkey::from_str_const("So11111111111111111111111111111111111111112");
        let token_out = Pubkey::from_str_const("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
        let pyth_in = Pubkey::new_unique();
        let pyth_out = Pubkey::new_unique();

        let vault_in = Vault {
            token_mint: token_in,
            pyth_price_account: pyth_in,
            create_at_ts: 1,
            is_active: true,
            base_fee: 1,
            max_age_price: 300,
            lp_mint: Pubkey::new_unique(),
            initial_liquidity: 1000000000000,
            current_liquidity: 1000000000000,
            max_liquidity: 1000000000000,
            cumulative_yield_per_lp: 0,
            protocol_yield: 0,
        };

        let vault_out = Vault {
            token_mint: token_out,
            pyth_price_account: pyth_out,
            create_at_ts: 1,
            is_active: true,
            base_fee: 1,
            max_age_price: 300,
            lp_mint: Pubkey::new_unique(),
            initial_liquidity: 1000000000000,
            current_liquidity: 1000000000000,
            max_liquidity: 1000000000000,
            cumulative_yield_per_lp: 0,
            protocol_yield: 0,
        };

        let treasury = Treasury {
            fee_bps: 1, // 0.01% fee
            stoptap: false,
            admin: Pubkey::new_unique(),
        };

        let mut amm = OxediumAmm {
            key: Pubkey::new_unique(),
            label: "Oxedium".to_string(),
            treasury: Some((Pubkey::new_unique(), treasury)),
            vaults: HashMap::from([
                (vault_in_pubkey, vault_in),
                (vault_out_pubkey, vault_out),
            ]),
            prices: HashMap::default(),
            decimals: HashMap::default(),
            program_id: Pubkey::new_unique(),
        };

        amm.prices.insert(token_in, 13500000000);   // price_in (e.g., $135)
        amm.prices.insert(token_out, 100000000);  // price_out (e.g., $1)

        amm.decimals.insert(token_in, 9);   // e.g., SOL decimals
        amm.decimals.insert(token_out, 6);

        let params = QuoteParams {
            amount: 1_000_000_000, // 1 token_in (with 9 decimals)

            input_mint: token_in,
            output_mint: token_out,
            swap_mode: SwapMode::ExactIn,
        };

        let quote = amm.quote(&params).unwrap();

        println!("Quote: {:?}", quote);

        assert_eq!(quote.in_amount, params.amount);
        assert!(quote.out_amount > 0);           // Ensure output > 0
        assert!(quote.fee_amount > 0);           // Ensure fees > 0
        assert_eq!(quote.fee_mint, token_out);   // Fee is taken in output token
    }

    #[test]
     fn test_oxedium_amm_get_swap_accounts() {
        let vault_in_pubkey = Pubkey::new_unique();
        let vault_out_pubkey = Pubkey::new_unique();
        let token_in = Pubkey::from_str_const("So11111111111111111111111111111111111111112");
        let token_out = Pubkey::from_str_const("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
        let pyth_in = Pubkey::new_unique();
        let pyth_out = Pubkey::new_unique();

        let vault_in = Vault {
            token_mint: token_in,
            pyth_price_account: pyth_in,
            create_at_ts: 0,
            is_active: true,
            base_fee: 0,
            max_age_price: 0,
            lp_mint: Pubkey::new_unique(),
            initial_liquidity: 1_000_000,
            current_liquidity: 1_000_000,
            max_liquidity: 1_000_000,
            cumulative_yield_per_lp: 0,
            protocol_yield: 0,
        };

        let vault_out = Vault {
            token_mint: token_out,
            pyth_price_account: pyth_out,
            create_at_ts: 0,
            is_active: true,
            base_fee: 0,
            max_age_price: 0,
            lp_mint: Pubkey::new_unique(),
            initial_liquidity: 1_000_000,
            current_liquidity: 1_000_000,
            max_liquidity: 1_000_000,
            cumulative_yield_per_lp: 0,
            protocol_yield: 0,
        };

        let treasury = Treasury {
            fee_bps: 30,
            stoptap: false,
            admin: Pubkey::new_unique(),
        };

        let amm = OxediumAmm {
            key: Pubkey::new_unique(),
            label: "Oxedium".to_string(),
            treasury: Some((Pubkey::new_unique(), treasury)),
            program_id: Pubkey::new_unique(),
            vaults: HashMap::from([
                (vault_in_pubkey, vault_in),
                (vault_out_pubkey, vault_out),
            ]),
            prices: HashMap::default(),
            decimals: HashMap::default(),
        };

        let user = Pubkey::new_unique();
        let source_ata = Pubkey::new_unique();
        let dest_ata = Pubkey::new_unique();

        let params = SwapParams {
            in_amount: 1000,
            token_transfer_authority: user,
            source_token_account: source_ata,
            destination_token_account: dest_ata,
            quote_mint_to_referrer: None,
            swap_mode: SwapMode::ExactIn,
            out_amount: 1,
            source_mint: token_in,
            destination_mint: token_out,
            jupiter_program_id: &Pubkey::new_unique(),
            missing_dynamic_accounts_as_default: false,
        };

        let result = amm.get_swap_and_account_metas(&params).unwrap();

            println!("--- Swap Account Metas ---");
            for (i, meta) in result.account_metas.iter().enumerate() {
                println!(
                    "{}: pubkey={}, writable={}, signer={}",
                    i,
                    meta.pubkey,
                    meta.is_writable,
                    meta.is_signer
                );
            }

            assert_eq!(result.swap, Swap::Oxedium);
            assert!(!result.account_metas.is_empty());
    }
}

