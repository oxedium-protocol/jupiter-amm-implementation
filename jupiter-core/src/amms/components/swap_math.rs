use anyhow::{anyhow, Result};
use crate::{
    components::{calculate_fee_amount, fees_setting, raw_amount_out},
    states::Vault,
};

pub struct SwapMathResult {
    pub swap_fee_bps: u64,
    pub raw_amount_out: u64,
    pub net_amount_out: u64,
    pub lp_fee_amount: u64,
    pub protocol_fee_amount: u64,
    pub partner_fee_amount: u64,
}

/// Computes the resulting amounts and fees for a token swap
///
/// # Arguments
/// * `amount_in` - Amount of input tokens
/// * `price_in` - Price of input token (scaled)
/// * `price_out` - Price of output token (scaled)
/// * `decimals_in` - Number of decimals of the input token
/// * `decimals_out` - Number of decimals of the output token
/// * `vault_in` - Input token vault info
/// * `vault_out` - Output token vault info
/// * `treasury` - Treasury info containing protocol fees
/// * `partner_fee_bps` - Optional partner fee in basis points
///
/// # Returns
/// `SwapMathResult` containing raw output, net output, and all individual fees
pub fn compute_swap_math(
    amount_in: u64,
    price_in: u64,
    price_out: u64,
    decimals_in: u8,
    decimals_out: u8,
    vault_in: &Vault,
    vault_out: &Vault,
    protocol_fee_bps: u64,
    partner_fee_bps: u64,
) -> Result<SwapMathResult> {
    // Get the LP fee and protocol fee
    let swap_fee_bps = fees_setting(&vault_in, &vault_out);

    // 1️⃣ Calculate the raw output amount before any fees
    let raw_out = raw_amount_out(
        amount_in,
        decimals_in,
        decimals_out,
        price_in,
        price_out,
    ).map_err(|e| anyhow!("raw_amount_out failed: {:?}", e))?;

    // 2️⃣ Ensure the total fees do not exceed 100%
    if swap_fee_bps + protocol_fee_bps + partner_fee_bps > 10_000 {
        return Err(anyhow!("Total fee exceeds 100%"));
    }

    // 3️⃣ Calculate individual fees and net output after fees
    let (after_fee, lp_fee, protocol_fee, partner_fee) = calculate_fee_amount(
        raw_out,
        swap_fee_bps,
        protocol_fee_bps,
        partner_fee_bps,
    ).map_err(|e| anyhow!("calculate_fee_amount failed: {:?}", e))?;

    // 4️⃣ Check if the vault has sufficient liquidity
    let total_out = after_fee
        .checked_add(lp_fee)
        .and_then(|v| v.checked_add(protocol_fee))
        .and_then(|v| v.checked_add(partner_fee))
        .ok_or_else(|| anyhow!("Overflow when summing fees"))?;

    if vault_out.current_liquidity < total_out {
        return Err(anyhow!("Insufficient liquidity in vault"));
    }

    // 5️⃣ Return the computed result
    Ok(SwapMathResult {
        swap_fee_bps,
        raw_amount_out: raw_out,
        net_amount_out: after_fee,
        lp_fee_amount: lp_fee,
        protocol_fee_amount: protocol_fee,
        partner_fee_amount: partner_fee,
    })
}
