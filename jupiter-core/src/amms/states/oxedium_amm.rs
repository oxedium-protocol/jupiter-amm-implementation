use borsh::{BorshDeserialize, BorshSerialize};
use solana_sdk::pubkey::Pubkey;

#[derive(BorshDeserialize, BorshSerialize, Clone, Debug)]
pub struct OxediumAmmState {
    pub label: String,

    pub key: Pubkey,

    pub mint_in: Pubkey,
    pub mint_out: Pubkey,

    pub oracle_in: Pubkey,
    pub oracle_out: Pubkey,

    pub vault_in_signer_ata: Pubkey,
    pub vault_out_signer_ata: Pubkey,

    pub vault_in_pda: Pubkey,
    pub vault_out_pda: Pubkey,

    pub treasury_pda: Pubkey,
    pub treasury_ata_in: Pubkey,
    pub treasury_ata_out: Pubkey,

    pub partner_fee_account: Pubkey,

    pub program_id: Pubkey,
}