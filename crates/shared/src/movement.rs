use crate::{
    PlayerInput, PlayerState, TICK_DURATION,
    level::{self, CollisionWorld, TriggerKind},
};
use bevy::prelude::*;

/// One simulation tick. Same function on the authoritative server and during client replay.
pub fn step(state: &mut PlayerState, input: &PlayerInput) {
    state.health.0 = state.health.0.min(state.tuning.max_health as u16);
    state.jump.air_jumps_remaining = state
        .jump
        .air_jumps_remaining
        .min(state.tuning.air_jumps as u8);
    if state.match_paused {
        state.jump.last_press = input.jump_press;
        state.dash.last_press = input.dash_press;
        return;
    }
    if let Some(death) = &mut state.death {
        // Consume presses throughout death, including the respawn tick. They
        // must never become queued jumps/dashes when control returns.
        state.jump.last_press = input.jump_press;
        state.dash.last_press = input.dash_press;
        death.remaining_ticks -= 1;
        if death.remaining_ticks == 0 {
            respawn(state, input);
        }
        return;
    }
    step_in_world(level::world(), state, input);
    apply_map_triggers(state, input);
    crate::weapon::step(state, input);
}

/// Enter the same death sequence for projectiles and map hazards. Repeated hits
/// cannot reset the timer or the captured death-camera aim.
pub fn kill(state: &mut PlayerState, pitch: f32) {
    if state.death.is_some() {
        return;
    }
    state.health.0 = 0;
    state.death = Some(crate::protocol::DeathState {
        remaining_ticks: crate::tuning::seconds_to_ticks(state.tuning.respawn_time).max(1),
        total_ticks: crate::tuning::seconds_to_ticks(state.tuning.respawn_time).max(1),
        pitch,
    });
    state.velocity = Vec2::ZERO;
    state.vertical_velocity = 0.0;
    state.dash.remaining_ticks = 0;
}

pub fn respawn(state: &mut PlayerState, input: &PlayerInput) {
    let index = (state.spawn_index as usize + 1) % level::data().spawns.len();
    let spawn = crate::arena::spawn(index, state.tuning);
    *state = PlayerState {
        tuning: state.tuning,
        health: crate::health::Health(state.tuning.max_health as u16),
        position: spawn.position,
        yaw: spawn.yaw,
        view_yaw_offset: spawn.yaw - input.yaw_radians(),
        spawn_index: index as u8,
        relocation: state.relocation.wrapping_add(1),
        dash: crate::protocol::DashState {
            last_press: input.dash_press,
            ..crate::protocol::DashState::new(state.tuning)
        },
        jump: crate::protocol::JumpState {
            last_press: input.jump_press,
            air_jumps_remaining: state.tuning.air_jumps as u8,
        },
        weapon: crate::weapon::WeaponState {
            shot: state.weapon.shot,
            ..default()
        },
        grounded: true,
        ..default()
    };
}

fn step_in_world(world: &CollisionWorld, state: &mut PlayerState, input: &PlayerInput) {
    let world = world.character(state.tuning);
    let dt = TICK_DURATION.as_secs_f32();
    state.yaw = input.yaw_radians() + state.view_yaw_offset;
    let grounded = state.vertical_velocity <= 0.0 && world.is_grounded(state.position);
    if grounded {
        state.jump.air_jumps_remaining = state.tuning.air_jumps as u8;
    }
    let jump_pressed = input.jump_press != state.jump.last_press;
    // Consume presses even when jumping is unavailable, so they cannot fire on landing.
    state.jump.last_press = input.jump_press;
    let axes = Vec2::new(
        i32::from(input.right) as f32 - i32::from(input.left) as f32,
        i32::from(input.backward) as f32 - i32::from(input.forward) as f32,
    )
    .normalize_or_zero();
    let (sin, cos) = state.yaw.sin_cos();
    let wish = Vec2::new(cos * axes.x + sin * axes.y, -sin * axes.x + cos * axes.y);
    let max_charges = state.tuning.dash_charges as u8;
    let recharge = crate::tuning::seconds_to_ticks(state.tuning.dash_recharge_time);
    state.dash.charges = state.dash.charges.min(max_charges);
    // Restore charges sequentially, including while airborne or dashing.
    if recharge == 0 || state.dash.charges == max_charges {
        state.dash.recharge_ticks = 0;
        state.dash.charges = max_charges;
    } else if state.dash.recharge_ticks > 0 {
        state.dash.recharge_ticks -= 1;
        if state.dash.recharge_ticks == 0 {
            state.dash.charges += 1;
            if state.dash.charges < max_charges {
                state.dash.recharge_ticks = recharge;
            }
        }
    } else {
        // A live capacity increase leaves new slots to recharge normally.
        state.dash.recharge_ticks = recharge;
    }
    let dash_pressed = input.dash_press != state.dash.last_press;
    state.dash.last_press = input.dash_press;
    let duration = crate::tuning::seconds_to_ticks(state.tuning.dash_time);
    if dash_pressed && state.dash.charges > 0 && duration > 0 {
        state.dash.charges -= 1;
        state.dash.remaining_ticks = duration;
        let (pitch_sin, pitch_cos) = input.pitch_radians().sin_cos();
        let forward = Vec3::new(-sin * pitch_cos, pitch_sin, -cos * pitch_cos);
        let right = Vec3::new(cos, 0.0, -sin);
        state.dash.direction = if axes == Vec2::ZERO {
            forward
        } else {
            (right * axes.x - forward * axes.y).normalize_or_zero()
        };
        // Flatten steep upward dashes while keeping their total speed and heading.
        // Downward aim is unrestricted so it can still feed landing momentum.
        state.dash.direction.y = state
            .dash
            .direction
            .y
            .min(state.dash.direction.xz().length() * state.tuning.dash_upward_ratio);
        state.dash.direction = state.dash.direction.normalize_or_zero();
        if state.dash.recharge_ticks == 0 {
            state.dash.recharge_ticks = recharge;
        }
    }
    // Capture before decrementing so the final dash tick can still convert an impact.
    let dashing = state.dash.remaining_ticks > 0;
    if dashing {
        // Replace all momentum, including falling/jumping speed, for a straight dash.
        let velocity = state.dash.direction * state.tuning.dash_speed;
        state.velocity = velocity.xz();
        state.vertical_velocity = velocity.y;
        state.dash.remaining_ticks -= 1;
    } else {
        if jump_pressed && (grounded || state.jump.air_jumps_remaining > 0) {
            if !grounded {
                state.jump.air_jumps_remaining -= 1;
            }
            state.vertical_velocity = state.tuning.jump_speed;
        }
        state.vertical_velocity -= state.tuning.gravity * dt;
        let target = wish * state.tuning.walk_speed;
        let acceleration = if axes == Vec2::ZERO {
            state.tuning.braking
        } else {
            state.tuning.acceleration
        };
        // Preserve landing momentum, then return gradually to walk speed.
        state.velocity = state.velocity.move_towards(target, acceleration * dt);
    }
    let velocity = Vec3::new(state.velocity.x, state.vertical_velocity, state.velocity.y);
    let motion = world.move_character(state.position, velocity, dt, grounded);
    state.position = motion.position;
    state.velocity = motion.velocity.xz();
    state.vertical_velocity = motion.velocity.y;
    state.grounded = motion.grounded;
    if dashing && !grounded && motion.landed && velocity.y < 0.0 {
        // Downward speed becomes forward speed only on an active dash impact.
        let boosted =
            Vec3::new(-sin, 0.0, -cos) * velocity.length() * state.tuning.dash_landing_multiplier;
        state.velocity = world.clip_at_wall(state.position, boosted).xz();
        state.vertical_velocity = 0.0;
        state.dash.remaining_ticks = 0;
    }
    if motion.grounded {
        state.jump.air_jumps_remaining = state.tuning.air_jumps as u8;
    }
}

fn apply_map_triggers(state: &mut PlayerState, input: &PlayerInput) {
    state.trigger_cooldown = state.trigger_cooldown.saturating_sub(1);
    let data = level::data();
    let hazard = state.position.y < state.tuning.kill_plane_height
        || data.triggers.iter().any(|trigger| {
            trigger.kind == TriggerKind::Hurt && trigger.touches(state.position, &state.tuning)
        });
    if hazard {
        kill(state, input.pitch_radians());
        return;
    }
    if state.trigger_cooldown > 0 {
        return;
    }
    for trigger in &data.triggers {
        if !trigger.touches(state.position, &state.tuning) {
            continue;
        }
        match trigger.kind {
            TriggerKind::Push => {
                state.velocity = trigger.velocity.xz() * state.tuning.jump_pad_multiplier;
                state.vertical_velocity = trigger.velocity.y * state.tuning.jump_pad_multiplier;
                state.grounded = false;
                state.dash.remaining_ticks = 0;
                state.jump.air_jumps_remaining = state.tuning.air_jumps as u8;
            }
            TriggerKind::Teleport | TriggerKind::Warp => {
                let rotation = if trigger.kind == TriggerKind::Teleport {
                    trigger.rotation - state.yaw
                } else {
                    trigger.rotation
                };
                let velocity = Quat::from_rotation_y(rotation)
                    * Vec3::new(state.velocity.x, state.vertical_velocity, state.velocity.y);
                state.velocity = velocity.xz() * state.tuning.portal_momentum_multiplier;
                state.vertical_velocity *= state.tuning.portal_momentum_multiplier;
                state.dash.direction = Quat::from_rotation_y(rotation) * state.dash.direction;
                state.position = trigger.destination;
                state.yaw += rotation;
                state.view_yaw_offset += rotation;
                state.relocation = state.relocation.wrapping_add(1);
                state.grounded = false;
            }
            TriggerKind::Hurt => unreachable!(),
        }
        state.trigger_cooldown = crate::tuning::seconds_to_ticks(state.tuning.trigger_cooldown);
        break;
    }
}
