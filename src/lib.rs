//! MobHub: Ultra-high-performance native Rust and Mojo faction operations suite for Torn City.

pub mod gym;
pub mod payout;
pub mod server;
pub mod storage;
pub mod torn;
pub mod valuation;
pub mod vault;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
