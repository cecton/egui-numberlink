# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](keep_a_changelog) and this project adheres to [Semantic
Versioning](semver).

## [Unreleased]

## [0.2.0] - 2026-07-26

### Changed

- **Breaking:** `GameStatus::Won` no longer requires filling every non-blocked cell — only connecting every pair. Filling the whole board was a Flow-Free-specific convention, not a Numberlink one; requiring it turned out to add no real difficulty once endpoints were well spread (see the next bullet), just mechanical clean-up after the puzzle was already effectively solved. Blocked cells remain a real routing obstacle (a path may never enter one), they just aren't also required to be used by someone
- **Breaking:** `NumberlinkGame::random`'s generator was reworked to fix generated pairs' numbers often ending up right next to each other, making puzzles trivially easy: it still builds one Hamiltonian path over the whole board, but now actively *searches* many candidate cut-point configurations and keeps whichever spreads every segment's own two endpoints apart the most (relative to that segment's own length), instead of accepting the first cut whose endpoints simply aren't touching. The generator never produces blocked cells, so `NumberlinkGame::random` drops the `fill_density` parameter it briefly took
- `NumberlinkGame::from_endpoints` now delegates to a new `NumberlinkGame::from_endpoints_with_blocked`, which also accepts a set of blocked cells (walls no path may ever enter) for hand-authored/curated puzzles — not produced by the generator itself
- `NumberlinkWidget` renders blocked cells with a distinct fill and diagonal hatch so they read as walls rather than empty space still waiting to be drawn on
- `GameStatus::Won`'s and `NumberlinkGame::random`'s doc comments now describe the generator's uniqueness check honestly as best-effort (preferred, not guaranteed), matching what `generator.rs`'s module doc already said
- The web demo's difficulty presets no longer take a `fill_density` argument, matching the generator's simplified signature

## [0.1.1] - 2026-07-24

### Added

- Mobile/narrow-mode layout for the web demo: bottom action bar with Pan/Draw mode toggle, pannable/zoomable board, and hamburger menu for preset selection, new game, and theme

### Fixed

- Drag through undrawn endpoints and past own completed endpoint no longer allowed

## [0.1.0] - 2026-07-23

### Added

- Initial release of `egui-numberlink`
- `NumberlinkGame` core game logic API (renderer-agnostic)
- `NumberlinkGame::from_endpoints` to build a puzzle from explicit endpoint pairs
- `NumberlinkGame::random` to build a seeded procedural puzzle: always a full-board, non-crossing solution by construction, preferring (via a connectivity-pruned backtracking solver, within a bounded search) one verified unique; segment cutting is biased towards even lengths and rejects (with retries) any pair whose two endpoints land directly adjacent, so puzzles don't end up with a few trivial one-step pairs and the leftover slack forced onto some other pair as an oversized detour
- `NumberlinkWidget` egui widget for interactive board rendering, paths drawn through cell interiors (never along cell borders); endpoint digits use a large monospace font (scaled to the board's cell size, matching `egui-minesweeper`'s number rendering) so they stay legible at any board size
- Click-and-drag drawing: either endpoint of a number always works to start/redraw its path (grabbing one always starts fresh from there, even if the path was drawn from the other end); grabbing an interior cell truncates the path to that point; dragging back over a path's own previous cell retracts it by one; dragging onto a different number's path is rejected; a plain click (no drag needed) also clears/starts a path
- `NumberlinkWidget::colors` to customize the per-number palette (defaults to a colorblind-safe built-in one); numbers are always shown on endpoints regardless of color, per the original Numberlink
- Undo/redo history
- Web example and GitHub Pages deployment workflow

[keep_a_changelog]: https://keepachangelog.com/en/1.1.0
[semver]: https://semver.org/spec/v2.0.0.html
[Unreleased]: https://github.com/cecton/egui-numberlink/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/cecton/egui-numberlink/releases/tag/v0.2.0
[0.1.1]: https://github.com/cecton/egui-numberlink/releases/tag/v0.1.1
[0.1.0]: https://github.com/cecton/egui-numberlink/releases/tag/v0.1.0
