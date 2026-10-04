//! Authoritative match data, shared with all clients.
use crate::player_color::PlayerColor;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KillEntry {
    pub killer: Option<String>,
    pub killer_color: Option<PlayerColor>,
    pub victim: String,
    pub victim_color: PlayerColor,
    pub remaining_seconds: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScoreRow {
    pub id: u64,
    pub nickname: String,
    pub color: PlayerColor,
    pub kills: u32,
    pub deaths: u32,
    pub connected: bool,
}

#[derive(Component, Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MatchState {
    pub number: u64,
    pub results: bool,
    pub remaining_seconds: u32,
    pub rows: Vec<ScoreRow>,
    pub kill_feed: Vec<KillEntry>,
}
impl Default for MatchState {
    fn default() -> Self {
        Self {
            number: 1,
            results: false,
            remaining_seconds: crate::tuning::MatchTuning::default().match_duration.ceil() as u32,
            rows: Vec::new(),
            kill_feed: Vec::new(),
        }
    }
}
impl MatchState {
    pub fn record_death(
        &mut self,
        victim: u64,
        killer: Option<u64>,
        tuning: crate::tuning::MatchTuning,
    ) {
        if self.results {
            return;
        }
        if let Some((victim_name, victim_color)) = self
            .rows
            .iter()
            .find(|r| r.id == victim)
            .map(|r| (r.nickname.clone(), r.color))
        {
            let killer = killer
                .filter(|id| *id != victim)
                .and_then(|id| self.rows.iter().find(|r| r.id == id))
                .map(|r| (r.nickname.clone(), r.color));
            self.kill_feed.push(KillEntry {
                killer: killer.as_ref().map(|(name, _)| name.clone()),
                killer_color: killer.map(|(_, color)| color),
                victim: victim_name,
                victim_color,
                remaining_seconds: tuning.kill_feed_time as u32,
            });
            self.trim_feed(tuning);
        }
        for row in &mut self.rows {
            if row.id == victim {
                row.deaths += 1;
            }
            if Some(row.id) == killer && row.id != victim {
                row.kills += 1;
            }
        }
    }
    pub fn trim_feed(&mut self, tuning: crate::tuning::MatchTuning) {
        for entry in &mut self.kill_feed {
            entry.remaining_seconds = entry.remaining_seconds.min(tuning.kill_feed_time as u32);
        }
        self.kill_feed.retain(|entry| entry.remaining_seconds > 0);
        let excess = self
            .kill_feed
            .len()
            .saturating_sub(tuning.kill_feed_limit as usize);
        self.kill_feed.drain(..excess);
    }
    /// Deterministic rank: most kills, fewest deaths, stable player id for ties.
    pub fn ranked(&self) -> Vec<&ScoreRow> {
        let mut rows: Vec<_> = self.rows.iter().collect();
        rows.sort_by_key(|row| (std::cmp::Reverse(row.kills), row.deaths, row.id));
        rows
    }
}
