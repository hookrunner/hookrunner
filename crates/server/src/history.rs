use bevy::prelude::*;
use hookrunner_shared::{PlayerId, PlayerState, TICK_DURATION};
use lightyear::{interpolation::plugin::InterpolationDelay, prelude::*};
use std::collections::VecDeque;

const MAX_REWIND_TICKS: f32 = 18.0; // 150 ms at 120 Hz
const HISTORY_FRAMES: usize = 22;

#[derive(Component)]
pub struct ProjectileLag(pub f32);

#[derive(Clone)]
struct Pose {
    id: u64,
    position: Vec3,
    relocation: u16,
    alive: bool,
}
struct Frame {
    tick: Tick,
    players: Vec<Pose>,
}

#[derive(Resource, Default)]
pub struct PlayerHistory {
    frames: VecDeque<Frame>,
}

pub fn validated_delay(ping: &PingManager, delay: Option<&InterpolationDelay>) -> f32 {
    let tick_ms = TICK_DURATION.as_secs_f32() * 1000.0;
    let requested = delay.map_or(0.0, |delay| {
        delay.delay.tick_diff() as f32 + delay.delay.overstep().to_f32()
    });
    // A client cannot request arbitrary rewind. Its view delay is additionally
    // bounded by server-measured latency/jitter and a render-buffer allowance.
    let measured_limit =
        (ping.rtt().as_secs_f32() * 1000.0 + ping.jitter().as_secs_f32() * 4000.0 + 50.0) / tick_ms;
    requested.clamp(0.0, MAX_REWIND_TICKS.min(measured_limit))
}

impl PlayerHistory {
    fn capture(&mut self, tick: Tick, players: &Query<(&PlayerId, &PlayerState)>) {
        if self.frames.back().is_some_and(|frame| frame.tick == tick) {
            return;
        }
        self.frames.push_back(Frame {
            tick,
            players: players
                .iter()
                .map(|(id, state)| Pose {
                    id: id.0,
                    position: state.position,
                    relocation: state.relocation,
                    alive: state.death.is_none(),
                })
                .collect(),
        });
        while self.frames.len() > HISTORY_FRAMES {
            self.frames.pop_front();
        }
    }

    pub fn sample(
        &self,
        id: u64,
        tick: Tick,
        fraction: f32,
        current: &PlayerState,
    ) -> Option<Vec3> {
        if current.death.is_some() {
            return None;
        }
        let pose = |tick| {
            self.frames
                .iter()
                .find(|frame| frame.tick == tick)
                .and_then(|frame| frame.players.iter().find(|pose| pose.id == id))
                .filter(|pose| pose.alive && pose.relocation == current.relocation)
        };
        let start = pose(tick)?;
        if fraction <= f32::EPSILON {
            return Some(start.position);
        }
        let end = pose(tick + 1)?;
        Some(start.position.lerp(end.position, fraction))
    }

    pub fn sweep(&self, id: u64, now: Tick, lag: f32, state: &PlayerState) -> Option<(Vec3, Vec3)> {
        let lag = lag.clamp(0.0, MAX_REWIND_TICKS);
        let whole = lag.ceil() as u16;
        let fraction = whole as f32 - lag;
        let start_tick = now - whole - 1;
        let start = self.sample(id, start_tick, fraction, state)?;
        let end = self.sample(id, start_tick + 1, fraction, state)?;
        Some((start, end - start))
    }
}

pub fn capture_before(
    timeline: Res<LocalTimeline>,
    players: Query<(&PlayerId, &PlayerState)>,
    mut history: ResMut<PlayerHistory>,
) {
    history.capture(timeline.tick() - 1, &players);
}
pub fn capture_after(
    timeline: Res<LocalTimeline>,
    players: Query<(&PlayerId, &PlayerState)>,
    mut history: ResMut<PlayerHistory>,
) {
    history.capture(timeline.tick(), &players);
}
