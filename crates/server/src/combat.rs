use bevy::prelude::*;
use hookrunner_shared::{
    PlayerId, PlayerInput, PlayerState, TICK_DURATION, health, level,
    weapon::{self, Impact, Projectile},
};
use lightyear::prelude::{input::native::ActionState, *};

/// Publish only bolts that survived this frame’s collision steps. An immediate
/// wall hit must not enqueue a network despawn for a never-published entity.
pub fn publish_projectiles(
    mut commands: Commands,
    bolts: Query<Entity, (With<Projectile>, Without<Replicate>)>,
) {
    for entity in &bolts {
        commands.entity(entity).insert((
            Replicate::to_clients(NetworkTarget::All),
            InterpolationTarget::to_clients(NetworkTarget::All),
        ));
    }
}

pub fn advance_projectiles(
    mut commands: Commands,
    mut projectiles: Query<(Entity, &mut Projectile)>,
    mut round: ResMut<hookrunner_shared::match_state::MatchState>,
    mut players: Query<(&PlayerId, &mut PlayerState, &ActionState<PlayerInput>)>,
) {
    if round.results {
        return;
    }
    for (entity, mut bolt) in &mut projectiles {
        let delta = bolt.direction * weapon::PROJECTILE_SPEED * TICK_DURATION.as_secs_f32();
        let hit = weapon::trace(
            level::world(),
            &bolt,
            delta,
            players.iter().map(|(id, state, _)| (id.0, state)),
        );
        if let Some((_, impact)) = hit {
            if let Impact::Player(victim) = impact {
                for (id, mut state, input) in &mut players {
                    if id.0 == victim && state.death.is_none() {
                        if health::apply_damage(
                            &mut state,
                            weapon::PROJECTILE_DAMAGE,
                            input.0.pitch_radians(),
                        ) {
                            round.record_death(victim, Some(bolt.owner));
                        }
                        break;
                    }
                }
            }
            commands.entity(entity).despawn();
            continue;
        }
        bolt.position += delta;
        bolt.remaining_ticks -= 1;
        if bolt.remaining_ticks == 0 {
            commands.entity(entity).despawn();
        }
    }
}
