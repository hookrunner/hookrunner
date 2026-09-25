//! Automatic energy pistol. Fire gating rewinds with player prediction;
//! projectile movement and hits are authoritative on the server.
use bevy::{math::Curve, prelude::*};
use parry3d::{
    na::{Isometry3, Vector3},
    query::{ShapeCastOptions, cast_shapes},
    shape::{Ball, Capsule},
};
use serde::{Deserialize, Serialize};

use crate::{PlayerInput, PlayerState, arena, level::CollisionWorld};

pub const FIRE_COOLDOWN_TICKS: u16 = (crate::TICK_HZ * 0.2) as u16;
pub const PROJECTILE_DAMAGE: u16 = 25;
pub const PROJECTILE_SPEED: f32 = 60.0;
pub const PROJECTILE_RADIUS: f32 = 0.045;
pub const PROJECTILE_LENGTH: f32 = 2.4;
pub const PROJECTILE_LIFETIME_TICKS: u16 = (crate::TICK_HZ * 2.0) as u16;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Reflect)]
pub struct WeaponState {
    pub cooldown_ticks: u16,
    pub shot: u16,
}

pub fn step(state: &mut PlayerState, input: &PlayerInput) {
    let weapon = &mut state.weapon;
    weapon.cooldown_ticks = weapon.cooldown_ticks.saturating_sub(1);
    if input.fire && weapon.cooldown_ticks == 0 && state.death.is_none() {
        weapon.shot = weapon.shot.wrapping_add(1);
        weapon.cooldown_ticks = FIRE_COOLDOWN_TICKS;
    }
}

#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Reflect)]
pub struct Projectile {
    pub owner: u64,
    pub shot: u16,
    pub origin: Vec3,
    pub position: Vec3,
    pub direction: Vec3,
    pub remaining_ticks: u16,
}

impl Projectile {
    pub fn from_shot(owner: u64, state: &PlayerState, input: &PlayerInput) -> Self {
        let origin = state.position + Vec3::Y * arena::EYE_HEIGHT;
        Self {
            owner,
            shot: state.weapon.shot,
            origin,
            position: origin,
            direction: Quat::from_euler(EulerRot::YXZ, state.yaw, input.pitch_radians(), 0.0)
                * Vec3::NEG_Z,
            remaining_ticks: PROJECTILE_LIFETIME_TICKS,
        }
    }
}

impl Ease for Projectile {
    fn interpolating_curve_unbounded(start: Self, end: Self) -> impl Curve<Self> {
        FunctionCurve::new(Interval::UNIT, move |t| Self {
            position: start.position.lerp(end.position, t),
            remaining_ticks: if t >= 1.0 {
                end.remaining_ticks
            } else {
                start.remaining_ticks
            },
            ..start.clone()
        })
    }
}

#[derive(Debug, PartialEq)]
pub enum Impact {
    Wall,
    Player(u64),
}

/// Sweep the complete segment, choosing the first collision. This prevents a
/// fast bolt tunnelling through thin walls or hitting a player behind a wall.
pub fn trace<'a>(
    world: &CollisionWorld,
    projectile: &Projectile,
    delta: Vec3,
    players: impl IntoIterator<Item = (u64, &'a PlayerState)>,
) -> Option<(f32, Impact)> {
    let mut first = world
        .sweep_sphere(projectile.position, delta, PROJECTILE_RADIUS)
        .map(|fraction| (fraction, Impact::Wall));
    let capsule = Capsule::new_y(
        arena::PLAYER_HEIGHT / 2.0 - arena::PLAYER_RADIUS,
        arena::PLAYER_RADIUS,
    );
    for (id, player) in players {
        if id == projectile.owner || player.death.is_some() {
            continue;
        }
        let center = player.position + Vec3::Y * (arena::PLAYER_HEIGHT / 2.0);
        let hit = cast_shapes(
            &Isometry3::translation(center.x, center.y, center.z),
            &Vector3::zeros(),
            &capsule,
            &Isometry3::translation(
                projectile.position.x,
                projectile.position.y,
                projectile.position.z,
            ),
            &Vector3::new(delta.x, delta.y, delta.z),
            &Ball::new(PROJECTILE_RADIUS),
            ShapeCastOptions {
                max_time_of_impact: 1.0,
                stop_at_penetration: true,
                ..Default::default()
            },
        )
        .expect("capsule/sphere shape cast is supported");
        if let Some(hit) = hit
            && first
                .as_ref()
                .is_none_or(|(fraction, _)| hit.time_of_impact < *fraction)
        {
            first = Some((hit.time_of_impact, Impact::Player(id)));
        }
    }
    first
}
