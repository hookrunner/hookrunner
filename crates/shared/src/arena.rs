use crate::level::{self, Spawn};
use bevy::prelude::*;

pub const PLAYER_RADIUS: f32 = 0.4;
pub const PLAYER_HEIGHT: f32 = 1.8;
pub const EYE_HEIGHT: f32 = 1.6;
pub const MAX_PLAYERS: usize = 16;

/// The authored deathmatch spawns, settled onto Stormkeep's actual floors.
pub fn spawn(index: usize) -> Spawn {
    level::world().settle_spawn(level::data().spawns[index % level::data().spawns.len()])
}

pub fn choose_spawn(random_index: usize, occupied: &[Vec3]) -> usize {
    let spawns = &level::data().spawns;
    let start = random_index % spawns.len();
    (0..spawns.len())
        .map(|offset| (start + offset) % spawns.len())
        .find(|&index| {
            occupied
                .iter()
                .all(|other| spawns[index].position.distance_squared(*other) > 4.0)
        })
        .unwrap_or_else(|| {
            (0..spawns.len())
                .max_by(|&a, &b| {
                    let nearest = |index: usize| {
                        occupied
                            .iter()
                            .map(|other| spawns[index].position.distance_squared(*other))
                            .fold(f32::INFINITY, f32::min)
                    };
                    nearest(a).total_cmp(&nearest(b))
                })
                .unwrap()
        })
}
