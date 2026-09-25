//! Authoritative match data, shared with all clients.
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

pub const MATCH_SECONDS: u32 = 300;
pub const RESULTS_SECONDS: u32 = 10;
pub const KILL_FEED_SECONDS: u32 = 6;
pub const KILL_FEED_LIMIT: usize = 5;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KillEntry {
    pub killer: Option<String>,
    pub victim: String,
    pub remaining_seconds: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScoreRow {
    pub id: u64,
    pub nickname: String,
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
            remaining_seconds: MATCH_SECONDS,
            rows: Vec::new(),
            kill_feed: Vec::new(),
        }
    }
}
impl MatchState {
    pub fn record_death(&mut self, victim: u64, killer: Option<u64>) {
        if self.results {
            return;
        }
        if let Some(victim_name) = self
            .rows
            .iter()
            .find(|r| r.id == victim)
            .map(|r| r.nickname.clone())
        {
            let killer_name = killer
                .filter(|id| *id != victim)
                .and_then(|id| self.rows.iter().find(|r| r.id == id))
                .map(|r| r.nickname.clone());
            self.kill_feed.push(KillEntry {
                killer: killer_name,
                victim: victim_name,
                remaining_seconds: KILL_FEED_SECONDS,
            });
            if self.kill_feed.len() > KILL_FEED_LIMIT {
                self.kill_feed.remove(0);
            }
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
    /// Deterministic rank: most kills, fewest deaths, stable player id for ties.
    pub fn ranked(&self) -> Vec<&ScoreRow> {
        let mut rows: Vec<_> = self.rows.iter().collect();
        rows.sort_by_key(|row| (std::cmp::Reverse(row.kills), row.deaths, row.id));
        rows
    }
}
