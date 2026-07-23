//! Puzzle generation: a random full-board tiling of non-crossing paths, cut
//! into `pair_count` numbered segments.
//!
//! Strategy:
//! 1. Build one random Hamiltonian path over the whole `width x height` grid
//!    via randomized backtracking (a rectangular grid graph always has one;
//!    small boards make this fast).
//! 2. Cut it into `pair_count` contiguous segments at random interior
//!    points. Each segment's two ends become one number's fixed endpoints.
//!    This guarantees full-board coverage *and* at least one valid solution
//!    (the one just constructed) by construction, regardless of what
//!    happens next.
//! 3. *Prefer* a verified-unique layout: a bounded-budget backtracking
//!    solver (with connectivity pruning, see `Solver::is_feasible`) counts
//!    full-board, non-crossing path-sets connecting every number's
//!    endpoints, stopping as soon as it finds a second one. Try up to
//!    [`UNIQUENESS_ATTEMPTS`] fresh candidates looking for one the solver
//!    confirms unique within budget; if none turns up, fall back to the
//!    first candidate generated.
//!
//! Unlike Nonogram, global uniqueness isn't load-bearing for Numberlink to
//! be a fair puzzle (a full-board non-crossing path-set connecting the same
//! endpoints is, in practice, rarely the *only* one on boards past a handful
//! of cells — proving otherwise is combinatorially expensive) — the classic
//! implementations of this genre don't insist on it either. Falling back to
//! "at least one solution exists" keeps generation latency bounded and
//! still produces a perfectly playable, winnable puzzle.

type Cell = (usize, usize);

const HAMILTONIAN_BUDGET: u64 = 400_000;
const SOLVE_BUDGET: u64 = 50_000;
/// Safety valve on the outer generate-and-check retry loop (covers the rare
/// case `random_hamiltonian_path` itself comes back empty). Reasonable
/// puzzle sizes succeed on the first or second attempt in practice.
const MAX_ATTEMPTS: u32 = 500;
/// How many constructively-valid candidates to uniqueness-check before
/// giving up on finding one and falling back to the first candidate
/// generated (see the module docs above). Bounds worst-case generation
/// latency to roughly `UNIQUENESS_ATTEMPTS * SOLVE_BUDGET` solver nodes.
const UNIQUENESS_ATTEMPTS: u32 = 30;

fn to_idx(cell: Cell, width: usize) -> usize {
    cell.1 * width + cell.0
}

fn neighbors(cell: Cell, width: usize, height: usize) -> [Option<Cell>; 4] {
    let (x, y) = cell;
    [
        (x > 0).then(|| (x - 1, y)),
        (x + 1 < width).then_some((x + 1, y)),
        (y > 0).then(|| (x, y - 1)),
        (y + 1 < height).then_some((x, y + 1)),
    ]
}

pub(crate) fn is_adjacent(a: Cell, b: Cell) -> bool {
    let dx = a.0.abs_diff(b.0);
    let dy = a.1.abs_diff(b.1);
    (dx == 1 && dy == 0) || (dx == 0 && dy == 1)
}

/// Randomized backtracking search for a Hamiltonian path over the whole
/// `width x height` grid, starting from a random cell. Returns `None` if the
/// node budget runs out before a full path is found (the caller retries with
/// fresh randomness rather than this function retrying internally, so a
/// single generation attempt has one predictable budget).
fn random_hamiltonian_path(
    width: usize,
    height: usize,
    rng: &mut fastrand::Rng,
) -> Option<Vec<Cell>> {
    let n = width * height;
    let start = (rng.usize(..width), rng.usize(..height));
    let mut visited = vec![false; n];
    visited[to_idx(start, width)] = true;
    let mut path = vec![start];
    let mut frames: Vec<Vec<Cell>> = vec![ranked_neighbors(start, width, height, &visited, rng)];
    let mut budget = HAMILTONIAN_BUDGET;

    // `frames.len() == path.len()` always. If a dead end backtracks all the
    // way past the starting cell, `frames` empties out and the loop exits
    // (this start had no Hamiltonian path within budget) rather than
    // indexing into an empty stack.
    while !frames.is_empty() {
        if path.len() == n {
            return Some(path);
        }
        if budget == 0 {
            return None;
        }
        budget -= 1;

        let frame = frames.last_mut().expect("loop condition ensures non-empty");
        let mut advanced = false;
        while let Some(candidate) = frame.pop() {
            let ci = to_idx(candidate, width);
            if !visited[ci] {
                visited[ci] = true;
                path.push(candidate);
                frames.push(ranked_neighbors(candidate, width, height, &visited, rng));
                advanced = true;
                break;
            }
        }
        if !advanced {
            frames.pop();
            let removed = path.pop().expect("frames/path stay in lockstep");
            visited[to_idx(removed, width)] = false;
        }
    }
    None
}

/// Unvisited neighbors of `cell`, ordered so the *most constrained* one
/// (fewest unvisited neighbors of its own) is tried first — since `frame`s
/// are consumed via `Vec::pop`, that means it ends up last in the returned
/// vec. This is Warnsdorff's rule (the standard heuristic for Hamiltonian
/// path/knight's-tour search): visiting a constrained cell early, while it
/// still has options, avoids stranding it for later. Ties are broken
/// randomly. Without this, plain randomized DFS backtracking degrades badly
/// past small boards (dead ends become frequent and expensive to unwind).
fn ranked_neighbors(
    cell: Cell,
    width: usize,
    height: usize,
    visited: &[bool],
    rng: &mut fastrand::Rng,
) -> Vec<Cell> {
    let mut candidates: Vec<Cell> = neighbors(cell, width, height)
        .into_iter()
        .flatten()
        .filter(|&c| !visited[to_idx(c, width)])
        .collect();
    rng.shuffle(&mut candidates);
    let degree = |c: Cell| -> usize {
        neighbors(c, width, height)
            .into_iter()
            .flatten()
            .filter(|&n| !visited[to_idx(n, width)])
            .count()
    };
    candidates.sort_by_key(|&c| std::cmp::Reverse(degree(c)));
    candidates
}

/// Cut a full-board Hamiltonian path into `pair_count` contiguous, non-empty
/// (at least 2 cells each) segments at random interior boundaries.
fn cut_into_segments(path: &[Cell], pair_count: usize, rng: &mut fastrand::Rng) -> Vec<Vec<Cell>> {
    debug_assert!(path.len() >= pair_count * 2);
    // Choose `pair_count - 1` distinct cut points among the `path.len() - 1`
    // gaps between consecutive cells, then locally adjust so every resulting
    // segment keeps at least 2 cells (a single-cell segment can't have two
    // distinct endpoints).
    let mut cuts: Vec<usize> = Vec::with_capacity(pair_count.saturating_sub(1));
    let mut boundary = 0usize;
    for remaining_segments in (1..pair_count).rev() {
        // Every following segment (including this one) needs >= 2 cells, so
        // this cut can land anywhere leaving at least `2 * (remaining + 1)`
        // cells after it and at least 2 cells since the previous boundary.
        let min = boundary + 2;
        let max = path.len() - remaining_segments * 2;
        let cut = if max > min { rng.usize(min..=max) } else { min };
        cuts.push(cut);
        boundary = cut;
    }
    let mut segments = Vec::with_capacity(pair_count);
    let mut start = 0;
    for &cut in &cuts {
        segments.push(path[start..cut].to_vec());
        start = cut;
    }
    segments.push(path[start..].to_vec());
    segments
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SolveOutcome {
    Count(u32),
    BudgetExceeded,
}

struct Solver {
    width: usize,
    height: usize,
    owner: Vec<Option<usize>>,
    heads: Vec<Cell>,
    targets: Vec<Cell>,
    done: Vec<bool>,
    filled: usize,
}

impl Solver {
    fn new(width: usize, height: usize, endpoints: &[(Cell, Cell)]) -> Self {
        let mut owner = vec![None; width * height];
        let mut heads = Vec::with_capacity(endpoints.len());
        let mut targets = Vec::with_capacity(endpoints.len());
        for (n, &(a, b)) in endpoints.iter().enumerate() {
            owner[to_idx(a, width)] = Some(n);
            heads.push(a);
            targets.push(b);
        }
        Self {
            width,
            height,
            owner,
            heads,
            targets,
            done: vec![false; endpoints.len()],
            filled: endpoints.len(),
        }
    }

    fn legal_moves(&self, number: usize) -> Vec<Cell> {
        neighbors(self.heads[number], self.width, self.height)
            .into_iter()
            .flatten()
            .filter(|&c| {
                let idx = to_idx(c, self.width);
                match self.owner[idx] {
                    None => true,
                    // Stepping onto another number's own endpoint would make
                    // that number unfinishable; only ever legal for its own.
                    Some(other) => other == number && c == self.targets[number],
                }
            })
            .collect()
    }

    fn apply(&mut self, number: usize, cell: Cell) {
        self.owner[to_idx(cell, self.width)] = Some(number);
        self.heads[number] = cell;
        self.filled += 1;
        if cell == self.targets[number] {
            self.done[number] = true;
        }
    }

    fn undo(&mut self, number: usize, cell: Cell, prev_head: Cell) {
        self.owner[to_idx(cell, self.width)] = None;
        self.heads[number] = prev_head;
        self.filled -= 1;
        self.done[number] = false;
    }

    /// Necessary (not sufficient) condition for the current partial
    /// assignment to still be completable. A free cell needs enough usable
    /// neighbors (free cells, or the current head position of a
    /// still-unfinished path, which could extend into it) to eventually get
    /// the path-neighbors it needs: 2 distinct ones if it'll end up as some
    /// path's *interior* cell, but only 1 if it's itself a still-pending
    /// target (a fixed endpoint only needs one connection, unlike a source,
    /// which is pre-owned from the start and never shows up here as free).
    /// A free cell short of that can never be completed, so the branch is
    /// dead. This is the standard degree/connectivity prune that makes
    /// backtracking over a path-cover feasible at all; without it the
    /// search degenerates into brute force and blows the node budget on
    /// boards much past 4x4.
    fn is_feasible(&self) -> bool {
        for y in 0..self.height {
            for x in 0..self.width {
                let cell = (x, y);
                if self.owner[to_idx(cell, self.width)].is_some() {
                    continue;
                }
                let usable = neighbors(cell, self.width, self.height)
                    .into_iter()
                    .flatten()
                    .filter(|&n| {
                        let ni = to_idx(n, self.width);
                        self.owner[ni].is_none()
                            || self
                                .heads
                                .iter()
                                .zip(&self.done)
                                .any(|(&h, &done)| !done && h == n)
                    })
                    .count();
                let is_pending_target = self
                    .targets
                    .iter()
                    .zip(&self.done)
                    .any(|(&t, &done)| !done && t == cell);
                let required = if is_pending_target { 1 } else { 2 };
                if usable < required {
                    return false;
                }
            }
        }
        true
    }

    /// Count full-board, non-crossing completions of every path, stopping
    /// early once `limit` is reached. Returns `BudgetExceeded` if the node
    /// budget runs out first (the caller treats that the same as
    /// "not verifiably unique" and regenerates).
    fn count_solutions(&mut self, limit: u32, budget: &mut u64) -> SolveOutcome {
        let mut count = 0u32;
        if self.dfs(limit, budget, &mut count) {
            SolveOutcome::Count(count)
        } else {
            SolveOutcome::BudgetExceeded
        }
    }

    /// Returns `false` on budget exhaustion (search abandoned), `true`
    /// otherwise (including early-stop once `count` reaches `limit`).
    fn dfs(&mut self, limit: u32, budget: &mut u64, count: &mut u32) -> bool {
        if *budget == 0 {
            return false;
        }
        *budget -= 1;

        if self.filled == self.width * self.height {
            if self.done.iter().all(|&d| d) {
                *count += 1;
            }
            return *count < limit;
        }

        // Extend whichever unfinished path currently has the fewest legal
        // moves, so hopeless branches (zero options) are pruned first.
        let mut chosen: Option<(usize, Vec<Cell>)> = None;
        for n in 0..self.heads.len() {
            if self.done[n] {
                continue;
            }
            let moves = self.legal_moves(n);
            if moves.is_empty() {
                return true; // dead end this branch, not a search failure
            }
            if chosen.as_ref().map_or(true, |(_, m)| moves.len() < m.len()) {
                chosen = Some((n, moves));
            }
        }
        let Some((number, moves)) = chosen else {
            // Every path is already done but the board isn't full: some
            // cells are unreachable, dead branch.
            return true;
        };

        for mv in moves {
            let prev_head = self.heads[number];
            self.apply(number, mv);
            let feasible = self.is_feasible();
            let keep_going = if feasible {
                self.dfs(limit, budget, count)
            } else {
                true // dead end, not a search failure; try the next move
            };
            self.undo(number, mv, prev_head);
            if !keep_going || *count >= limit {
                return keep_going;
            }
        }
        true
    }
}

/// Generate `pair_count` numbered endpoint pairs on a `width x height` grid
/// with a full-tiling solution (returned in generation order, number `i`'s
/// endpoints at `result[i]`), preferring one verified unique within a
/// bounded search and falling back to a merely-valid one otherwise (see the
/// module docs above).
///
/// # Panics
///
/// Panics if `width * height < pair_count * 2` (not enough cells for every
/// pair to have at least 2 distinct cells), or in the practically
/// unreachable case that not even one candidate can be constructed within
/// [`MAX_ATTEMPTS`] (indicates the requested board is too large/awkward for
/// this generator's budgets).
pub(crate) fn generate(
    width: usize,
    height: usize,
    pair_count: usize,
    seed: u64,
) -> Vec<(Cell, Cell)> {
    assert!(
        width * height >= pair_count * 2,
        "not enough cells for {pair_count} pairs on a {width}x{height} board"
    );
    let mut rng = fastrand::Rng::with_seed(seed);
    let mut fallback: Option<Vec<(Cell, Cell)>> = None;
    for attempt in 0..MAX_ATTEMPTS {
        let Some(path) = random_hamiltonian_path(width, height, &mut rng) else {
            continue;
        };
        let segments = cut_into_segments(&path, pair_count, &mut rng);
        let endpoints: Vec<(Cell, Cell)> = segments
            .iter()
            .map(|s| (s[0], *s.last().expect("segment has >= 2 cells")))
            .collect();
        if fallback.is_none() {
            fallback = Some(endpoints.clone());
        }
        if attempt >= UNIQUENESS_ATTEMPTS {
            break;
        }
        let mut budget = SOLVE_BUDGET;
        let mut solver = Solver::new(width, height, &endpoints);
        if let SolveOutcome::Count(1) = solver.count_solutions(2, &mut budget) {
            return endpoints;
        }
    }
    fallback.expect(
        "random_hamiltonian_path succeeds within a handful of attempts on any reasonable board",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count_solutions_for(
        width: usize,
        height: usize,
        endpoints: &[(Cell, Cell)],
        limit: u32,
    ) -> SolveOutcome {
        let mut solver = Solver::new(width, height, endpoints);
        let mut budget = SOLVE_BUDGET;
        solver.count_solutions(limit, &mut budget)
    }

    #[test]
    fn is_adjacent_only_true_for_orthogonal_neighbors() {
        assert!(is_adjacent((1, 1), (1, 2)));
        assert!(is_adjacent((1, 1), (2, 1)));
        assert!(!is_adjacent((1, 1), (2, 2)));
        assert!(!is_adjacent((1, 1), (1, 1)));
    }

    #[test]
    fn generate_produces_the_requested_shape() {
        for seed in 0..8 {
            let endpoints = generate(4, 4, 4, seed);
            assert_eq!(endpoints.len(), 4);
            let mut seen = std::collections::HashSet::new();
            for &(a, b) in &endpoints {
                assert_ne!(a, b);
                assert!(seen.insert(a));
                assert!(seen.insert(b));
            }
        }
    }

    #[test]
    fn generate_succeeds_on_doneward_s_actual_difficulty_presets() {
        // Regression test: an earlier version of `random_hamiltonian_path`
        // could backtrack all the way past its starting cell (a dead end
        // with nowhere left to retreat to) and panic on an empty `frames`
        // stack instead of returning `None` for the outer loop to retry.
        // 5x5/4 is exactly Doneward's "Beginner" preset that first
        // triggered it; a plain (non-Warnsdorff) neighbor order also made
        // 9x9/8 ("Expert") fail to find any Hamiltonian path at all within
        // budget. Kept to a handful of seeds since this crate's own dev
        // profile has no opt-level override (unlike Doneward's, which sets
        // `profile.dev.package."*".opt-level = 3` for exactly this reason)
        // and 9x9 is meaningfully slower unoptimized.
        for (width, height, pairs) in [(5, 5, 4), (7, 7, 6), (9, 9, 8)] {
            for seed in 0..3 {
                let endpoints = generate(width, height, pairs, seed);
                assert_eq!(endpoints.len(), pairs);
            }
        }
    }

    #[test]
    fn generate_is_verifiably_unique() {
        let endpoints = generate(4, 4, 3, 42);
        assert_eq!(
            count_solutions_for(4, 4, &endpoints, 2),
            SolveOutcome::Count(1)
        );
    }

    #[test]
    fn a_two_by_two_board_with_two_diagonal_pairs_is_unique() {
        // The only full tiling of a 2x2 board by two 2-cell dominoes with
        // these endpoints is the two horizontal rows.
        let endpoints = [((0, 0), (1, 0)), ((0, 1), (1, 1))];
        assert_eq!(
            count_solutions_for(2, 2, &endpoints, 2),
            SolveOutcome::Count(1)
        );
    }

    #[test]
    fn a_two_by_two_board_with_a_crossing_pair_layout_is_not_unique_or_impossible() {
        // Diagonal corners: the only way to link (0,0)-(1,1) and (1,0)-(0,1)
        // without crossing while filling all 4 cells is impossible for one
        // of the two pairings and forces a specific L-shape for the other;
        // sanity-check the solver terminates with a definite count rather
        // than hanging.
        let endpoints = [((0, 0), (1, 1)), ((1, 0), (0, 1))];
        match count_solutions_for(2, 2, &endpoints, 4) {
            SolveOutcome::Count(_) => {}
            SolveOutcome::BudgetExceeded => panic!("solver should finish instantly on a 2x2 board"),
        }
    }
}
