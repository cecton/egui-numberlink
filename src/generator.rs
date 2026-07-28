//! Puzzle generation: a full-board tiling of non-crossing paths, cut out of
//! one Hamiltonian walk — used as an internal construction technique only
//! (see below for why full coverage itself is not the difficulty mechanism).
//!
//! Strategy:
//! 1. Build one random Hamiltonian path over the whole `width x height` grid
//!    via randomized backtracking guided by Warnsdorff's rule (a rectangular
//!    grid graph always has one; small boards make this fast — see
//!    [`random_hamiltonian_path`]).
//! 2. Cut it into `pair_count` contiguous segments, one per number. Full
//!    board coverage falls out of this by construction (every cell ends up
//!    in exactly one segment), but that's an internal construction detail,
//!    not something the player is required to reproduce — see
//!    [`crate::game::GameStatus::Won`]. A Warnsdorff-guided path is *locally
//!    coherent* — it tends to fill one small neighborhood before moving to
//!    the next — so a single random cut often lands on a segment whose own
//!    two endpoints fold back close to each other despite the segment having
//!    plenty of cells; a fixed accept/reject threshold against one random
//!    cut was tried and measured to barely help (~5%), since it only ever
//!    evaluated one candidate. Instead, [`cut_into_segments`] actively
//!    *searches*: it tries [`CUT_ATTEMPTS`] independent random cut-point
//!    configurations and keeps whichever [`score_cuts`] ranks best — both
//!    maximizing the worst segment's own endpoint spread *and* requiring
//!    genuine contention: [`has_forced_contention`] (see [`CONTENTION_BUDGET`]'s
//!    doc for what that checks and why it replaced an earlier, wrong
//!    difficulty signal) confirms *some* conflict exists at all, and
//!    [`pairwise_conflicts`] (see [`PAIRWISE_CONTENTION_BUDGET`]'s doc)
//!    additionally requires that conflict to cover *every* number, not just
//!    one isolated pair while the rest of the board stays trivially free.
//!    [`score_cuts`] also vetoes any segment whose two endpoints are *both*
//!    on the board's outer border (see [`is_periphery`]) — such a pair can
//!    always be joined by just walking the border, no interior reasoning
//!    required, regardless of how contended the rest of the board is; one
//!    endpoint on the border is fine, only both is rejected. This constraint
//!    measurably shrinks the pool of acceptable cuts on small boards: it
//!    raised `generate`'s average latency roughly 4-12x (5x5/4: ~12ms→~105ms;
//!    7x7/7: ~16ms→~194ms; 9x9/9: ~15ms→~63ms, all still 100% contended, see
//!    `measure_generate_latency_and_contention`), since a periphery-heavy
//!    Hamiltonian path can fail every one of [`CUT_ATTEMPTS`]'s candidates
//!    and force a full outer retry with a fresh path — raising [`CUT_ATTEMPTS`]
//!    itself doesn't help (measured at 3000 and 8000, no consistent
//!    improvement), confirming the bottleneck is which *paths* admit a valid
//!    cut at all, not how many cuts are tried per path. Still comfortably
//!    bounded (worst case measured under 600ms, on an unoptimized dev
//!    build) for a one-time "new game" action, so shipped as an unconditional
//!    veto rather than a softer/partial one.
//!
//! Global uniqueness of the full-board tiling is not checked or chased by
//! this generator at all — see [`CONTENTION_BUDGET`]'s doc for why that
//! turned out to be the wrong thing to optimize for once the win condition
//! stopped requiring full coverage. Genuine, widespread contention — whether
//! the pairs' own independent shortest routes can be assigned simultaneously
//! without crossing, and whether that forced conflict actually involves
//! every number — is the real difficulty gate now, and both checks are cheap
//! enough to run on every one of [`CUT_ATTEMPTS`] candidates directly.
//!
//! This generator never produces blocked cells — the win condition only
//! requires connecting every pair, not filling the whole board (see
//! [`crate::game::GameStatus::Won`]), so full coverage here is purely an
//! internal construction technique. Permanently blocked cells (walls) remain
//! supported for hand-authored/curated puzzles via
//! [`crate::game::NumberlinkGame::from_endpoints_with_blocked`], just not
//! produced by this generator.
//!
//! Endpoint spread alone (maximizing reach fraction) does *not* guarantee
//! genuine contention between pairs — two earlier, cheaper proxies for it
//! were tried and abandoned before landing on [`has_forced_contention`]:
//! a bounding-box-overlap check (satisfied 95-99% of the time even on
//! *unsearched* random cuts — vetoed almost nothing) and a cell-proximity
//! variant (99-100%). Both were geometric approximations of "could the
//! pairs' plausible routes conflict" checked on the *finished* segmentation,
//! and neither could discriminate — within this single-Hamiltonian-path
//! architecture, most segments end up spatially close to several others
//! regardless of cut choice, so proximity alone says nothing useful.
//! [`has_forced_contention`] instead answers the question directly, via a
//! real (heavily restricted, so still cheap) solver search rather than a
//! geometric approximation. It turned out necessary but not sufficient on
//! its own, though: a puzzle can satisfy "some conflict exists" with just one
//! small, isolated reroute while every other pair stays trivially solvable
//! by inspection — still easy overall despite technically passing. Requiring
//! that conflict to cover every number ([`pairwise_conflicts`]) is the
//! additional bar that actually makes the whole board demand attention, not
//! just one corner of it. Even this remains a *coverage* measure, not a
//! *severity* one — a number can be "covered" by a conflict costing it one
//! trivial extra step just as easily as one requiring a long detour, an axis
//! this doesn't attempt to measure yet.

type Cell = (usize, usize);
/// A cut-point configuration's `(min_reach_fraction, mean_reach_fraction)`,
/// as computed by [`score_cuts`].
type CutScore = (f64, f64);

const HAMILTONIAN_BUDGET: u64 = 400_000;
/// Node budget for the (strict, full-board-tiling) uniqueness check, used
/// only by a handful of hand-built tests now — no longer used by `generate`
/// itself, since that check turned out to have ~zero relationship to real
/// puzzle difficulty (see [`CONTENTION_BUDGET`]'s doc for the full story).
#[cfg(test)]
const SOLVE_BUDGET: u64 = 50_000;
/// Safety valve on the outer generate-and-check retry loop (covers the rare
/// case `random_hamiltonian_path` itself comes back empty). Reasonable
/// puzzle sizes succeed on the first or second attempt in practice.
const MAX_ATTEMPTS: u32 = 500;
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
/// Node budget for [`has_forced_contention`]'s per-candidate check inside
/// [`score_cuts`]. This replaced an earlier difficulty signal — full-board
/// -tiling uniqueness (still what [`Solver::new`]'s default checks) — which
/// turned out to have ~zero relationship to real puzzle difficulty: the
/// game's win condition doesn't require full coverage, so a puzzle can be a
/// *unique* full tiling while still being trivially solvable by just drawing
/// each pair's own shortest path, ignoring the rest of the board entirely.
/// Verifying that full-tiling uniqueness was also too expensive to check
/// live at 7x7/9x9, which is why an offline-harvested puzzle bank existed
/// for those sizes at one point — once the check itself turned out to be
/// wrong *and* [`has_forced_contention`] turned out both cheap and effective
/// at every board size including 9x9, live generation alone made that bank
/// unnecessary and it was deleted rather than re-harvested under the
/// corrected criterion.
///
/// [`has_forced_contention`] checks the question that actually matters — can
/// the pairs' own independent shortest routes be assigned simultaneously
/// without crossing? — and, measured across 224 random cut candidates per
/// preset, turned out both common (19.6%/52.7%/66.5% of candidates on
/// 5x5/4, 7x7/6, 9x9/8 respectively) and cheap (mean cost 16.7/57.8/1115.3
/// nodes, max 113/2535/90337) — cheap enough to check on every one of
/// [`CUT_ATTEMPTS`] candidates directly inside `score_cuts` (see
/// `survey_contention_hit_rate_and_cost` in the `difficulty_survey` test
/// module). This budget is a generous margin above that measured max.
const CONTENTION_BUDGET: u64 = 500_000;
/// Per-pairwise-check node budget for [`pairwise_conflicts`] inside
/// [`score_cuts`]. `has_forced_contention` alone only requires *some*
/// conflict to exist anywhere on the board — satisfied just as well by one
/// small, isolated conflict between two numbers as by a puzzle where every
/// number is entangled with someone, and only the latter forces a player to
/// reason about more than a corner of the board. Measured (see
/// `survey_pairwise_conflict_coverage` in `difficulty_survey`) across 224
/// already-contended candidates per preset: mean coverage was already
/// 3.05/4, 4.30/6, 5.46/8 numbers on 5x5/4, 7x7/6, 9x9/8 respectively, with
/// roughly half of candidates (111/224, 117/224, 120/224) already reaching
/// *full* coverage — common enough that requiring full coverage
/// (`pairwise_conflicts(..) == pair_count`) is comfortably findable within
/// [`CUT_ATTEMPTS`]. Each individual pairwise sub-check involves only 2
/// numbers, a much smaller search than `has_forced_contention`'s full
/// `pair_count`-way problem, so this budget is set well below
/// `CONTENTION_BUDGET` rather than reusing it directly.
///
/// Coverage isn't the same as difficulty, though: a number can be "covered"
/// by a conflict that only costs it one trivial extra step, just as easily
/// as one requiring a long detour. This check widens *how many* numbers are
/// forced to notice a conflict, not *how much* reasoning each conflict
/// demands — a real, separate axis this doesn't attempt to measure.
const PAIRWISE_CONTENTION_BUDGET: u64 = 50_000;

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

/// Whether `cell` sits on the outer border of a `width x height` board (row
/// or column 0, or the last row/column). See [`score_cuts`]'s doc for why
/// this matters: a pair with *both* endpoints on the border can always be
/// joined by just walking the border itself, with no need to reason about
/// the board's interior at all.
fn is_periphery(width: usize, height: usize, cell: Cell) -> bool {
    let (x, y) = cell;
    x == 0 || y == 0 || x == width - 1 || y == height - 1
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
/// Manhattan distance falls under [`PAIR_DISTANCE_FLOOR`], or whose two
/// endpoints are *both* on the board's outer border (see [`is_periphery`] —
/// such a pair can always be joined by just walking the border itself, no
/// interior reasoning needed at all, regardless of how contended the rest of
/// the board is; one endpoint on the border is fine, only both is vetoed),
/// or whose resulting endpoints don't clear [`has_forced_contention`] (see
/// [`CONTENTION_BUDGET`]'s doc for why that check, not full-board-tiling
/// uniqueness, is the real difficulty gate) *and* [`pairwise_conflicts`]
/// covering every number (see [`PAIRWISE_CONTENTION_BUDGET`]'s doc for why a
/// plain yes/no on contention wasn't enough on its own), forces this
/// candidate's score to `(0.0, 0.0)`, vetoing it. Path *shape* itself is
/// never shown to the
/// player before solving — only the two endpoints are — so a high reach
/// fraction (even a dead-straight segment) is purely a good thing here,
/// unlike in generators that route each pair independently and must also
/// worry about the drawn path looking obvious.
fn score_cuts(width: usize, height: usize, segments: &[Vec<Cell>]) -> CutScore {
    let fractions: Vec<f64> = segments
        .iter()
        .map(|s| {
            let first = s[0];
            let last = *s.last().expect("segment has >= 2 cells");
            let dist = manhattan(first, last);
            if dist < PAIR_DISTANCE_FLOOR {
                return 0.0;
            }
            if is_periphery(width, height, first) && is_periphery(width, height, last) {
                return 0.0;
            }
            dist as f64 / (s.len() - 1) as f64
        })
        .collect();
    if fractions.contains(&0.0) {
        return (0.0, 0.0);
    }
    let endpoints = to_endpoints(segments);
    let mut budget = CONTENTION_BUDGET;
    if !has_forced_contention(width, height, &endpoints, &mut budget) {
        return (0.0, 0.0);
    }
    let covered = pairwise_conflicts(width, height, &endpoints, PAIRWISE_CONTENTION_BUDGET);
    if covered < endpoints.len() {
        return (0.0, 0.0);
    }
    let min = fractions.iter().copied().fold(f64::INFINITY, f64::min);
    let mean = fractions.iter().sum::<f64>() / fractions.len() as f64;
    (min, mean)
}

/// A segmentation's first and last cell, per segment — number `i`'s two
/// endpoints at `result[i]`.
fn to_endpoints(segments: &[Vec<Cell>]) -> Vec<(Cell, Cell)> {
    segments
        .iter()
        .map(|s| (s[0], *s.last().expect("segment has >= 2 cells")))
        .collect()
}

/// Cut a full-board Hamiltonian `path` into `pair_count` contiguous,
/// non-empty segments (every cell ends up in exactly one), keeping whichever
/// of [`CUT_ATTEMPTS`] independent random cut-point configurations (see
/// [`random_cut_points`]) [`score_cuts`] ranks best — including its
/// [`has_forced_contention`] veto — rather than accepting the first
/// structurally-valid one. See the module doc for why searching many
/// candidates (instead of one accept/reject shot) is what actually fixes
/// segments folding back on themselves.
fn cut_into_segments(
    width: usize,
    height: usize,
    path: &[Cell],
    pair_count: usize,
    rng: &mut fastrand::Rng,
) -> Vec<Vec<Cell>> {
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

        let score = score_cuts(width, height, &segments);
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
    /// Whether every non-blocked cell must be claimed by some path for a
    /// completion to count as a solution. `true` (the default, via
    /// [`Solver::new`]) matches this generator's own internal full-board
    /// tiling construction/verification technique; `false` (see
    /// [`Solver::without_full_coverage_requirement`]) matches the actual
    /// game rule ([`crate::game::GameStatus::Won`]): connecting every pair
    /// is already a complete solution, whether or not every cell ends up
    /// claimed.
    require_full_coverage: bool,
    /// When `true` (see [`Solver::shortest_paths_only`]), a move is only
    /// legal if it's strictly closer (Manhattan distance) to the moving
    /// number's own target than its current head — i.e. every path is
    /// forced to be one of that pair's own shortest routes, no detours.
    /// Used to check for genuine *contention*: whether the pairs' own
    /// independent shortest routes can be assigned simultaneously without
    /// any of them crossing, which if true means the puzzle is solvable
    /// without the player ever needing to notice or reroute around another
    /// pair at all.
    shortest_paths_only: bool,
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
            require_full_coverage: true,
            shortest_paths_only: false,
        }
    }

    /// Relax completion to match the real game rule (see
    /// [`crate::game::GameStatus::Won`]): every pair connected is already a
    /// solution, whether or not every cell ends up claimed. `Solver::new`'s
    /// default instead matches this generator's own internal full-board
    /// tiling technique — see the module docs above for why that technique
    /// was chosen for construction, and [`CONTENTION_BUDGET`]'s doc for why
    /// it turned out to be the wrong thing to check for real puzzle
    /// difficulty. Only used by tests now — [`Self::shortest_paths_only`]
    /// sets the same flag directly for `has_forced_contention`'s own use.
    #[cfg(test)]
    fn without_full_coverage_requirement(mut self) -> Self {
        self.require_full_coverage = false;
        self
    }

    /// Restrict every path to its own shortest (Manhattan-monotone) routes
    /// only, and imply [`Self::without_full_coverage_requirement`] (a
    /// shortest path essentially never uses every cell). Used to check for
    /// genuine contention — see [`Self::shortest_paths_only`]'s field doc.
    /// Dramatically cheaper to search than either full-coverage or general
    /// coverage-optional solving, since detours (the source of both
    /// architectures' expense) are impossible by construction here.
    fn shortest_paths_only(mut self) -> Self {
        self.shortest_paths_only = true;
        self.require_full_coverage = false;
        self
    }

    /// A cell is enterable if it's not a wall, not already owned, not a
    /// *different*, still-pending number's target (stepping onto that would
    /// make the other number unfinishable; only ever legal for its own path
    /// to reach its own target), and — under
    /// [`Self::shortest_paths_only`] — strictly closer to `number`'s own
    /// target than its current head.
    fn enterable(&self, number: usize, cell: Cell) -> bool {
        let idx = to_idx(cell, self.width);
        if self.blocked[idx] || self.owner[idx].is_some() {
            return false;
        }
        if self.shortest_paths_only
            && manhattan(cell, self.targets[number])
                >= manhattan(self.heads[number], self.targets[number])
        {
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
    ///
    /// Skipped entirely under [`Self::shortest_paths_only`]: the
    /// monotone-distance move restriction already keeps that search small
    /// on its own, and this prune's assumptions (every free cell eventually
    /// needs to be reachable) don't hold once detours aren't possible in
    /// the first place.
    fn is_feasible(&self) -> bool {
        if self.shortest_paths_only {
            return true;
        }
        for y in 0..self.height {
            for x in 0..self.width {
                let cell = (x, y);
                let idx = to_idx(cell, self.width);
                if self.blocked[idx] || self.owner[idx].is_some() {
                    continue;
                }
                let is_pending_target = self
                    .targets
                    .iter()
                    .zip(&self.done)
                    .any(|(&t, &done)| !done && t == cell);
                if !self.require_full_coverage && !is_pending_target {
                    // Coverage isn't required: a free cell nobody still
                    // needs to reach is fine to leave stranded forever.
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
                let required = if is_pending_target { 1 } else { 2 };
                if usable < required {
                    return false;
                }
            }
        }
        true
    }

    /// Count non-crossing completions of every path, stopping early once
    /// `limit` is reached — full-coverage-of-non-blocked-cells is also
    /// required by default, unless relaxed via
    /// [`Self::without_full_coverage_requirement`]. Returns `BudgetExceeded`
    /// if the node budget runs out first (the caller treats that the same
    /// as "not verifiably unique" and regenerates).
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

        // Short-circuits to `false` (no `.all()` call) whenever coverage is
        // required, so `Solver::new`'s default behavior is untouched by this
        // check — only the relaxed mode (see
        // `without_full_coverage_requirement`) ever evaluates it. Reaching a
        // solution is never a budget failure, so this always returns `true`
        // here regardless of whether `count` just hit `limit` — the caller's
        // own `*count >= limit` check (below) is what stops further search,
        // kept as a separate concern from budget exhaustion.
        if !self.require_full_coverage && self.done.iter().all(|&d| d) {
            *count += 1;
            return true;
        }
        if self.filled == self.usable_cells {
            if self.done.iter().all(|&d| d) {
                *count += 1;
            }
            return true;
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

/// Whether `endpoints` genuinely require rerouting to solve: `true` iff
/// there is *no* way to simultaneously assign each pair its own shortest
/// (Manhattan-monotone) route without any of them crossing (see
/// [`Solver::shortest_paths_only`]). If such a trivial assignment exists
/// instead, the puzzle is solvable without ever needing to notice or route
/// around another pair — no genuine contention, regardless of how spread
/// out or full-board-tiling-unique it is. `budget` bounds the (typically
/// cheap, since detours are impossible in this mode) search; a budget
/// timeout is treated as "not confirmed genuine" (same conservative
/// direction as every other budget-bounded check in this module).
fn has_forced_contention(
    width: usize,
    height: usize,
    endpoints: &[(Cell, Cell)],
    budget: &mut u64,
) -> bool {
    let mut solver = Solver::new(width, height, endpoints, &[]).shortest_paths_only();
    matches!(solver.count_solutions(1, budget), SolveOutcome::Count(0))
}

/// How many distinct numbers participate in at least one unavoidable
/// pairwise conflict with another number — a severity/coverage signal on
/// top of [`has_forced_contention`]'s plain yes/no. [`has_forced_contention`]
/// only requires *some* simultaneous shortest-path assignment across *all*
/// pairs to fail — satisfied just as well by one small, isolated conflict
/// between two numbers as by a puzzle where every number is entangled with
/// someone, and only the latter forces a player to actually reason about
/// more than a corner of the board.
///
/// For every distinct pair of numbers `(i, j)` (`C(pair_count, 2)`
/// combinations), checks whether *just those two* are forced to conflict —
/// the same [`Solver::shortest_paths_only`] mechanism, restricted to those
/// two numbers, with every other number's own two endpoints passed as
/// blocked cells (approximating "already spoken for by someone else"
/// without needing to solve the full joint problem — a cheap, tractable
/// *necessary* signal, not a sufficient one: it can miss conflicts that only
/// emerge with three or more numbers simultaneously present, same honesty as
/// `has_forced_contention` itself). Returns how many distinct numbers show
/// up in at least one such conflicting pair: `0` means no conflicts at all,
/// `endpoints.len()` means every number is entangled with someone.
/// `budget_per_check` bounds each pairwise check independently (not shared
/// across checks, so one expensive early pair can't starve the rest).
fn pairwise_conflicts(
    width: usize,
    height: usize,
    endpoints: &[(Cell, Cell)],
    budget_per_check: u64,
) -> usize {
    let mut conflicted = vec![false; endpoints.len()];
    for i in 0..endpoints.len() {
        for j in (i + 1)..endpoints.len() {
            if conflicted[i] && conflicted[j] {
                continue; // both already known entangled; no need to re-check
            }
            let others_blocked: Vec<Cell> = endpoints
                .iter()
                .enumerate()
                .filter(|&(n, _)| n != i && n != j)
                .flat_map(|(_, &(a, b))| [a, b])
                .collect();
            let pair = [endpoints[i], endpoints[j]];
            let mut budget = budget_per_check;
            let mut solver =
                Solver::new(width, height, &pair, &others_blocked).shortest_paths_only();
            if matches!(
                solver.count_solutions(1, &mut budget),
                SolveOutcome::Count(0)
            ) {
                conflicted[i] = true;
                conflicted[j] = true;
            }
        }
    }
    conflicted.iter().filter(|&&c| c).count()
}

/// Generate `pair_count` numbered endpoint pairs on a `width x height` grid
/// (endpoints returned in generation order, number `i`'s endpoints at
/// `result[i]`). Prefers a layout confirmed to require genuine rerouting —
/// [`has_forced_contention`], checked as part of [`score_cuts`] on every
/// candidate — and falls back to whichever candidate scored best by the
/// cheap reach-fraction heuristic across every attempt made otherwise, if
/// none clears that bar within [`MAX_ATTEMPTS`] fresh Hamiltonian paths (see
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
    for _ in 0..MAX_ATTEMPTS {
        let Some(path) = random_hamiltonian_path(width, height, &mut rng) else {
            continue;
        };
        let segments = cut_into_segments(width, height, &path, pair_count, &mut rng);
        let score = score_cuts(width, height, &segments);
        let is_better = match &fallback {
            Some((_, best)) => score > *best,
            None => true,
        };
        if is_better {
            fallback = Some((to_endpoints(&segments), score));
        }
        // A degenerate score (some segment fell under `PAIR_DISTANCE_FLOOR`,
        // or the layout has no genuine contention — see `score_cuts`) means
        // this Hamiltonian path's `CUT_ATTEMPTS` candidates didn't clear the
        // bar: retry with a fresh path rather than ever accepting it.
        if score.0 == 0.0 {
            continue;
        }
        return to_endpoints(&segments);
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
    fn is_periphery_only_true_on_the_outer_border() {
        // 5x5 board: corners, edges, and interior.
        assert!(is_periphery(5, 5, (0, 0)));
        assert!(is_periphery(5, 5, (4, 4)));
        assert!(is_periphery(5, 5, (0, 2)));
        assert!(is_periphery(5, 5, (2, 0)));
        assert!(is_periphery(5, 5, (4, 2)));
        assert!(is_periphery(5, 5, (2, 4)));
        assert!(!is_periphery(5, 5, (1, 1)));
        assert!(!is_periphery(5, 5, (2, 2)));
        assert!(!is_periphery(5, 5, (3, 3)));
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
    fn generate_succeeds_at_the_web_demos_difficulty_presets() {
        // Regression test: an earlier version of `random_hamiltonian_path`
        // could backtrack all the way past its starting cell (a dead end
        // with nowhere left to retreat to) and panic on an empty `frames`
        // stack instead of returning `None` for the outer loop to retry.
        // 5x5/4 first triggered it; a plain (non-Warnsdorff) neighbor order
        // also made 9x9/8 fail to find any Hamiltonian path at all within
        // budget. The fixture below reads (5, 5, 4)/(7, 7, 7)/(9, 9, 9),
        // matching `examples/webapp.rs`'s current Beginner/Intermediate/
        // Expert presets (Intermediate/Expert were bumped denser — pairs =
        // side length, matching Flow Free's convention — while Beginner
        // stayed at 5x5/4 since it measured too tight to bump, see
        // `measure_generate_latency_and_contention`). Kept to a handful of
        // seeds since this crate's dev profile has no opt-level override,
        // and 9x9 is meaningfully slower unoptimized.
        for (width, height, pairs) in [(5, 5, 4), (7, 7, 7), (9, 9, 9)] {
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
        for (width, height, pairs) in [(5, 5, 4), (7, 7, 7), (9, 9, 9)] {
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
    fn generated_pairs_never_have_both_endpoints_on_the_periphery() {
        // A pair with both endpoints on the outer border can always be
        // joined by just walking the border, with no interior reasoning
        // needed — trivially easy regardless of how contended the rest of
        // the board is. One endpoint on the border is fine; only both is
        // vetoed (see `score_cuts`'s `is_periphery` check).
        for (width, height, pairs) in [(5, 5, 4), (7, 7, 7), (9, 9, 9)] {
            for seed in 0..10 {
                let endpoints = generate(width, height, pairs, seed);
                for &(a, b) in &endpoints {
                    assert!(
                        !(is_periphery(width, height, a) && is_periphery(width, height, b)),
                        "pair {a:?}-{b:?} has both endpoints on the periphery of a \
                         {width}x{height}/{pairs} board (seed {seed})"
                    );
                }
            }
        }
    }

    #[test]
    fn generate_is_verifiably_unique() {
        let endpoints = generate(4, 4, 3, 0);
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
        for (width, height, pairs) in [(5, 5, 4), (7, 7, 7), (9, 9, 9)] {
            let mut path = None;
            for _ in 0..MAX_ATTEMPTS {
                if let Some(p) = random_hamiltonian_path(width, height, &mut rng) {
                    path = Some(p);
                    break;
                }
            }
            let path = path.expect("small boards find a Hamiltonian path within a few attempts");
            let segments = cut_into_segments(width, height, &path, pairs, &mut rng);
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
        for (width, height, pairs) in [(5, 5, 4), (7, 7, 7), (9, 9, 9)] {
            for seed in 0..20 {
                let mut rng = fastrand::Rng::with_seed(seed);
                let mut segments = None;
                for _ in 0..MAX_ATTEMPTS {
                    let Some(path) = random_hamiltonian_path(width, height, &mut rng) else {
                        continue;
                    };
                    let candidate = cut_into_segments(width, height, &path, pairs, &mut rng);
                    if score_cuts(width, height, &candidate).0 > 0.0 {
                        segments = Some(candidate);
                        break;
                    }
                }
                let segments = segments
                    .expect("a non-degenerate cut should be found within MAX_ATTEMPTS fresh paths");
                let (min_fraction, _mean) = score_cuts(width, height, &segments);
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

    #[test]
    fn without_full_coverage_requirement_finds_many_solutions_when_space_allows() {
        // 3x3 board, one pair at diagonal corners, plenty of free space and
        // nothing else to route around: at least 2 non-crossing routes
        // connect them without needing every cell, so this must NOT get
        // stuck at `Count(1)` — the whole reason full-board-tiling
        // uniqueness is the wrong check now that coverage isn't required to
        // win (see the module doc and `CONTENTION_BUDGET`'s doc for the full
        // story).
        let endpoints = [((0, 0), (2, 2))];
        let mut solver = Solver::new(3, 3, &endpoints, &[]).without_full_coverage_requirement();
        let mut budget = SOLVE_BUDGET;
        assert_eq!(
            solver.count_solutions(2, &mut budget),
            SolveOutcome::Count(2)
        );
    }

    #[test]
    fn without_full_coverage_requirement_still_finds_a_genuinely_forced_route() {
        // 1x3 vertical corridor: only one possible route between the two
        // ends regardless of whether coverage is required, since there's no
        // room to branch at all. Confirms the relaxed mode isn't just
        // "always many" — it still reports a real unique route when the
        // board geometry actually forces one.
        let endpoints = [((0, 0), (0, 2))];
        let mut solver = Solver::new(1, 3, &endpoints, &[]).without_full_coverage_requirement();
        let mut budget = SOLVE_BUDGET;
        assert_eq!(
            solver.count_solutions(2, &mut budget),
            SolveOutcome::Count(1)
        );
    }

    #[test]
    fn pairwise_conflicts_is_zero_when_nothing_is_forced() {
        // Two diagonal pairs, each with multiple shortest-route options
        // (via either corner), far enough apart to never interact.
        let endpoints = [((0, 0), (1, 1)), ((3, 3), (4, 4))];
        assert_eq!(pairwise_conflicts(5, 5, &endpoints, SOLVE_BUDGET), 0);
    }

    #[test]
    fn pairwise_conflicts_isolates_just_the_entangled_numbers() {
        // pair_a (0,1)-(2,1) and pair_b (1,0)-(1,2) each have exactly one
        // possible shortest route (any off-axis move strictly increases
        // Manhattan distance to their own target), and both routes are
        // forced through the shared center cell (1,1) — guaranteed mutual
        // conflict. pair_c sits far away with an untouched, free diagonal
        // route and should stay uninvolved.
        let pair_a = ((0, 1), (2, 1));
        let pair_b = ((1, 0), (1, 2));
        let pair_c = ((3, 3), (4, 4));
        let endpoints = [pair_a, pair_b, pair_c];
        assert_eq!(
            pairwise_conflicts(5, 5, &endpoints, SOLVE_BUDGET),
            2,
            "only pair_a/pair_b should be entangled, pair_c stays free"
        );
    }

    #[test]
    fn pairwise_conflicts_counts_every_entangled_number_across_multiple_clusters() {
        // Two independent copies of the same forced-through-the-center-cell
        // trick, side by side with a gap between them: every number ends up
        // entangled with *someone*, just split across two separate clusters
        // rather than one puzzle-wide conflict.
        let endpoints = [
            ((0, 1), (2, 1)), // cluster 1, forced through (1, 1)
            ((1, 0), (1, 2)),
            ((4, 1), (6, 1)), // cluster 2, forced through (5, 1)
            ((5, 0), (5, 2)),
        ];
        assert_eq!(pairwise_conflicts(7, 3, &endpoints, SOLVE_BUDGET), 4);
    }

    /// The actual regression test for "still feels trivial despite being
    /// spread out": every generated puzzle must genuinely require rerouting
    /// to solve, not just happen to have well-spread endpoints, *and* that
    /// requirement must cover every number, not just one isolated pair while
    /// the rest stay trivially free. Checks [`has_forced_contention`] and
    /// [`pairwise_conflicts`] directly on `generate`'s own output across
    /// presets/seeds, the same checks baked into `score_cuts` at generation
    /// time — so this mostly guards against a future regression loosening or
    /// bypassing those vetoes, rather than testing new logic.
    #[test]
    fn generate_produces_genuinely_contended_puzzles() {
        for (width, height, pairs) in [(5, 5, 4), (7, 7, 7), (9, 9, 9)] {
            for seed in 0..10 {
                let endpoints = generate(width, height, pairs, seed);
                let mut budget = 2_000_000u64;
                assert!(
                    has_forced_contention(width, height, &endpoints, &mut budget),
                    "{width}x{height}/{pairs} seed {seed}: {endpoints:?} has no forced \
                     contention — a naive independent-shortest-path assignment solves it"
                );
                let covered = pairwise_conflicts(width, height, &endpoints, 2_000_000);
                assert_eq!(
                    covered,
                    pairs,
                    "{width}x{height}/{pairs} seed {seed}: {endpoints:?} only has {covered}/{pairs} \
                     numbers entangled in a forced conflict, not all of them"
                );
            }
        }
    }
}

/// Offline, opt-in difficulty-measurement harness — every test here is
/// `#[ignore]`d, so `cargo test --lib` (and CI) never runs it. Unlike the
/// throwaway `debug_*` instrumentation used elsewhere this session (added,
/// measured, then deleted), this one is meant to stay: a reusable tool for
/// spending real compute measuring puzzle difficulty at a much larger sample
/// size than an interactive session affords. It's how [`has_forced_contention`]
/// was validated (common and cheap enough, even at 9x9, to bake directly
/// into [`score_cuts`]'s hot-path veto — see [`CONTENTION_BUDGET`]'s doc) —
/// note the earlier full-board-tiling solver-cost signal it replaced never
/// got that lucky: it stayed too expensive to check broadly at every board
/// size no matter how this harness was used, which is *why* it was replaced.
///
/// Run explicitly:
///
/// ```sh
/// cargo test --release --lib -- --ignored --nocapture difficulty_survey
/// ```
///
/// Sample counts default small so a sanity-check run finishes in a couple of
/// minutes. Override via env vars for a much longer, more statistically
/// powerful run on a many-core machine — e.g. leaving this going for hours or
/// days:
///
/// ```sh
/// SURVEY_SAMPLES=200000 SURVEY_TOPK_SAMPLES=5000 \
///     cargo test --release --lib -- --ignored --nocapture difficulty_survey
/// ```
///
/// Work is split across [`std::thread::available_parallelism`] threads, so
/// more cores directly means more samples per unit time.
#[cfg(test)]
mod difficulty_survey {
    use super::*;

    /// Mirrors `examples/webapp.rs`'s live presets. Note: `CONTENTION_BUDGET`
    /// and `PAIRWISE_CONTENTION_BUDGET`'s doc comments cite specific
    /// percentages/means measured against this constant's *previous* value
    /// (5,5,4)/(7,7,6)/(9,9,8) — those numbers are a historical record of
    /// that measurement, not a live reflection of whatever this constant
    /// currently holds; re-run the surveys below after any preset change to
    /// get fresh numbers before updating those doc comments.
    const PRESETS: [(usize, usize, usize); 3] = [(5, 5, 4), (7, 7, 7), (9, 9, 9)];
    /// Real difficulty ground truth is expensive by nature — give it a much
    /// larger allowance than the live [`SOLVE_BUDGET`] so a genuinely hard
    /// candidate isn't clipped at the same low ceiling as an easy one.
    const SURVEY_SOLVE_BUDGET: u64 = 2_000_000;

    fn env_count(var: &str, default: usize) -> usize {
        std::env::var(var)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(default)
    }

    fn sample_count() -> usize {
        env_count("SURVEY_SAMPLES", 200)
    }

    /// How expensive [`has_forced_contention`] is, and how often it's
    /// actually satisfied, across random cut candidates for each preset —
    /// answers whether it's cheap enough to bake into the live generator's
    /// hot path (checked on every `CUT_ATTEMPTS` candidate, like
    /// `PAIR_DISTANCE_FLOOR`) versus needing to stay a smaller-shortlist
    /// post-filter, and whether contention is even achievable often enough
    /// to select for at all.
    ///
    /// Deliberately checks the [`PAIR_DISTANCE_FLOOR`] veto directly here
    /// (`min_reach_fraction` alone) rather than calling the full
    /// [`score_cuts`] — `score_cuts` now bakes the very
    /// [`has_forced_contention`] check this survey measures into its own
    /// veto, so skipping degenerate candidates via `score_cuts` itself would
    /// make this circular (every survivor would already be pre-confirmed
    /// contended, trivially reporting ~100% regardless of the true rate).
    #[test]
    #[ignore]
    fn survey_contention_hit_rate_and_cost() {
        let samples = sample_count();
        for &(width, height, pairs) in &PRESETS {
            let data: Vec<(bool, u64)> = parallel_samples(samples, |count, rng| {
                let mut out = Vec::with_capacity(count);
                while out.len() < count {
                    let Some(path) = random_hamiltonian_path(width, height, rng) else {
                        continue;
                    };
                    for _ in 0..10.min(count - out.len()) {
                        let cuts = random_cut_points(path.len(), pairs, rng);
                        let mut segments = Vec::with_capacity(pairs);
                        let mut start = 0;
                        for &cut in &cuts {
                            segments.push(path[start..cut].to_vec());
                            start = cut;
                        }
                        segments.push(path[start..].to_vec());
                        let too_close = segments
                            .iter()
                            .any(|s| manhattan(s[0], *s.last().unwrap()) < PAIR_DISTANCE_FLOOR);
                        if too_close {
                            continue;
                        }
                        let endpoints = to_endpoints(&segments);
                        let mut budget = SURVEY_SOLVE_BUDGET;
                        let contends =
                            has_forced_contention(width, height, &endpoints, &mut budget);
                        let cost = SURVEY_SOLVE_BUDGET - budget;
                        out.push((contends, cost));
                    }
                }
                out
            });
            let hits = data.iter().filter(|&&(c, _)| c).count();
            let costs: Vec<u64> = data.iter().map(|&(_, c)| c).collect();
            let mean_cost = costs.iter().sum::<u64>() as f64 / costs.len() as f64;
            let max_cost = costs.iter().copied().max().unwrap_or(0);
            println!(
                "{width}x{height}/{pairs}: n={} genuine_contention={hits}/{} ({:.1}%) \
                 mean_cost={mean_cost:.1} max_cost={max_cost}",
                data.len(),
                data.len(),
                100.0 * hits as f64 / data.len() as f64
            );
        }
    }

    /// Among candidates that already pass today's [`has_forced_contention`]
    /// veto, how many of the `pairs` numbers end up covered by
    /// [`pairwise_conflicts`] — i.e. is a typical "contended" puzzle mostly
    /// just two entangled numbers and the rest trivially free, or is
    /// contention already fairly widespread? Answers what a reasonable
    /// minimum-coverage threshold would be without making candidates too
    /// rare to find within `CUT_ATTEMPTS`.
    #[test]
    #[ignore]
    fn survey_pairwise_conflict_coverage() {
        let samples = sample_count();
        for &(width, height, pairs) in &PRESETS {
            let coverage: Vec<usize> = parallel_samples(samples, |count, rng| {
                let mut out = Vec::with_capacity(count);
                while out.len() < count {
                    let Some(path) = random_hamiltonian_path(width, height, rng) else {
                        continue;
                    };
                    for _ in 0..10.min(count - out.len()) {
                        let cuts = random_cut_points(path.len(), pairs, rng);
                        let mut segments = Vec::with_capacity(pairs);
                        let mut start = 0;
                        for &cut in &cuts {
                            segments.push(path[start..cut].to_vec());
                            start = cut;
                        }
                        segments.push(path[start..].to_vec());
                        let too_close = segments
                            .iter()
                            .any(|s| manhattan(s[0], *s.last().unwrap()) < PAIR_DISTANCE_FLOOR);
                        if too_close {
                            continue;
                        }
                        let endpoints = to_endpoints(&segments);
                        let mut budget = SURVEY_SOLVE_BUDGET;
                        if !has_forced_contention(width, height, &endpoints, &mut budget) {
                            continue; // only measuring today's already-contended candidates
                        }
                        let covered =
                            pairwise_conflicts(width, height, &endpoints, SURVEY_SOLVE_BUDGET);
                        out.push(covered);
                    }
                }
                out
            });
            let n = coverage.len() as f64;
            let mean = coverage.iter().sum::<usize>() as f64 / n;
            let max = coverage.iter().copied().max().unwrap_or(0);
            let min = coverage.iter().copied().min().unwrap_or(0);
            let fully_entangled = coverage.iter().filter(|&&c| c == pairs).count();
            println!(
                "{width}x{height}/{pairs}: n={} covered_numbers mean={mean:.2}/{pairs} \
                 min={min} max={max} fully_entangled={fully_entangled}/{}",
                coverage.len(),
                coverage.len()
            );
        }
    }

    /// Split `total` samples across available threads and run `work` (given
    /// a per-thread share of the sample count and its own RNG) on each,
    /// concatenating the results.
    fn parallel_samples<T: Send>(
        total: usize,
        work: impl Fn(usize, &mut fastrand::Rng) -> Vec<T> + Sync,
    ) -> Vec<T> {
        let threads = std::thread::available_parallelism()
            .map(std::num::NonZeroUsize::get)
            .unwrap_or(1)
            .min(total.max(1));
        let per_thread = total.div_ceil(threads);
        std::thread::scope(|scope| {
            let work = &work;
            let handles: Vec<_> = (0..threads)
                .map(|i| {
                    scope.spawn(move || {
                        let mut rng = fastrand::Rng::with_seed(0x9E37_79B9 ^ i as u64);
                        work(per_thread, &mut rng)
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|h| h.join().unwrap())
                .collect()
        })
    }

    /// Sanity-checks `generate`'s actual real-world latency and contention
    /// hit rate together — the numbers that justified retiring the curated
    /// puzzle bank entirely (see the module doc): live generation turned out
    /// fast (single-digit-to-low-tens of milliseconds) and 100% genuinely
    /// contended at every preset once `has_forced_contention` was wired into
    /// `score_cuts`, including 9x9-sized boards — the one size a full-board-
    /// tiling uniqueness check could never resolve at any practical budget.
    /// Intermediate/Expert below were bumped denser (pairs = side length,
    /// matching Flow Free's convention) after this test confirmed they stay
    /// fast and fully contended at the higher density (7x7/7: avg=16.4ms
    /// max=70.4ms 30/30 contended; 9x9/9: avg=14.8ms max=46.5ms 30/30
    /// contended). 5x5 stayed at its original N-1 density (4 pairs, not 5):
    /// this same test caught 5x5/5 measuring badly (avg=543.6ms,
    /// max=1.2s, only 27/30 contended) — a 25-cell board split 5 ways is too
    /// tight for `cut_into_segments` to reliably land a non-degenerate,
    /// fully-contended cut within [`CUT_ATTEMPTS`]/[`MAX_ATTEMPTS`].
    #[test]
    #[ignore]
    fn measure_generate_latency_and_contention() {
        for (width, height, pairs) in [(5, 5, 4), (7, 7, 7), (9, 9, 9)] {
            let n = 30u64;
            let mut total = std::time::Duration::ZERO;
            let mut max = std::time::Duration::ZERO;
            let mut contended = 0u32;
            for seed in 0..n {
                let start = std::time::Instant::now();
                let endpoints = generate(width, height, pairs, seed);
                let elapsed = start.elapsed();
                total += elapsed;
                max = max.max(elapsed);
                let mut budget = SURVEY_SOLVE_BUDGET;
                if has_forced_contention(width, height, &endpoints, &mut budget) {
                    contended += 1;
                }
            }
            println!(
                "{width}x{height}/{pairs}: avg={:?} max={:?} contended={contended}/{n}",
                total / n as u32,
                max
            );
        }
    }
}
