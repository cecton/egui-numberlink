# egui-numberlink

[![crates.io](https://img.shields.io/crates/v/egui-numberlink.svg)](https://crates.io/crates/egui-numberlink)
[![docs.rs](https://docs.rs/egui-numberlink/badge.svg)](https://docs.rs/egui-numberlink)
[![deps.rs](https://deps.rs/repo/github/cecton/egui-numberlink/status.svg)](https://deps.rs/repo/github/cecton/egui-numberlink)
[![CI](https://github.com/cecton/egui-numberlink/actions/workflows/ci.yml/badge.svg)](https://github.com/cecton/egui-numberlink/actions/workflows/ci.yml)
[![Rust version](https://img.shields.io/badge/rustc-1.80+-ab6000.svg)](https://blog.rust-lang.org/2024/07/25/Rust-1.80.0.html)
[![License](https://img.shields.io/crates/l/egui-numberlink.svg)](https://github.com/cecton/egui-numberlink#license)
[![Changelog](https://img.shields.io/badge/changelog-Keep%20a%20Changelog%20v1.1.0-%23E05735)](CHANGELOG.md)
[![Live demo](https://img.shields.io/badge/demo-live-brightgreen)](https://cecton.github.io/egui-numberlink)

A self-contained Numberlink puzzle game library for [egui](https://github.com/emilk/egui).

Numberlink is the classic puzzle of connecting matching pairs of numbered
endpoints on a grid with a path each, such that no two paths cross and (in
this crate's variant) every cell of the board ends up covered by exactly one
path.

## Features

- Pure game logic struct (`NumberlinkGame`) with no egui dependency — usable headlessly or with any renderer
- Ready-to-use egui `Widget` (`NumberlinkWidget`) that renders an interactive, numbered board
- Two ways to build a puzzle: `NumberlinkGame::from_endpoints` (supply your own endpoints) and `NumberlinkGame::random` (procedural, seeded, verified to have a unique full-board solution)
- Endpoints are always labeled with numbers, like the original Numberlink — color is an optional, fully customizable skin on top, not a replacement
- `NumberlinkWidget::colors` lets embedding apps supply their own per-number palette; defaults to a colorblind-safe built-in one
- Paths are drawn through the interior of each cell (connecting cell centers, turning at a cell's center), never along a cell's border
- Click-and-drag drawing: dragging back over a path's own previous cell retracts it by one; dragging onto a different number's path is rejected
- Undo/redo history, one entry per completed drag
- Win banner drawn over the board once solved

## Usage

Add the dependency:

```toml
[dependencies]
egui-numberlink = "0.1"
```

Then use it in your egui app:

```rust,ignore
use egui_numberlink::{NumberlinkGame, NumberlinkWidget};

// A curated puzzle from explicit endpoint pairs (number 0's endpoints, then
// number 1's, ...):
let mut game = NumberlinkGame::from_endpoints(
    3,
    3,
    vec![((0, 0), (2, 2)), ((0, 2), (1, 2))],
);

// Or a procedural puzzle, reproducible via a seed, verified to have exactly
// one full-board solution:
let mut game = NumberlinkGame::random(6, 6, 5, 42);

// Inside your egui update/UI closure:
ui.add(egui_numberlink::NumberlinkWidget::new(&mut game));

// Customize the per-number palette (must be at least one color; cycles if
// shorter than the puzzle's pair count):
use egui::Color32;
let colors = [Color32::RED, Color32::GREEN, Color32::BLUE];
ui.add(egui_numberlink::NumberlinkWidget::new(&mut game).colors(&colors));
```

After each frame you can inspect `game.status` to check for a win:

```rust,ignore
use egui_numberlink::GameStatus;

match game.status {
    GameStatus::Playing => {}
    GameStatus::Won => println!("Solved!"),
}
```

To start over on the same puzzle:

```rust,ignore
game.reset();
```

## egui version compatibility

| egui-numberlink | egui |
|------------------|------|
| 0.1              | 0.35 |

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.
