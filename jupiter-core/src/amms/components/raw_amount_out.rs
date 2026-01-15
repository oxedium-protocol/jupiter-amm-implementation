use anyhow::{anyhow, Result};
use crate::utils::SCALE;

/// Calculates the raw output amount for a token swap using fixed-point math.
/// Supports dust swaps by avoiding early division and rounding only once at the end.
///
/// # Arguments
/// * `amount_in` - Input token amount in smallest units
/// * `decimals_in` - Decimals of the input token
/// * `decimals_out` - Decimals of the output token
/// * `price_in` - Price of the input token (e.g. Pyth price, scaled)
/// * `price_out` - Price of the output token (e.g. Pyth price, scaled)
///
/// # Returns
/// * `Result<u64>` - Output token amount in smallest units
pub fn raw_amount_out(
    amount_in: u64,
    decimals_in: u32,
    decimals_out: u32,
    price_in: u64,
    price_out: u64,
) -> Result<u64> {
    let amount_in = amount_in as u128;
    let price_in = price_in as u128;
    let price_out = price_out as u128;

    // 1. Convert input amount into fixed-point token representation
    let amount_fp = amount_in
        .checked_mul(SCALE)
        .ok_or_else(|| anyhow!("Overflow in mul during amount_fp calculation"))?
        .checked_div(10u128.pow(decimals_in as u32))
        .ok_or_else(|| anyhow!("Overflow in div during amount_fp calculation"))?;

    // 2. Convert input token amount into USD value (still fixed-point)
    let usd_fp = amount_fp
        .checked_mul(price_in)
        .ok_or_else(|| anyhow!("Overflow in mul during usd_fp calculation"))?
        .checked_div(1_000_000_00) // adjust scale if needed
        .ok_or_else(|| anyhow!("Overflow in div during usd_fp calculation"))?;

    // 3. Convert USD value into output token amount (fixed-point)
    let out_fp = usd_fp
        .checked_mul(1_000_000_00)
        .ok_or_else(|| anyhow!("Overflow in mul during out_fp calculation"))?
        .checked_div(price_out)
        .ok_or_else(|| anyhow!("Overflow in div during out_fp calculation"))?;

    // 4. Convert fixed-point output into smallest output token units
    let out = out_fp
        .checked_mul(10u128.pow(decimals_out as u32))
        .ok_or_else(|| anyhow!("Overflow in mul during final conversion"))?
        .checked_div(SCALE)
        .ok_or_else(|| anyhow!("Overflow in div during final conversion"))?;

    u64::try_from(out).map_err(|_| anyhow!("Overflow converting final output to u64"))
}
