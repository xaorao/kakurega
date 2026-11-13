//! The NEAR side of a capsule.
//!
//! Reads the Kakurega registry contract (who registered a capsule, whether a model's signing key was seen inside genuine
//! Intel TDX hardware, what an order holds) and sends the transactions an owner asks for: having NEAR's verifier contract
//! check a model's hardware quote, opening an order, registering a capsule for one. Transactions are sent on testnet only.

pub mod collateral;
pub mod intents;
pub mod registry;

pub use near_api::NearToken;
pub use registry::{Chain, ChainError, ModelQuote, Network, Payer, Sent};

/// The registry this build knows on NEAR testnet.
pub const TESTNET_REGISTRY: &str = "kakurega-registry-c68af3.testnet";
/// Kakurega's fee on a paid order, in hundredths of a percent of the reviewer's amount: what this build's registries are
/// created with. A client reads the registry's own `fee_terms` when it can.
pub const DEFAULT_FEE_BPS: u16 = 200;
/// The highest fee any registry can be created with (10%); a reported price above it is not believed.
pub const MAX_FEE_BPS: u16 = 1_000;
/// NEAR AI Cloud's public address for hardware reports.
pub const NEAR_AI: &str = "https://cloud-api.near.ai/v1";

/// A runtime for a command that makes a few chain calls and exits.
pub fn runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
}

/// What a client sets aside so the reviewer receives `amount` at a fee of `bps`: the amount and the fee on it, rounded up, as
/// the registry computes them (`price_of` in the contract). `None` when the sum cannot be held.
pub fn price_with(amount: u128, bps: u16) -> Option<(u128, u128)> {
    let b = u128::from(bps);
    let fee = amount / 10_000 * b + (amount % 10_000 * b).div_ceil(10_000);
    amount.checked_add(fee).map(|_| (amount, fee))
}

#[cfg(test)]
mod price_tests {
    #[test]
    fn pr_w01_the_client_prices_an_amount_as_the_registry_does() {
        assert_eq!(super::price_with(1_234_567, 200), Some((1_234_567, 24_692)));
        assert_eq!(super::price_with(10_000, 200), Some((10_000, 200)));
        assert_eq!(super::price_with(5, 0), Some((5, 0)));
        assert_eq!(super::price_with(u128::MAX, 200), None);
    }
}
