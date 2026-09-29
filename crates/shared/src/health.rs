//! Health rewinds together with PlayerState. Projectile damage is applied only by the server.
use crate::{PlayerState, movement};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

pub const MAX_HEALTH: u16 = 100;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Reflect)]
pub struct Health(pub u16);
impl Default for Health {
    fn default() -> Self {
        Self(MAX_HEALTH)
    }
}

/// Returns true only for the hit that causes death, so score is awarded once.
pub fn apply_damage(state: &mut PlayerState, amount: u16, pitch: f32) -> bool {
    if state.match_paused || state.death.is_some() || amount == 0 {
        return false;
    }
    state.health.0 = state.health.0.saturating_sub(amount);
    if state.health.0 == 0 {
        movement::kill(state, pitch);
        true
    } else {
        false
    }
}
