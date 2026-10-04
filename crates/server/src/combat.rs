use bevy::prelude::*;
use hookrunner_shared::{
    PlayerId, PlayerInput, PlayerState, TICK_DURATION, health, level,
    weapon::{self, Impact, Projectile, ShotImpact},
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
    mut projectiles: Query<(Entity, &mut Projectile, &crate::history::ProjectileLag)>,
    timeline: Res<LocalTimeline>,
    history: Res<crate::history::PlayerHistory>,
    mut clients: Query<&mut MessageSender<ShotImpact>, With<Connected>>,
    mut round: ResMut<hookrunner_shared::match_state::MatchState>,
    mut players: Query<(&PlayerId, &mut PlayerState, &ActionState<PlayerInput>)>,
) {
    if round.results {
        return;
    }
    for (entity, mut bolt, lag) in &mut projectiles {
        let delta = bolt.direction * weapon::PROJECTILE_SPEED * TICK_DURATION.as_secs_f32();
        let hit = weapon::trace_moving(
            level::world(),
            &bolt,
            delta,
            players.iter().filter_map(|(id, state, _)| {
                history
                    .sweep(id.0, timeline.tick(), lag.0, state)
                    .map(|(position, movement)| (id.0, position, movement))
            }),
        );
        if let Some((fraction, impact)) = hit {
            let mut damaged = None;
            if let Impact::Player(victim) = impact {
                for (id, mut state, input) in &mut players {
                    if id.0 == victim && state.death.is_none() && !state.match_paused {
                        damaged = Some(victim);
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
            let event = ShotImpact {
                owner: bolt.owner,
                shot: bolt.shot,
                tick: timeline.tick().0,
                position: bolt.position + delta * fraction,
                normal: -bolt.direction,
                victim: damaged,
            };
            for mut sender in &mut clients {
                sender.send::<hookrunner_shared::protocol::LobbyChannel>(event.clone());
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
