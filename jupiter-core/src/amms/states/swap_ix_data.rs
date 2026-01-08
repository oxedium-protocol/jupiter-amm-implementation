use borsh::BorshSerialize;

#[derive(BorshSerialize)]
pub struct SwapIxData {
    pub amount_in: u64,
    pub partner_fee_bps: u64,
}