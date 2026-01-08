use borsh::{BorshDeserialize, BorshSerialize};
use solana_sdk::pubkey::Pubkey;

#[derive(BorshDeserialize, BorshSerialize, Clone, Debug, Default)]
pub struct Treasury {
    pub stoptap: bool,
    pub admin: Pubkey,
    pub fee_bps: u64
}