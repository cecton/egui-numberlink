# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](keep_a_changelog) and this project adheres to [Semantic
Versioning](semver).

## [Unreleased]

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
[Unreleased]: https://github.com/cecton/egui-numberlink/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/cecton/egui-numberlink/releases/tag/v0.1.1
[0.1.0]: https://github.com/cecton/egui-numberlink/releases/tag/v0.1.0
