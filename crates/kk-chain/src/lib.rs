//! The NEAR side of a capsule.
//!
//! Reads the Kakurega registry contract (who registered a capsule, whether a model's signing key was seen inside genuine
//! Intel TDX hardware, what an order holds) and sends the transactions an owner asks for: having NEAR's verifier contract
//! check a model's hardware quote, opening an order, registering a capsule for one. Transactions are sent on testnet only.

pub mod collateral;
pub mod intents;
pub mod registry;

pub use registry::{Chain, ChainError, ModelQuote, Network, Payer, Sent};

/// The registry this build knows on NEAR testnet.
pub const TESTNET_REGISTRY: &str = "kakurega-registry-5d26f9.testnet";
/// NEAR AI Cloud's public address for hardware reports.
pub const NEAR_AI: &str = "https://cloud-api.near.ai/v1";

/// A runtime for a command that makes a few chain calls and exits.
pub fn runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
}
