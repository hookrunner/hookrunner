use bevy::{ecs::entity::MapEntities, math::Curve, prelude::*};
use lightyear::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PlayerId(pub u64);

#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PlayerName(pub String);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct JoinRequest {
    pub nickname: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct JoinRejected {
    pub reason: String,
}

pub struct LobbyChannel;

/// Feet position, planar/vertical velocity and body yaw. Presentation never writes this state.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Reflect, Default)]
pub struct PlayerState {
    pub position: Vec3,
    pub velocity: Vec2,
    pub vertical_velocity: f32,
    pub yaw: f32,
    pub dash: DashState,
    pub jump: JumpState,
    pub weapon: crate::weapon::WeaponState,
    pub health: crate::health::Health,
    pub grounded: bool,
    pub match_paused: bool,
    pub death: Option<DeathState>,
    /// Map-authored spawn heading and portal rotations relative to raw mouse aim.
    pub view_yaw_offset: f32,
    pub trigger_cooldown: u16,
    pub spawn_index: u8,
    /// Do not interpolate across a teleport or respawn.
    pub relocation: u16,
}

/// Death timing and captured aim rewind together with movement prediction.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Reflect)]
pub struct DeathState {
    pub remaining_ticks: u16,
    pub pitch: f32,
}

/// The extra air jump and consumed press history must rewind with prediction.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Reflect)]
pub struct JumpState {
    pub air_jumps_remaining: u8,
    pub last_press: u16,
}

impl Default for JumpState {
    fn default() -> Self {
        Self {
            air_jumps_remaining: crate::movement::MAX_AIR_JUMPS,
            last_press: 0,
        }
    }
}

/// Dash resources, direction and press history are part of rollback state.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Reflect)]
pub struct DashState {
    pub charges: u8,
    pub remaining_ticks: u16,
    pub recharge_ticks: u16,
    pub direction: Vec3,
    pub last_press: u16,
}

impl Default for DashState {
    fn default() -> Self {
        Self {
            charges: crate::movement::MAX_DASH_CHARGES,
            remaining_ticks: 0,
            recharge_ticks: 0,
            direction: Vec3::ZERO,
            last_press: 0,
        }
    }
}

impl Ease for PlayerState {
    fn interpolating_curve_unbounded(start: Self, end: Self) -> impl Curve<Self> {
        FunctionCurve::new(Interval::UNIT, move |t| {
            if start.relocation != end.relocation {
                return end.clone();
            }
            let angle = (end.yaw - start.yaw + std::f32::consts::PI)
                .rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;
            Self {
                position: start.position.lerp(end.position, t),
                velocity: start.velocity.lerp(end.velocity, t),
                vertical_velocity: start.vertical_velocity
                    + (end.vertical_velocity - start.vertical_velocity) * t,
                yaw: start.yaw + angle * t,
                weapon: if t >= 1.0 { end.weapon } else { start.weapon },
                health: if t >= 1.0 { end.health } else { start.health },
                // Discrete resource/timer state must not be blended between snapshots.
                dash: if t >= 1.0 { end.dash } else { start.dash },
                jump: if t >= 1.0 { end.jump } else { start.jump },
                death: if t >= 1.0 { end.death } else { start.death },
                match_paused: if t >= 1.0 {
                    end.match_paused
                } else {
                    start.match_paused
                },
                grounded: if t >= 1.0 {
                    end.grounded
                } else {
                    start.grounded
                },
                view_yaw_offset: if t >= 1.0 {
                    end.view_yaw_offset
                } else {
                    start.view_yaw_offset
                },
                trigger_cooldown: if t >= 1.0 {
                    end.trigger_cooldown
                } else {
                    start.trigger_cooldown
                },
                spawn_index: if t >= 1.0 {
                    end.spawn_index
                } else {
                    start.spawn_index
                },
                relocation: start.relocation,
            }
        })
    }
}

/// Only intent crosses the client-to-server boundary. Quantized aim cannot contain NaN.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq, Reflect)]
pub struct PlayerInput {
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
    /// One jump attempt per physical press; holding or repeating a packet cannot jump again.
    pub jump_press: u16,
    /// Wrapping press counter, sampled each render frame and carried across fixed ticks.
    /// Repeating an input packet must not trigger another dash.
    pub dash_press: u16,
    /// Held left button; fire cadence is enforced by shared simulation.
    pub fire: bool,
    pub yaw: u16,
    pub pitch: i16,
}

impl MapEntities for PlayerInput {
    fn map_entities<M: EntityMapper>(&mut self, _mapper: &mut M) {}
}

impl PlayerInput {
    pub const MAX_PITCH: f32 = 1.5;

    pub fn encode_yaw(yaw: f32) -> u16 {
        (yaw.rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU * 65536.0) as u16
    }

    pub fn yaw_radians(&self) -> f32 {
        self.yaw as f32 / 65536.0 * std::f32::consts::TAU
    }

    pub fn encode_pitch(pitch: f32) -> i16 {
        (pitch.clamp(-Self::MAX_PITCH, Self::MAX_PITCH) / Self::MAX_PITCH * i16::MAX as f32).round()
            as i16
    }

    pub fn pitch_radians(&self) -> f32 {
        (self.pitch as f32 / i16::MAX as f32).clamp(-1.0, 1.0) * Self::MAX_PITCH
    }
}

pub struct ProtocolPlugin;

impl Plugin for ProtocolPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(input::native::InputPlugin::<PlayerInput> {
            config: input::InputConfig {
                send_interval: crate::TICK_DURATION,
                ..default()
            },
        });
        app.add_channel::<LobbyChannel>(ChannelSettings {
            mode: ChannelMode::OrderedReliable(ReliableSettings::default()),
            ..default()
        })
        .add_direction(NetworkDirection::Bidirectional);
        app.register_message::<JoinRequest>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<JoinRejected>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_component::<crate::match_state::MatchState>();
        app.register_component::<PlayerName>();
        app.register_component::<PlayerId>();
        app.register_component::<crate::weapon::Projectile>()
            .add_linear_interpolation();
        app.register_component::<PlayerState>()
            .add_prediction()
            .add_linear_interpolation();
    }
}
