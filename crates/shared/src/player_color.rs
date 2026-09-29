//! Small shared palette, stored as a validated network index.
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

pub const PALETTE: [[f32; 3]; 8] = [
    [1.00, 0.35, 0.18], // coral
    [0.20, 0.78, 0.95], // cyan
    [0.45, 0.88, 0.45], // green
    [1.00, 0.76, 0.25], // gold
    [0.72, 0.55, 1.00], // violet
    [1.00, 0.48, 0.73], // pink
    [0.38, 0.58, 1.00], // blue
    [0.96, 0.94, 0.76], // ivory
];

#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerColor(pub u8);

impl PlayerColor {
    pub fn new(index: u8) -> Option<Self> {
        ((index as usize) < PALETTE.len()).then_some(Self(index))
    }

    pub fn rgb(self) -> [f32; 3] {
        PALETTE[self.0 as usize]
    }
}
