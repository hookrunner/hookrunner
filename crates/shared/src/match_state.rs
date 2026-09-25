//! Authoritative match data, shared with all clients.
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

pub const MATCH_SECONDS: u32 = 300;
pub const RESULTS_SECONDS: u32 = 10;

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
}
impl Default for MatchState {
    fn default() -> Self {
        Self {
            number: 1,
            results: false,
            remaining_seconds: MATCH_SECONDS,
            rows: Vec::new(),
        }
    }
}
impl MatchState {
    pub fn record_death(&mut self, victim: u64, killer: Option<u64>) {
        if self.results {
            return;
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
