//! Gameplay data and fixed-step movement shared by the server and predicted client.
pub mod arena;
pub mod level;
pub mod match_state;
pub mod movement;
pub mod nickname;
pub mod protocol;
pub mod weapon;

pub use protocol::{PlayerId, PlayerInput, PlayerState, ProtocolPlugin};

/// Exact shared simulation/map fingerprint, checked before WebSocket upgrade.
pub const SIMULATION_BUILD: &str = env!("HOOKRUNNER_SIMULATION_BUILD");

pub const TICK_HZ: f64 = 120.0;
pub const TICK_DURATION: std::time::Duration =
    std::time::Duration::from_nanos(1_000_000_000 / TICK_HZ as u64);
pub const SEND_INTERVAL: std::time::Duration = TICK_DURATION;
