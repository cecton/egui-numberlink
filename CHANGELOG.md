# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](keep_a_changelog) and this project adheres to [Semantic
Versioning](semver).

## [Unreleased]

### Added

- Initial release of `egui-numberlink`
- `NumberlinkGame` core game logic API (renderer-agnostic)
- `NumberlinkGame::from_endpoints` to build a puzzle from explicit endpoint pairs
- `NumberlinkGame::random` to build a seeded procedural puzzle: always a full-board, non-crossing solution by construction, preferring (via a connectivity-pruned backtracking solver, within a bounded search) one verified unique
- `NumberlinkWidget` egui widget for interactive board rendering, paths drawn through cell interiors (never along cell borders)
- Click-and-drag drawing with retraction (dragging back over a path's own previous cell shrinks it) and rejection of crossing into another number's path
- `NumberlinkWidget::colors` to customize the per-number palette (defaults to a colorblind-safe built-in one); numbers are always shown on endpoints regardless of color, per the original Numberlink
- Undo/redo history
- Web example and GitHub Pages deployment workflow

[keep_a_changelog]: https://keepachangelog.com/en/1.1.0
[semver]: https://semver.org/spec/v2.0.0.html
[Unreleased]: https://github.com/cecton/egui-numberlink
