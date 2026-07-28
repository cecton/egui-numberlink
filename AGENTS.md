# AGENTS.md

Instructions for AI coding agents working in this repository.

## What this is

`egui-numberlink` is a self-contained Rust library that implements a
Numberlink puzzle game for [egui](https://github.com/emilk/egui):
renderer-agnostic game logic plus a ready-to-use `egui::Widget`. It has no
application of its own beyond the demo in `examples/webapp.rs` — it's meant
to be pulled into other egui apps as a dependency.

Numberlink is the puzzle of connecting matching pairs of numbered endpoints
on a grid with a path each, no two paths crossing — winning only requires
connecting every pair, not filling the whole board (that's a Flow-Free-
specific convention, not a Numberlink one; see `GameStatus::Won`). Some cells
may be permanently blocked (walls no path may ever enter), for hand-authored/
curated puzzles built via `NumberlinkGame::from_endpoints_with_blocked` — the
procedural generator itself never produces blocked cells (see
`src/generator.rs` below). "Flow Free" is a trademarked product name for a
specific commercial game in this genre — never use it in code, docs, or the
crate's own naming; this project only ever refers to the genre by its
generic name, Numberlink.

## Module layout

- `src/game.rs` — `NumberlinkGame`, `GameStatus`. Pure logic, no
  `egui::Widget`/`Ui` usage. Keep it that way: it should stay usable
  headlessly (e.g. for tests or a non-egui renderer) without pulling in any
  painting code.
- `src/generator.rs` — puzzle generation (`pub(crate)` only, not part of the
  public API): a random Hamiltonian path over the whole board (Warnsdorff's
  rule ordering, see `ranked_neighbors` — plain randomized order doesn't
  scale much past 4x4), cut into `pair_count` contiguous segments (one per
  number — full board coverage falls out of this by construction, but it's
  purely an internal construction technique; the player never needs to
  reproduce it, see `GameStatus::Won`). Cutting actively *searches* many
  candidate cut-point configurations and keeps whichever `score_cuts` ranks
  best — both maximizing the worst segment's own endpoint spread *and*
  requiring genuine, *widespread* contention, via two stacked checks:
  `has_forced_contention` (a real, but heavily restricted and therefore
  cheap, solver search: can every pair's own independent shortest route be
  assigned simultaneously without any of them crossing? if so, the puzzle
  needs no rerouting reasoning at all, veto it) confirms *some* conflict
  exists anywhere, and `pairwise_conflicts` additionally requires that
  conflict to involve *every* number — `has_forced_contention` alone was
  satisfied just as well by one small, isolated conflict between two numbers
  while the rest of the board stayed trivially solvable by inspection, which
  still played as "easy" despite technically passing. `has_forced_contention`
  replaced full-board-tiling uniqueness (still what `Solver::new`'s default
  checks, used only by a few tests now) as the real difficulty gate: that
  older check turned out to have ~zero relationship to real difficulty once
  the win condition stopped requiring full coverage, *and* was too expensive
  to run broadly at 7x7/9x9 — motivating (and then eventually making
  unnecessary) an offline-harvested puzzle bank that has since been deleted,
  since the contention checks turned out both common and cheap enough to run
  on every candidate live, at every board size including 9x9. The module doc
  also records two earlier, cheaper geometric proxies for contention
  (bounding-box overlap, cell-proximity) that were tried and measured to be
  near-no-ops before landing on the real, direct check — worth reading
  before reaching for a cheap approximation over an accurately restricted
  real search. Even the current pair, note, only measure *coverage* (how
  many numbers are forced into some conflict), not *severity* (how much
  detour each conflict actually costs) — a real, separate axis nothing here
  measures yet. `score_cuts` also vetoes a pair whose *both* endpoints sit on
  the board's outer border (`is_periphery`) — such a pair is always
  trivially joinable by just walking the border, no interior reasoning
  needed, regardless of contention elsewhere; one endpoint on the border is
  fine, only both is rejected. This one is real but not free: it measurably
  raised `generate`'s average latency (roughly 4-12x across presets, still
  well under a second worst case on an unoptimized dev build) since some
  Hamiltonian paths admit no valid cut at all under the added constraint,
  forcing a full-path retry — raising `CUT_ATTEMPTS` doesn't help (measured),
  since the bottleneck is which *paths* work, not how many cuts are tried
  per path. See the module doc for the exact numbers. A separate, persistent
  (not throwaway) `difficulty_survey`
  test module at the bottom of the file — every test in it `#[ignore]`d — is
  an opt-in offline harness for spending much larger amounts of compute than
  the live generator can afford to measure real puzzle difficulty/contention;
  see its own doc comment for how to run it at a much larger sample size.
  Internal detail of `NumberlinkGame::random`; keep it that way unless
  there's a concrete need for lower-level access.
- `src/widget.rs` — `NumberlinkWidget`, `DEFAULT_COLORS`, `content_size`, and
  all painting/input handling. This is the only file allowed to depend on
  `egui::Ui`/`Painter`.
- `src/lib.rs` — thin re-export surface. `#![doc = include_str!("../README.md")]`
  means the crate-level docs are the README; keep the two in sync (usage
  snippets especially).
- `examples/webapp.rs` — a wasm demo app (via `xtask-wasm`), deployed to
  GitHub Pages by `.github/workflows/pages.yml` on every push to `main`. Not
  part of the published crate (`Cargo.toml` excludes `/examples`).

## Rendering convention (easy to get wrong, so it's called out here)

Paths are drawn **through the interior of each cell they occupy** —
`line_segment`s connect consecutive cells' *centers*, turning at a cell's
center on a corner — never along a cell's border/edge. A border-following
line reads as a wall between cells, not a connection through them; that is
the one mistake to specifically check for after touching `widget.rs`'s
painting code.

## Building and testing

```sh
cargo check
cargo test --lib
cargo clippy -- -D warnings
cargo fmt --check
```

These four are exactly what `.github/workflows/ci.yml` runs on every push
and PR. Run them locally before committing.

The wasm demo isn't covered by `ci.yml` (only `pages.yml` builds it, on push
to `main`). If you touch `examples/webapp.rs`, check it manually:

```sh
cargo check --target wasm32-unknown-unknown --example webapp
cargo clippy --target wasm32-unknown-unknown --example webapp -- -D warnings
```

## Conventions

- Numbers are always the primary identifier (`NumberlinkWidget::show_numbers`
  defaults to `true`); color (`NumberlinkWidget::colors`) is an optional skin
  on top, never a replacement. If a monochrome/numbers-only rendering mode is
  ever wanted, add it as a new, additive option — don't make numbers
  removable without a color alternative in place.
- No losing state. Every player action must be reversible via undo, or by
  simply redrawing a number's path (grabbing it anywhere resets it back to
  its original starting cell, see `NumberlinkGame::start_drag`'s doc
  comment).
- `NumberlinkGame::random`'s generated puzzles must always have *at least
  one* solution by construction (a full-board-coverage tiling, internally —
  see `generator.rs`'s module docs), and prefer (without insisting on) one
  confirmed to genuinely and widely require rerouting (`has_forced_contention`
  plus `pairwise_conflicts`, not full-board-tiling uniqueness — see
  `generator.rs`'s module docs for why). The win condition (`check_win` in
  `game.rs`) only checks "every pair
  connected" — filling the rest of the board was never required, matching
  the classic Numberlink rules rather than Flow Free's convention. Blocked
  cells (`is_blocked`) remain a real routing obstacle regardless — a path
  may never enter one — they just aren't also required to be used by
  someone.
- `NumberlinkGame::from_endpoints`/`from_endpoints_with_blocked` (curated
  puzzles) have no such solvability guarantee — that's documented as being
  on the caller.
- Add unit tests in `src/game.rs` for player-facing behavior (drag/retract/
  reject, win detection, undo/redo) and in `src/generator.rs` for generation
  properties (shape of the output, verified uniqueness). The widget/painting
  code isn't unit-testable the same way; verify it by eye via the wasm demo.
- Keep the crate's public API renderer-agnostic where possible: prefer
  exposing queries (`path_cells`, `owner_at`, `number_at`, etc.) over raw
  field access, so the internal representation can change without breaking
  callers.

## Release process

Every published version gets a git tag and a changelog entry. To cut a release:

1. Update `CHANGELOG.md`: move the `[Unreleased]` section's contents under a
   new `## [X.Y.Z] - YYYY-MM-DD` heading (Keep a Changelog format), and add
   the corresponding link reference at the bottom of the file.
2. Bump the `version` in `Cargo.toml` to match.
3. Run the full check suite above, plus `cargo package --list` as a final
   sanity check of what will actually be published.
4. `cargo publish`. This is irreversible per-version (a bad release can only
   be `cargo yank`-ed, not deleted) — don't skip step 3.
5. `git tag vX.Y.Z && git push && git push --tags`.

Follow SemVer: breaking changes (renamed/removed public items, changed
method signatures) require a major version bump (or a minor bump pre-1.0,
per SemVer's pre-1.0 rules).
