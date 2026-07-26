//! Puzzle generation: a full-board tiling of non-crossing paths, cut out of
//! one Hamiltonian walk.
//!
//! Strategy:
//! 1. Build one random Hamiltonian path over the whole `width x height` grid
//!    via randomized backtracking guided by Warnsdorff's rule (a rectangular
//!    grid graph always has one; small boards make this fast — see
//!    [`random_hamiltonian_path`]).
//! 2. Cut it into `pair_count` contiguous segments, one per number. Full
//!    board coverage falls out of this by construction (every cell ends up
//!    in exactly one segment), but that's an internal construction/
//!    verification detail, not something the player is required to
//!    reproduce — see [`crate::game::GameStatus::Won`]. A Warnsdorff-guided
//!    path is *locally coherent* — it tends to fill one small neighborhood
//!    before moving to the next — so a single random cut often lands on a
//!    segment whose own two endpoints fold back close to each other despite
//!    the segment having plenty of cells; a fixed accept/reject threshold
//!    against one random cut was tried and measured to barely help (~5%),
//!    since it only ever evaluated one candidate. Instead,
//!    [`cut_into_segments`] actively *searches*: it tries [`CUT_ATTEMPTS`]
//!    independent random cut-point configurations and keeps whichever
//!    maximizes the worst segment's own endpoint spread (see [`score_cuts`])
//!    — trying many candidates and keeping the best is what actually finds
//!    cuts landing on the path's straighter stretches.
//! 3. *Prefer* a verified-unique layout: a bounded-budget backtracking
//!    solver (with the standard degree/connectivity pruning, see
//!    `Solver::is_feasible`) counts full-board, non-crossing path
//!    assignments connecting every number's endpoints, stopping as soon as
//!    it finds a second one. Try up to [`UNIQUENESS_ATTEMPTS`] fresh
//!    candidates looking for one the solver confirms unique within budget;
//!    if none turns up, fall back to whichever candidate scored best in
//!    step 2 across every attempt made.
//!
//! Global uniqueness isn't formally guaranteed by this generator (proving it
//! in general is combinatorially expensive) — the classic implementations of
//! this genre don't insist on it either. Falling back to "at least one
//! solution exists" keeps generation latency bounded and still produces a
//! perfectly playable, winnable puzzle.
//!
//! This generator never produces blocked cells — the win condition only
//! requires connecting every pair, not filling the whole board (see
//! [`crate::game::GameStatus::Won`]), so full coverage here is purely an
//! internal technique that keeps both Hamiltonian construction and
//! uniqueness verification tractable. Permanently blocked cells (walls)
//! remain supported for hand-authored/curated puzzles via
//! [`crate::game::NumberlinkGame::from_endpoints_with_blocked`], just not
//! produced by this generator.
//!
//! Endpoint spread alone (step 2) does *not* guarantee genuine contention
//! between pairs — a bounding-box-overlap check was tried as a cheap proxy
//! for "a player's naive shortest-path-per-pair strategy would conflict
//! somewhere" and measured to be satisfied 95-99% of the time even on
//! *unsearched* random cuts, so it vetoed almost nothing; a stronger,
//! cell-proximity-based variant fared no better (touching in 99-100% of
//! random cuts). Within this single-Hamiltonian-path architecture, most
//! segments already end up spatially close to several others just from the
//! path winding through a small board, regardless of cut choice — so cheap
//! post-hoc proxies checked on the *finished* segmentation can't
//! meaningfully discriminate. Making genuine contention (in the sense of
//! "two pairs' plausible routes could actually conflict, forcing a player to
//! notice and reroute") a first-class, checkable property would need a
//! different construction — e.g. growing multiple regions that compete for
//! cells as they're built, rather than slicing one finished path after the
//! fact — which is a larger change than this generator currently attempts.

type Cell = (usize, usize);
/// A cut-point configuration's `(min_reach_fraction, mean_reach_fraction)`,
/// as computed by [`score_cuts`].
type CutScore = (f64, f64);

const HAMILTONIAN_BUDGET: u64 = 400_000;
const SOLVE_BUDGET: u64 = 50_000;
/// Safety valve on the outer generate-and-check retry loop (covers the rare
/// case `random_hamiltonian_path` itself comes back empty). Reasonable
/// puzzle sizes succeed on the first or second attempt in practice.
const MAX_ATTEMPTS: u32 = 500;
/// How many constructively-valid candidates to uniqueness-check before
/// giving up on finding one and falling back to the best-scoring candidate
/// generated (see the module docs above). Bounds worst-case generation
/// latency to roughly `UNIQUENESS_ATTEMPTS * SOLVE_BUDGET` solver nodes.
const UNIQUENESS_ATTEMPTS: u32 = 30;
/// How many independent random cut-point configurations [`cut_into_segments`]
/// tries per Hamiltonian path before keeping whichever scored best. A
/// candidate cut is O(`pair_count`) with no backtracking search at all, so
/// being generous here is nearly free — see the module doc for why trying
/// many candidates (instead of one accept/reject shot) is the actual fix for
/// segments folding back on themselves.
const CUT_ATTEMPTS: u32 = 500;
/// Minimum Manhattan distance a segment's own two endpoints must clear to
/// count as anything other than degenerate in [`score_cuts`] — without this,
/// a straight 2-cell segment would score a perfect reach fraction of `1.0`
/// despite being trivially adjacent.
const PAIR_DISTANCE_FLOOR: usize = 3;

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

fn manhattan(a: Cell, b: Cell) -> usize {
    a.0.abs_diff(b.0) + a.1.abs_diff(b.1)
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

/// One candidate: `pair_count - 1` distinct cut points among the
/// `path_len - 1` gaps between consecutive cells, chosen uniformly at
/// random and retried (cheap — no search involved) until every resulting
/// segment has at least 2 cells. Deliberately *not* biased towards even
/// segment lengths: [`cut_into_segments`] tries many candidates and keeps
/// whichever scores best, so an uneven-but-well-spread cut winning over an
/// even-but-folded one is exactly the point — an evenness prior would only
/// narrow the search.
fn random_cut_points(path_len: usize, pair_count: usize, rng: &mut fastrand::Rng) -> Vec<usize> {
    debug_assert!(path_len >= pair_count * 2);
    loop {
        let mut cuts: Vec<usize> = (0..pair_count - 1)
            .map(|_| rng.usize(1..path_len))
            .collect();
        cuts.sort_unstable();
        cuts.dedup();
        if cuts.len() != pair_count - 1 {
            continue; // collision: retry with fresh random cuts
        }
        let mut boundary = 0;
        let mut ok = true;
        for &cut in &cuts {
            if cut - boundary < 2 {
                ok = false;
                break;
            }
            boundary = cut;
        }
        if ok && path_len - boundary >= 2 {
            return cuts;
        }
    }
}

/// Score one candidate segmentation as `(min_reach_fraction,
/// mean_reach_fraction)`, compared lexicographically by [`cut_into_segments`]
/// (maximize the worst segment first, break ties by the mean). For a
/// segment of `L` cells, `reach_fraction = manhattan(first, last) / (L - 1)`
/// — how close the segment's actual endpoint spread comes to its own
/// theoretical straight-line maximum. Maximizing the *minimum* (not just the
/// average) specifically targets a segment that folds back near its own
/// start despite having plenty of cells, since an average-based score would
/// let one bad segment hide among otherwise-good ones. A segment whose raw
/// Manhattan distance falls under [`PAIR_DISTANCE_FLOOR`] forces this
/// candidate's score to `(0.0, 0.0)`, vetoing it (a straight 2-cell segment
/// would otherwise score a perfect `1.0` fraction despite being trivially
/// adjacent). Path *shape* itself is never shown to the player before
/// solving — only the two endpoints are — so a high reach fraction (even a
/// dead-straight segment) is purely a good thing here, unlike in generators
/// that route each pair independently and must also worry about the drawn
/// path looking obvious.
fn score_cuts(segments: &[Vec<Cell>]) -> CutScore {
    let fractions: Vec<f64> = segments
        .iter()
        .map(|s| {
            let first = s[0];
            let last = *s.last().expect("segment has >= 2 cells");
            let dist = manhattan(first, last);
            if dist < PAIR_DISTANCE_FLOOR {
                return 0.0;
            }
            dist as f64 / (s.len() - 1) as f64
        })
        .collect();
    if fractions.contains(&0.0) {
        return (0.0, 0.0);
    }
    let min = fractions.iter().copied().fold(f64::INFINITY, f64::min);
    let mean = fractions.iter().sum::<f64>() / fractions.len() as f64;
    (min, mean)
}

/// Cut a full-board Hamiltonian `path` into `pair_count` contiguous,
/// non-empty segments (every cell ends up in exactly one). Actively
/// searches up to [`CUT_ATTEMPTS`] random cut configurations (see
/// [`random_cut_points`]) and keeps whichever [`score_cuts`] ranks best,
/// rather than accepting the first structurally-valid one — see the module
/// doc for why this search is what actually fixes segments folding back on
/// themselves.
fn cut_into_segments(path: &[Cell], pair_count: usize, rng: &mut fastrand::Rng) -> Vec<Vec<Cell>> {
    let mut best: Option<(Vec<Vec<Cell>>, CutScore)> = None;
    for _ in 0..CUT_ATTEMPTS {
        let cuts = random_cut_points(path.len(), pair_count, rng);
        let mut segments = Vec::with_capacity(pair_count);
        let mut start = 0;
        for &cut in &cuts {
            segments.push(path[start..cut].to_vec());
            start = cut;
        }
        segments.push(path[start..].to_vec());

        let score = score_cuts(&segments);
        let is_better = match &best {
            Some((_, best_score)) => score > *best_score,
            None => true,
        };
        if is_better {
            best = Some((segments, score));
        }
    }
    best.expect("CUT_ATTEMPTS > 0, so at least one attempt was made")
        .0
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
    /// `blocked[idx]` is `true` when cell `idx` is a permanent wall no path
    /// may ever enter — excluded from both the full-coverage requirement
    /// and every "usable neighbor" count.
    blocked: Vec<bool>,
    heads: Vec<Cell>,
    targets: Vec<Cell>,
    /// `target_of[idx]` is `Some(n)` when cell `idx` is number `n`'s target
    /// endpoint — precomputed once so `legal_moves` can cheaply block
    /// entering a *different*, still-pending number's target without
    /// scanning every number's `targets` entry each time.
    target_of: Vec<Option<usize>>,
    done: Vec<bool>,
    /// Number of non-blocked cells currently owned by some number — a
    /// solution is only complete once this reaches [`Self::usable_cells`]
    /// *and* every number is done (a number can be done — its head at its
    /// target — while unrelated cells elsewhere are still unclaimed).
    filled: usize,
    /// Total non-blocked cells on the board — the full-coverage target.
    usable_cells: usize,
}

impl Solver {
    fn new(
        width: usize,
        height: usize,
        endpoints: &[(Cell, Cell)],
        blocked_cells: &[Cell],
    ) -> Self {
        let mut owner = vec![None; width * height];
        let mut blocked = vec![false; width * height];
        for &c in blocked_cells {
            blocked[to_idx(c, width)] = true;
        }
        let mut target_of = vec![None; width * height];
        let mut heads = Vec::with_capacity(endpoints.len());
        let mut targets = Vec::with_capacity(endpoints.len());
        for (n, &(a, b)) in endpoints.iter().enumerate() {
            owner[to_idx(a, width)] = Some(n);
            target_of[to_idx(b, width)] = Some(n);
            heads.push(a);
            targets.push(b);
        }
        let usable_cells = width * height - blocked.iter().filter(|&&b| b).count();
        Self {
            width,
            height,
            owner,
            blocked,
            heads,
            targets,
            target_of,
            done: vec![false; endpoints.len()],
            filled: endpoints.len(),
            usable_cells,
        }
    }

    /// A cell is enterable if it's not a wall, not already owned, and not a
    /// *different*, still-pending number's target (stepping onto that would
    /// make the other number unfinishable; only ever legal for its own path
    /// to reach its own target).
    fn enterable(&self, number: usize, cell: Cell) -> bool {
        let idx = to_idx(cell, self.width);
        if self.blocked[idx] || self.owner[idx].is_some() {
            return false;
        }
        !matches!(self.target_of[idx], Some(other) if other != number && !self.done[other])
    }

    fn legal_moves(&self, number: usize) -> Vec<Cell> {
        neighbors(self.heads[number], self.width, self.height)
            .into_iter()
            .flatten()
            .filter(|&c| self.enterable(number, c))
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
    /// assignment to still be completable. A free, non-blocked cell needs
    /// enough usable neighbors (free non-blocked cells, or the current head
    /// position of a still-unfinished path, which could extend into it) to
    /// eventually get the path-neighbors it needs: 2 distinct ones if it'll
    /// end up as some path's *interior* cell, but only 1 if it's itself a
    /// still-pending target. A free cell short of that can never be
    /// completed, so the branch is dead. This is the standard
    /// degree/connectivity prune that makes backtracking over a
    /// (blocked-cell-aware) path-cover feasible at all; without it the
    /// search degenerates into brute force and blows the node budget on
    /// boards much past 4x4.
    fn is_feasible(&self) -> bool {
        for y in 0..self.height {
            for x in 0..self.width {
                let cell = (x, y);
                let idx = to_idx(cell, self.width);
                if self.blocked[idx] || self.owner[idx].is_some() {
                    continue;
                }
                let usable = neighbors(cell, self.width, self.height)
                    .into_iter()
                    .flatten()
                    .filter(|&n| {
                        let ni = to_idx(n, self.width);
                        !self.blocked[ni]
                            && (self.owner[ni].is_none()
                                || self
                                    .heads
                                    .iter()
                                    .zip(&self.done)
                                    .any(|(&h, &done)| !done && h == n))
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

    /// Count full-coverage-of-non-blocked-cells, non-crossing completions
    /// of every path, stopping early once `limit` is reached. Returns
    /// `BudgetExceeded` if the node budget runs out first (the caller
    /// treats that the same as "not verifiably unique" and regenerates).
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

        if self.filled == self.usable_cells {
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
            // Every path is already done but non-blocked cells remain
            // unclaimed: some are unreachable, dead branch.
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
/// with a full-board tiling solution (endpoints returned in generation
/// order, number `i`'s endpoints at `result[i]`). Prefers a layout verified
/// unique within a bounded search and falls back to whichever candidate
/// scored best by [`score_cuts`] across every attempt made otherwise (see
/// the module docs above).
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
    let mut fallback: Option<(Vec<(Cell, Cell)>, CutScore)> = None;
    for attempt in 0..MAX_ATTEMPTS {
        let Some(path) = random_hamiltonian_path(width, height, &mut rng) else {
            continue;
        };
        let segments = cut_into_segments(&path, pair_count, &mut rng);
        let score = score_cuts(&segments);
        let endpoints: Vec<(Cell, Cell)> = segments
            .iter()
            .map(|s| (s[0], *s.last().expect("segment has >= 2 cells")))
            .collect();
        let is_better = match &fallback {
            Some((_, best)) => score > *best,
            None => true,
        };
        if is_better {
            fallback = Some((endpoints.clone(), score));
        }
        // A degenerate score (some segment fell under `PAIR_DISTANCE_FLOOR`)
        // means this Hamiltonian path's fold geometry didn't yield a single
        // good cut in `CUT_ATTEMPTS` tries — retry with a fresh path rather
        // than ever accepting it, even if it happens to be uniquely
        // solvable: a trivially-adjacent pair is exactly the defect this
        // generator exists to avoid.
        if score.0 == 0.0 {
            continue;
        }
        if attempt >= UNIQUENESS_ATTEMPTS {
            break;
        }
        let mut budget = SOLVE_BUDGET;
        let mut solver = Solver::new(width, height, &endpoints, &[]);
        if let SolveOutcome::Count(1) = solver.count_solutions(2, &mut budget) {
            return endpoints;
        }
    }
    fallback
        .expect(
            "random_hamiltonian_path succeeds within a handful of attempts on any reasonable board",
        )
        .0
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
        let mut solver = Solver::new(width, height, endpoints, &[]);
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
    fn generated_pairs_are_never_trivially_adjacent() {
        // Regression test: `cut_into_segments` used to pick cut points
        // uniformly across the whole remaining range, making a 2-cell
        // segment (endpoints directly touching, so the only "solution" is
        // the trivial one-step join) about as likely as one big enough to
        // eat most of the board — reported as several adjacent-looking
        // pairs plus a couple of forced, oversized snakes on Intermediate.
        for (width, height, pairs) in [(5, 5, 4), (7, 7, 6), (9, 9, 8)] {
            for seed in 0..10 {
                let endpoints = generate(width, height, pairs, seed);
                for &(a, b) in &endpoints {
                    assert!(
                        !is_adjacent(a, b),
                        "pair {a:?}-{b:?} is directly adjacent on a {width}x{height}/{pairs} \
                         board (seed {seed})"
                    );
                }
            }
        }
    }

    #[test]
    fn generate_is_verifiably_unique() {
        let endpoints = generate(4, 4, 3, 42);
        let mut solver = Solver::new(4, 4, &endpoints, &[]);
        let mut budget = SOLVE_BUDGET;
        assert_eq!(
            solver.count_solutions(2, &mut budget),
            SolveOutcome::Count(1)
        );
    }

    #[test]
    fn cut_into_segments_partitions_the_whole_path() {
        let mut rng = fastrand::Rng::with_seed(7);
        for (width, height, pairs) in [(5, 5, 4), (7, 7, 6), (9, 9, 8)] {
            let mut path = None;
            for _ in 0..MAX_ATTEMPTS {
                if let Some(p) = random_hamiltonian_path(width, height, &mut rng) {
                    path = Some(p);
                    break;
                }
            }
            let path = path.expect("small boards find a Hamiltonian path within a few attempts");
            let segments = cut_into_segments(&path, pairs, &mut rng);
            assert_eq!(segments.len(), pairs);
            let mut rebuilt = Vec::with_capacity(path.len());
            for segment in &segments {
                assert!(
                    segment.len() >= 2,
                    "segment {segment:?} has fewer than 2 cells"
                );
                rebuilt.extend(segment.iter().copied());
            }
            assert_eq!(
                rebuilt, path,
                "segments must concatenate back to the original path in order"
            );
        }
    }

    #[test]
    fn cut_into_segments_finds_well_spread_endpoints_across_seeds() {
        // The actual regression test for the "numbers in front of each
        // other" complaint: assert real numeric spread, not just
        // non-adjacency, across many seeds/presets. Mirrors `generate`'s own
        // retry-on-degenerate behavior: not every single Hamiltonian path has
        // a non-degenerate cut within `CUT_ATTEMPTS` (measured directly: on
        // the smallest preset, a real but small fraction of paths don't),
        // so retry with fresh paths the same way `generate` does rather than
        // asserting the very first path always works.
        for (width, height, pairs) in [(5, 5, 4), (7, 7, 6), (9, 9, 8)] {
            for seed in 0..20 {
                let mut rng = fastrand::Rng::with_seed(seed);
                let mut segments = None;
                for _ in 0..MAX_ATTEMPTS {
                    let Some(path) = random_hamiltonian_path(width, height, &mut rng) else {
                        continue;
                    };
                    let candidate = cut_into_segments(&path, pairs, &mut rng);
                    if score_cuts(&candidate).0 > 0.0 {
                        segments = Some(candidate);
                        break;
                    }
                }
                let segments = segments
                    .expect("a non-degenerate cut should be found within MAX_ATTEMPTS fresh paths");
                let (min_fraction, _mean) = score_cuts(&segments);
                assert!(
                    min_fraction > 0.0,
                    "{width}x{height}/{pairs} seed {seed}: every segment should clear \
                     PAIR_DISTANCE_FLOOR, got min_fraction {min_fraction}"
                );
                for segment in &segments {
                    let first = segment[0];
                    let last = *segment.last().unwrap();
                    let dist = manhattan(first, last);
                    assert!(
                        dist >= PAIR_DISTANCE_FLOOR,
                        "{width}x{height}/{pairs} seed {seed}: segment {first:?}-{last:?} only \
                         {dist} apart, expected at least {PAIR_DISTANCE_FLOOR}"
                    );
                }
            }
        }
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

    #[test]
    fn blocked_cells_are_never_entered() {
        // 3x1 board, one pair spanning the two end cells with the middle
        // cell blocked: no solution should be found, since the only route
        // between (0,0) and (2,0) runs through the blocked (1,0).
        let endpoints = [((0, 0), (2, 0))];
        let mut solver = Solver::new(3, 1, &endpoints, &[(1, 0)]);
        let mut budget = SOLVE_BUDGET;
        assert_eq!(
            solver.count_solutions(2, &mut budget),
            SolveOutcome::Count(0)
        );
    }
}
