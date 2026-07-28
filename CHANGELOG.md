# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](keep_a_changelog) and this project adheres to [Semantic
Versioning](semver).

## [Unreleased]

### Changed

- `NumberlinkGame::random`'s generator now selects for genuine difficulty: a candidate cut is only kept if the pairs' own independent shortest routes *can't* all be assigned simultaneously without crossing (`has_forced_contention`), forcing a player to actually notice and reroute around another pair to solve it, rather than just drawing each pair's shortest path in isolation. This replaces an internal, uniqueness-of-the-full-board-tiling check that turned out to have essentially no relationship to real difficulty once the win condition stopped requiring full coverage — a puzzle could be a *unique* full tiling while still being trivially solvable, since the player was never required to fill the board at all. The new check is both common (found in 20-67% of random candidates, depending on board size) and cheap enough to check on every candidate at generation time, at every board size including 9x9 — unlike the old check, which had become so expensive at 7x7/9x9 that an offline-precomputed puzzle bank existed for those sizes at one point during development; that bank is gone now that live generation handles every size directly
- Added an opt-in, offline difficulty-survey test harness (`difficulty_survey` in `src/generator.rs`, every test `#[ignore]`d) for measuring real puzzle difficulty/contention at a much larger sample size than the live generator can afford — not run by `cargo test`/CI by default
- Fixed a solver bug where reaching the requested solution-count limit exactly (rather than exhausting the search or running out of node budget) was misreported as budget exhaustion instead of the actual count found
- `NumberlinkGame::random`'s generator now also requires contention to be widespread, not just present: `has_forced_contention` alone was satisfied just as well by one small, isolated conflict between two numbers as by a puzzle where every number is entangled with someone, which still played easy overall since the rest of the board stayed trivially solvable by inspection. A new check (`pairwise_conflicts`) additionally requires every number to participate in at least one unavoidable conflict with another. Still a coverage measure, not a severity one — a real, separate axis (how much detour each conflict actually costs) that isn't measured yet
- The web demo's Intermediate and Expert presets are denser (7x7/7, 9x9/9, up from 7x7/6 and 9x9/8), matching Flow Free's convention of roughly one pair per side length rather than one fewer; confirmed both stay fast (~15ms average) and 100% genuinely contended at the new density. Beginner stays at 5x5/4: bumping it to 5x5/5 was measured to regress badly (543ms average, 1.2s worst case, and only 27/30 seeds even contended) since a 25-cell board split 5 ways is too tight for the cutting search to reliably land a good candidate
- `NumberlinkGame::random`'s generator now also rejects any pair whose *both* endpoints sit on the board's outer border: such a pair could always be solved by just walking the border itself, no interior reasoning needed, regardless of how contended the rest of the board was — one endpoint on the border remains fine, only both is vetoed. This measurably slows generation (roughly 4-12x average latency across presets, still under a second worst case) since some Hamiltonian paths admit no valid cut at all once this is required, forcing a full-path retry; raising the cut-attempt budget doesn't help, since the bottleneck is which paths work at all, not how many cuts are tried per path
- `DEFAULT_COLORS` gained a 9th color (purple) — the web demo's Expert preset moving to 9 pairs (see above) made pair 9 wrap `colors[number % colors.len()]` back onto pair 1's blue

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
