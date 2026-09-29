use bevy::prelude::*;
use hookrunner_shared::{
    PlayerId, PlayerState, TICK_HZ, arena, level,
    match_state::MatchState,
    powerups::{Pickup, PickupKind},
};
use lightyear::prelude::*;

const ACTIVE_PICKUPS: usize = 8;
const RESPAWN_TICKS: u16 = (TICK_HZ * 4.0) as u16;
const MIN_SPACING: f32 = 5.0;
const COLLECTION_RADIUS: f32 = 1.05;

#[derive(Resource, Default)]
pub struct PickupDirector {
    initialized: bool,
    remaining_ticks: u16,
}

pub fn collect(
    mut commands: Commands,
    round: Res<MatchState>,
    pickups: Query<(Entity, &Pickup)>,
    mut players: Query<(&PlayerId, &mut PlayerState)>,
) {
    if round.results {
        return;
    }
    for (entity, pickup) in &pickups {
        for (id, mut player) in &mut players {
            let center = player.position + Vec3::Y * (arena::PLAYER_HEIGHT / 2.0);
            if center.distance_squared(pickup.position) > COLLECTION_RADIUS * COLLECTION_RADIUS {
                continue;
            }
            if pickup.apply(&mut player) {
                commands.entity(entity).despawn();
                info!("Player {:016x} collected {:?}", id.0, pickup.kind);
                break;
            }
        }
    }
}

pub fn manage(
    mut commands: Commands,
    mut director: ResMut<PickupDirector>,
    round: Res<MatchState>,
    pickups: Query<(Entity, &Pickup)>,
    players: Query<&PlayerState>,
) {
    if round.results {
        for (entity, _) in &pickups {
            commands.entity(entity).despawn();
        }
        director.initialized = false;
        director.remaining_ticks = 0;
        return;
    }

    let mut occupied: Vec<Vec3> = pickups.iter().map(|(_, pickup)| pickup.position).collect();
    if !director.initialized {
        for kind in [
            PickupKind::Health,
            PickupKind::Shield,
            PickupKind::Speed,
            PickupKind::Health,
            PickupKind::Shield,
            PickupKind::Speed,
            PickupKind::Health,
            PickupKind::Shield,
        ] {
            spawn_random(&mut commands, kind, &mut occupied, &players);
        }
        info!("Spawned {} map powerups", occupied.len());
        director.initialized = true;
        director.remaining_ticks = RESPAWN_TICKS;
        return;
    }
    director.remaining_ticks = director.remaining_ticks.saturating_sub(1);
    if director.remaining_ticks == 0 {
        if occupied.len() < ACTIVE_PICKUPS {
            let kind = match rand::random::<u8>() % 3 {
                0 => PickupKind::Health,
                1 => PickupKind::Shield,
                _ => PickupKind::Speed,
            };
            spawn_random(&mut commands, kind, &mut occupied, &players);
        }
        director.remaining_ticks = RESPAWN_TICKS;
    }
}

fn spawn_random(
    commands: &mut Commands,
    kind: PickupKind,
    occupied: &mut Vec<Vec3>,
    players: &Query<&PlayerState>,
) {
    let candidates = &level::data().pickup_spawns;
    if candidates.is_empty() {
        return;
    }
    let start = rand::random::<u32>() as usize % candidates.len();
    for offset in 0..candidates.len() {
        let raw = candidates[(start + offset) % candidates.len()];
        let Some(feet) = level::world().ground(raw + Vec3::Y * 1.0, 3.0) else {
            continue;
        };
        let position = feet + Vec3::Y * 0.65;
        if occupied
            .iter()
            .any(|other| other.distance_squared(position) < MIN_SPACING * MIN_SPACING)
            || players
                .iter()
                .any(|player| player.position.distance_squared(feet) < 2.0 * 2.0)
        {
            continue;
        }
        commands.spawn((
            Pickup { kind, position },
            Replicate::to_clients(NetworkTarget::All),
        ));
        occupied.push(position);
        return;
    }
}
