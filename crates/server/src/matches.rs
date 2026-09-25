use bevy::prelude::*;
use hookrunner_shared::{
    PlayerId, PlayerInput, PlayerState,
    match_state::{MATCH_SECONDS, MatchState, RESULTS_SECONDS, ScoreRow},
    movement,
    protocol::PlayerName,
    weapon::Projectile,
};
use lightyear::prelude::{input::native::ActionState, *};
use std::time::Duration;

#[derive(Resource)]
pub struct MatchClock {
    pub remaining: Duration,
    feed_fraction: Duration,
}
impl Default for MatchClock {
    fn default() -> Self {
        Self {
            remaining: Duration::from_secs(MATCH_SECONDS.into()),
            feed_fraction: Duration::ZERO,
        }
    }
}

pub fn setup(mut commands: Commands) {
    commands.spawn((
        MatchState::default(),
        Replicate::to_clients(NetworkTarget::All),
    ));
}

// Real time keeps the single server session advancing even without players.
// This runs before fixed simulation, so shots at/after the deadline cannot score.
pub fn advance(
    time: Res<Time<Real>>,
    mut clock: ResMut<MatchClock>,
    mut round: ResMut<MatchState>,
    mut players: Query<(
        &PlayerId,
        &PlayerName,
        &mut PlayerState,
        Option<&ActionState<PlayerInput>>,
    )>,
    bolts: Query<Entity, With<Projectile>>,
    mut commands: Commands,
) {
    // Expire transient events on server time, including during results/empty sessions.
    clock.feed_fraction += time.delta();
    let seconds = clock.feed_fraction.as_secs();
    clock.feed_fraction -= Duration::from_secs(seconds);
    if seconds > 0 && !round.kill_feed.is_empty() {
        for entry in &mut round.kill_feed {
            entry.remaining_seconds = entry
                .remaining_seconds
                .saturating_sub(seconds.min(u32::MAX as u64) as u32);
        }
        round.kill_feed.retain(|entry| entry.remaining_seconds > 0);
    }
    let mut delta = time.delta();
    let mut reset = false;
    let mut clear_bolts = false;
    while delta >= clock.remaining {
        delta -= clock.remaining;
        if round.results {
            round.number += 1;
            round.results = false;
            round.rows.clear();
            round.kill_feed.clear();
            clock.remaining = Duration::from_secs(MATCH_SECONDS.into());
            reset = true;
            info!("Match {} started", round.number);
        } else {
            round.results = true;
            clock.remaining = Duration::from_secs(RESULTS_SECONDS.into());
            info!("Match {} finished", round.number);
        }
        clear_bolts = true;
    }
    clock.remaining -= delta;
    let seconds = clock.remaining.as_secs_f64().ceil() as u32;
    if round.remaining_seconds != seconds {
        round.remaining_seconds = seconds;
    }
    if clear_bolts {
        for entity in &bolts {
            commands.entity(entity).despawn();
        }
    }
    for (id, name, mut player, input) in &mut players {
        if reset {
            movement::respawn(&mut player, &input.map(|a| a.0.clone()).unwrap_or_default());
        }
        if player.match_paused != round.results {
            player.match_paused = round.results;
        }
        if !round.results && !round.rows.iter().any(|r| r.id == id.0) {
            round.rows.push(ScoreRow {
                id: id.0,
                nickname: name.0.clone(),
                kills: 0,
                deaths: 0,
                connected: true,
            });
        }
    }
    // Keep departed participants' scores until the round ends.
    let changed = round
        .rows
        .iter()
        .any(|row| row.connected != players.iter().any(|(id, _, _, _)| id.0 == row.id));
    if changed {
        for row in &mut round.rows {
            row.connected = players.iter().any(|(id, _, _, _)| id.0 == row.id);
        }
    }
}

pub fn publish(round: Res<MatchState>, mut snapshot: Single<&mut MatchState>) {
    if **snapshot != *round {
        **snapshot = round.clone();
    }
}
