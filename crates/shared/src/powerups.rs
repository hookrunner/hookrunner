use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{PlayerState, health::MAX_HEALTH};

pub const MAX_SHIELD: u16 = 100;
pub const SPEED_DURATION_TICKS: u16 = (crate::TICK_HZ * 8.0) as u16;
pub const SPEED_MULTIPLIER: f32 = 1.5;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Reflect)]
pub enum PickupKind {
    Health,
    Speed,
    Shield,
}

#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Reflect)]
pub struct Pickup {
    pub kind: PickupKind,
    pub position: Vec3,
}

impl Pickup {
    pub fn apply(self, player: &mut PlayerState) -> bool {
        if player.death.is_some() || player.match_paused {
            return false;
        }
        match self.kind {
            PickupKind::Health if player.health.0 < MAX_HEALTH => {
                player.health.0 = MAX_HEALTH;
                true
            }
            PickupKind::Shield if player.shield < MAX_SHIELD => {
                player.shield = MAX_SHIELD;
                true
            }
            PickupKind::Speed => {
                player.speed_ticks = SPEED_DURATION_TICKS;
                true
            }
            _ => false,
        }
    }
}
