//! Renderer-agnostic Numberlink game logic.

use crate::generator;

/// The current status of the game.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GameStatus {
    /// The puzzle is not yet solved.
    Playing,
    /// Every number's two endpoints are connected and every cell on the
    /// board is filled. Numberlink puzzles built via [`NumberlinkGame::random`]
    /// have a verified-unique solution, so this is *the* solution; puzzles
    /// built via [`NumberlinkGame::from_endpoints`] have no such guarantee.
    Won,
}

/// The Numberlink game state: board dimensions, each number's fixed
/// endpoints, the player's drawn paths, and undo/redo history.
///
/// Numbers are `0..pair_count`, displayed 1-based by convention (number `0`
/// is "1" on the board) — see [`NumberlinkWidget`](crate::NumberlinkWidget).
pub struct NumberlinkGame {
    /// Board width in cells.
    pub width: usize,
    /// Board height in cells.
    pub height: usize,
    endpoints: Vec<((usize, usize), (usize, usize))>,
    /// `paths[n]` is the ordered sequence of cells the player has drawn for
    /// number `n` so far, always starting at one of `endpoints[n]`. Empty
    /// until the player starts drawing that number.
    paths: Vec<Vec<(usize, usize)>>,
    /// Flat `width * height` grid: which number (if any) currently occupies
    /// each cell via a drawn path. Endpoints of a number that hasn't been
    /// drawn yet are *not* reflected here — query [`Self::endpoints`] for
    /// those regardless of path state.
    owner: Vec<Option<usize>>,
    /// Set at the start of a drag, consumed (and possibly pushed to
    /// `undo_stack`) when the drag ends; `None` while no drag is active.
    pending_undo: Option<Vec<Vec<(usize, usize)>>>,
    undo_stack: Vec<Vec<Vec<(usize, usize)>>>,
    redo_stack: Vec<Vec<Vec<(usize, usize)>>>,
    /// Current game status.
    pub status: GameStatus,
}

impl NumberlinkGame {
    /// Build a puzzle from explicit endpoint pairs (one per number, in
    /// display order). Use this for curated puzzles.
    ///
    /// Unlike [`Self::random`], there is no guarantee a full-board,
    /// non-crossing solution exists for arbitrary endpoints — that's on the
    /// caller to ensure if a definite win is wanted.
    ///
    /// # Panics
    ///
    /// Panics if `width`/`height` is zero, any endpoint is out of bounds, an
    /// endpoint pair repeats the same cell for both ends, or any cell is
    /// used as an endpoint by more than one number.
    pub fn from_endpoints(
        width: usize,
        height: usize,
        endpoints: Vec<((usize, usize), (usize, usize))>,
    ) -> Self {
        assert!(width > 0 && height > 0, "board must have at least one cell");
        let mut seen = std::collections::HashSet::new();
        for &(a, b) in &endpoints {
            assert!(a.0 < width && a.1 < height, "endpoint {a:?} out of bounds");
            assert!(b.0 < width && b.1 < height, "endpoint {b:?} out of bounds");
            assert_ne!(a, b, "a pair's two endpoints must be distinct cells");
            assert!(seen.insert(a), "cell {a:?} used as more than one endpoint");
            assert!(seen.insert(b), "cell {b:?} used as more than one endpoint");
        }
        let pair_count = endpoints.len();
        Self {
            width,
            height,
            endpoints,
            paths: vec![Vec::new(); pair_count],
            owner: vec![None; width * height],
            pending_undo: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            status: GameStatus::Playing,
        }
    }

    /// Build a procedurally-generated puzzle with `pair_count` numbers on a
    /// `width x height` board, reproducible via `seed`. The generator
    /// verifies the puzzle has exactly one full-board, non-crossing
    /// solution before returning it.
    ///
    /// # Panics
    ///
    /// Panics if `width * height < pair_count * 2`, or in the practically
    /// unreachable case that generation can't find a verified-unique layout
    /// within its attempt budget (see [`crate::generator`]).
    pub fn random(width: usize, height: usize, pair_count: usize, seed: u64) -> Self {
        let endpoints = generator::generate(width, height, pair_count, seed);
        Self::from_endpoints(width, height, endpoints)
    }

    /// Number of numbered pairs on the board.
    pub fn pair_count(&self) -> usize {
        self.endpoints.len()
    }

    /// The two fixed endpoint cells for `number`.
    pub fn endpoints(&self, number: usize) -> ((usize, usize), (usize, usize)) {
        self.endpoints[number]
    }

    /// The cells of `number`'s path drawn so far, in order from one endpoint
    /// towards the other. Empty if the player hasn't started drawing it.
    pub fn path_cells(&self, number: usize) -> &[(usize, usize)] {
        &self.paths[number]
    }

    /// Which number (if any) currently occupies `cell` via a drawn path.
    /// `None` for an empty cell *or* an endpoint that hasn't been drawn to
    /// yet (use [`Self::number_at`] to also match untouched endpoints).
    pub fn owner_at(&self, cell: (usize, usize)) -> Option<usize> {
        self.owner[self.idx(cell)]
    }

    /// Which number `cell` belongs to for the purpose of starting a drag:
    /// either one of that number's two endpoints (drawn or not), or a cell
    /// currently on its drawn path.
    pub fn number_at(&self, cell: (usize, usize)) -> Option<usize> {
        if let Some(n) = self.owner_at(cell) {
            return Some(n);
        }
        self.endpoints
            .iter()
            .position(|&(a, b)| cell == a || cell == b)
    }

    #[inline]
    fn idx(&self, cell: (usize, usize)) -> usize {
        cell.1 * self.width + cell.0
    }

    fn set_owner(&mut self, cell: (usize, usize), number: Option<usize>) {
        let idx = self.idx(cell);
        self.owner[idx] = number;
    }

    /// Begin (or restart) drawing `number`'s path from `cell`. `cell` must
    /// be one of `number`'s two endpoints or a cell already on its path
    /// (typically decided by [`Self::number_at`]); returns `false` and does
    /// nothing otherwise.
    ///
    /// Both endpoints are always "live": pressing either one clears the
    /// path and starts fresh anchored at *that* cell, even if the path was
    /// originally drawn starting from the other end (a number's two ends
    /// are symmetric, not a fixed direction). Pressing an interior cell of
    /// the current path instead truncates it to the prefix ending there,
    /// keeping whichever end it was drawn from — the standard "grab a
    /// point along the pipe to redraw the tail" gesture.
    pub fn start_drag(&mut self, number: usize, cell: (usize, usize)) -> bool {
        let (a, b) = self.endpoints[number];
        if cell == a || cell == b {
            self.pending_undo = Some(self.paths.clone());
            let old_path = std::mem::take(&mut self.paths[number]);
            for c in old_path {
                self.set_owner(c, None);
            }
            self.paths[number].push(cell);
            self.set_owner(cell, Some(number));
            return true;
        }
        let Some(index) = self.paths[number].iter().position(|&c| c == cell) else {
            return false;
        };
        self.pending_undo = Some(self.paths.clone());
        let removed: Vec<_> = self.paths[number].split_off(index + 1);
        for c in removed {
            self.set_owner(c, None);
        }
        true
    }

    /// Extend, retract, or reject a move of the in-progress drag (started
    /// via [`Self::start_drag`]) towards `cell`:
    /// - Moving onto an empty, orthogonally-adjacent cell extends the path.
    /// - Moving onto the path's own second-to-last cell retracts it by one.
    /// - Moving onto a cell owned by a *different* number is rejected.
    ///
    /// No-op if `number` has no path started (call [`Self::start_drag`]
    /// first) or `cell` is neither adjacent nor a retraction target.
    pub fn drag_to(&mut self, number: usize, cell: (usize, usize)) {
        let Some(&last) = self.paths[number].last() else {
            return;
        };
        if cell == last {
            return;
        }
        if self.paths[number].len() >= 2 && self.paths[number][self.paths[number].len() - 2] == cell
        {
            let removed = self.paths[number].pop().expect("checked len >= 2 above");
            self.set_owner(removed, None);
            return;
        }
        if !generator::is_adjacent(last, cell) {
            return;
        }
        if let Some(owner) = self.owner_at(cell) {
            if owner != number {
                return;
            }
            // Occupied by this same number but not the retraction target
            // above (i.e. the path looping back on itself) — reject.
            return;
        }
        self.paths[number].push(cell);
        self.set_owner(cell, Some(number));
        self.check_win();
    }

    /// End the in-progress drag for `number` (pointer released or
    /// cancelled). If the drag actually changed the board relative to when
    /// [`Self::start_drag`] was called, pushes that prior state onto the
    /// undo stack and clears the redo stack. No-op if no drag is active.
    pub fn end_drag(&mut self, number: usize) {
        let Some(before) = self.pending_undo.take() else {
            return;
        };
        // A path of length <= 1 (just the anchor `start_drag` always seeds,
        // with nothing successfully extended onto it) renders and plays
        // identically to an empty one — no pipe is drawn either way, and
        // the player can pick it back up from the same starting point next
        // time. Treat the two as equal so a drag that starts and
        // immediately fails to extend (e.g. every target rejected) doesn't
        // push a no-op-looking entry onto the undo stack.
        fn normalized(p: &[(usize, usize)]) -> Option<&[(usize, usize)]> {
            (p.len() > 1).then_some(p)
        }
        if normalized(&before[number]) != normalized(&self.paths[number]) {
            self.undo_stack.push(before);
            self.redo_stack.clear();
        }
    }

    /// Clear every number's path (keeping the same puzzle) and undo/redo
    /// history. To get a *different* puzzle, construct a new
    /// [`NumberlinkGame`].
    pub fn reset(&mut self) {
        self.paths = vec![Vec::new(); self.pair_count()];
        self.owner = vec![None; self.width * self.height];
        self.pending_undo = None;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.status = GameStatus::Playing;
    }

    pub fn can_undo(&self) -> bool {
        self.pending_undo.is_none() && !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        self.pending_undo.is_none() && !self.redo_stack.is_empty()
    }

    pub fn undo(&mut self) {
        if self.pending_undo.is_some() {
            return;
        }
        if let Some(prev) = self.undo_stack.pop() {
            let current = self.paths.clone();
            self.redo_stack.push(current);
            self.apply_paths(prev);
        }
    }

    pub fn redo(&mut self) {
        if self.pending_undo.is_some() {
            return;
        }
        if let Some(next) = self.redo_stack.pop() {
            let current = self.paths.clone();
            self.undo_stack.push(current);
            self.apply_paths(next);
        }
    }

    fn apply_paths(&mut self, paths: Vec<Vec<(usize, usize)>>) {
        self.owner = vec![None; self.width * self.height];
        for (n, path) in paths.iter().enumerate() {
            for &c in path {
                self.set_owner(c, Some(n));
            }
        }
        self.paths = paths;
        self.check_win();
    }

    fn check_win(&mut self) {
        let all_connected = (0..self.pair_count()).all(|n| {
            let (a, b) = self.endpoints[n];
            let path = &self.paths[n];
            matches!(path.first(), Some(&f) if f == a || f == b)
                && matches!(path.last(), Some(&l) if l == a || l == b)
                && path.first() != path.last()
        });
        let all_filled = self.owner.iter().all(Option::is_some);
        self.status = if all_connected && all_filled {
            GameStatus::Won
        } else {
            GameStatus::Playing
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small_game() -> NumberlinkGame {
        // 2x2 board, two horizontal pairs: (0,0)-(1,0) and (0,1)-(1,1).
        NumberlinkGame::from_endpoints(2, 2, vec![((0, 0), (1, 0)), ((0, 1), (1, 1))])
    }

    #[test]
    fn drawing_both_pairs_wins() {
        let mut game = small_game();
        game.start_drag(0, (0, 0));
        game.drag_to(0, (1, 0));
        game.end_drag(0);
        assert_eq!(game.status, GameStatus::Playing);

        game.start_drag(1, (0, 1));
        game.drag_to(1, (1, 1));
        game.end_drag(1);
        assert_eq!(game.status, GameStatus::Won);
    }

    #[test]
    fn retracting_removes_the_last_cell() {
        let mut game = NumberlinkGame::from_endpoints(3, 1, vec![((0, 0), (2, 0))]);
        game.start_drag(0, (0, 0));
        game.drag_to(0, (1, 0));
        game.drag_to(0, (2, 0));
        assert_eq!(game.path_cells(0), &[(0, 0), (1, 0), (2, 0)]);

        game.drag_to(0, (1, 0)); // retract
        assert_eq!(game.path_cells(0), &[(0, 0), (1, 0)]);
        assert_eq!(game.owner_at((2, 0)), None);
    }

    #[test]
    fn cannot_step_onto_another_numbers_path() {
        let mut game = small_game();
        game.start_drag(0, (0, 0));
        game.drag_to(0, (1, 0));
        game.end_drag(0);

        game.start_drag(1, (0, 1));
        game.drag_to(1, (1, 0)); // belongs to number 0, must be rejected
        assert_eq!(game.path_cells(1), &[(0, 1)]);
    }

    #[test]
    fn a_drag_that_never_extends_does_not_push_a_no_op_undo_entry() {
        // Regression test: `start_drag` always seeds the path with the
        // anchor cell, so a drag that starts and then has every extension
        // rejected still left a 1-cell "path" (renders/plays identically to
        // untouched) that used to still count as "changed" and push a
        // spurious entry onto the undo stack.
        let mut game = small_game();
        game.start_drag(0, (0, 0));
        game.drag_to(0, (1, 0));
        game.end_drag(0);
        assert!(game.can_undo());

        game.start_drag(1, (0, 1));
        game.drag_to(1, (1, 0)); // belongs to number 0, rejected
        game.end_drag(1);
        // `start_drag` always seeds the anchor cell; a 1-cell path is the
        // expected result of a drag that never successfully extended.
        assert_eq!(game.path_cells(1), &[(0, 1)]);

        game.undo();
        // The one real move (number 0's connection) must be what gets
        // undone, not an invisible no-op from the rejected number-1 drag.
        assert!(game.path_cells(0).is_empty());
        assert!(!game.can_undo());
    }

    #[test]
    fn grabbing_an_interior_cell_truncates_the_path_there() {
        let mut game = NumberlinkGame::from_endpoints(4, 1, vec![((0, 0), (3, 0))]);
        game.start_drag(0, (0, 0));
        game.drag_to(0, (1, 0));
        game.drag_to(0, (2, 0));
        game.end_drag(0);

        // Grabbing an interior cell keeps the prefix up to it and frees the
        // rest, rather than wiping the whole path.
        assert!(game.start_drag(0, (1, 0)));
        assert_eq!(game.path_cells(0), &[(0, 0), (1, 0)]);
        assert_eq!(game.owner_at((2, 0)), None);
    }

    #[test]
    fn grabbing_the_far_endpoint_starts_fresh_from_there() {
        let mut game = NumberlinkGame::from_endpoints(4, 1, vec![((0, 0), (3, 0))]);
        game.start_drag(0, (0, 0));
        game.drag_to(0, (1, 0));
        game.drag_to(0, (2, 0));
        game.end_drag(0);

        // Both endpoints are symmetric: grabbing the *other* one starts a
        // fresh path anchored there, not at the original (0, 0) start.
        assert!(game.start_drag(0, (3, 0)));
        assert_eq!(game.path_cells(0), &[(3, 0)]);
        assert_eq!(game.owner_at((0, 0)), None);
        assert_eq!(game.owner_at((1, 0)), None);
        assert_eq!(game.owner_at((2, 0)), None);
    }

    #[test]
    fn undo_and_redo_restore_prior_and_next_path_state() {
        let mut game = small_game();
        game.start_drag(0, (0, 0));
        game.drag_to(0, (1, 0));
        game.end_drag(0);
        assert_eq!(game.path_cells(0), &[(0, 0), (1, 0)]);

        assert!(game.can_undo());
        game.undo();
        assert!(game.path_cells(0).is_empty());

        assert!(game.can_redo());
        game.redo();
        assert_eq!(game.path_cells(0), &[(0, 0), (1, 0)]);
    }

    #[test]
    #[should_panic(expected = "used as more than one endpoint")]
    fn from_endpoints_rejects_a_cell_reused_across_numbers() {
        NumberlinkGame::from_endpoints(2, 2, vec![((0, 0), (1, 0)), ((0, 0), (1, 1))]);
    }
}
