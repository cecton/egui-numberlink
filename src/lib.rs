#![doc = include_str!("../README.md")]

mod game;
mod generator;
mod widget;

pub use game::{GameStatus, NumberlinkGame};
pub use widget::{content_size, NumberlinkWidget, DEFAULT_COLORS};
