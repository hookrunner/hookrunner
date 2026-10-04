//! Automatic energy pistol. Fire gating rewinds with player prediction;
//! projectile movement and hits are authoritative on the server.
use bevy::{math::Curve, prelude::*};
use parry3d::{
    na::{Isometry3, Vector3},
    query::{ShapeCastOptions, cast_shapes},
    shape::{Ball, Capsule},
};
use serde::{Deserialize, Serialize};

use crate::{PlayerInput, PlayerState, level::CollisionWorld};

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
        weapon.cooldown_ticks = crate::tuning::seconds_to_ticks(state.tuning.fire_interval);
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
    pub lifetime_ticks: u16,
    pub speed: f32,
    pub radius: f32,
    pub damage: u16,
}

impl Projectile {
    pub fn from_shot(owner: u64, state: &PlayerState, input: &PlayerInput) -> Self {
        let origin = state.position + Vec3::Y * state.tuning.eye_height;
        Self {
            owner,
            shot: state.weapon.shot,
            origin,
            position: origin,
            direction: Quat::from_euler(EulerRot::YXZ, state.yaw, input.pitch_radians(), 0.0)
                * Vec3::NEG_Z,
            remaining_ticks: crate::tuning::seconds_to_ticks(state.tuning.projectile_lifetime),
            lifetime_ticks: crate::tuning::seconds_to_ticks(state.tuning.projectile_lifetime),
            speed: state.tuning.projectile_speed,
            radius: state.tuning.projectile_radius,
            damage: state.tuning.projectile_damage as u16,
        }
    }
}

impl Ease for Projectile {
    fn interpolating_curve_unbounded(start: Self, end: Self) -> impl Curve<Self> {
        FunctionCurve::new(Interval::UNIT, move |t: f32| Self {
            position: start.position.lerp(end.position, t.clamp(0.0, 1.0)),
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
    trace_moving(
        world,
        projectile,
        delta,
        players
            .into_iter()
            .filter(|(_, p)| p.death.is_none())
            .map(|(id, p)| {
                (
                    id,
                    p.position,
                    Vec3::ZERO,
                    p.tuning.player_radius,
                    p.tuning.player_height,
                )
            }),
    )
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ShotImpact {
    pub owner: u64,
    pub shot: u16,
    pub tick: u16,
    pub position: Vec3,
    pub normal: Vec3,
    /// Only populated after the server actually applied damage.
    pub victim: Option<u64>,
}

pub fn trace_moving(
    world: &CollisionWorld,
    projectile: &Projectile,
    delta: Vec3,
    players: impl IntoIterator<Item = (u64, Vec3, Vec3, f32, f32)>,
) -> Option<(f32, Impact)> {
    let mut first = world
        .sweep_sphere(projectile.position, delta, projectile.radius)
        .map(|fraction| (fraction, Impact::Wall));
    for (id, position, movement, radius, height) in players {
        if id == projectile.owner {
            continue;
        }
        let capsule = Capsule::new_y(height / 2.0 - radius, radius);
        let center = position + Vec3::Y * (height / 2.0);
        let hit = cast_shapes(
            &Isometry3::translation(center.x, center.y, center.z),
            &Vector3::new(movement.x, movement.y, movement.z),
            &capsule,
            &Isometry3::translation(
                projectile.position.x,
                projectile.position.y,
                projectile.position.z,
            ),
            &Vector3::new(delta.x, delta.y, delta.z),
            &Ball::new(projectile.radius),
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
