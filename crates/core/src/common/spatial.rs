//! Finding nearby agents: which contacts are within some distance of a point?
//!
//! The engine builds a `SpatialIndex` from post-motion positions each tick and
//! refreshes it before each interaction and before networks and metrics: if
//! any entity has moved (an interaction may move entities), it is rebuilt, so
//! every query sees current positions. Plugins only ask questions of it. The mission's `spatial_lookup` chooses how
//! they are answered: by testing every agent (scan, the default) or by testing
//! only agents in nearby cells of a grid. Both give the same answers in the same
//! order, because the grid only proposes candidates and each one gets the scan's
//! exact test.
//!
//! DEVNOTE: the grid exists because of a measurement. Profiling perf.py's
//! `collision` workload (aircraft spread over about a kilometre at one altitude,
//! 5 m collision range) put SimpleCollision's every-pair check at 1.1 ms per step
//! at 1,024 agents (58% of the step), 4.5 ms at 2,048 (71%), and 18 ms at 4,096
//! (79%). Positions are kept in a compact list instead of being read from each
//! `Entity`, whose size had changed collision timings by 20% with identical
//! output. Measured on an M2 Pro (median of five alternating runs): a step with
//! 4,096 spread-out agents took 19.1 ms with the previous every-pair loop (since
//! removed), 10.2 ms with the compact scan, and 5.0 ms with a 5 m grid.
//! Subtracting the same mission without SimpleCollision estimates collision's
//! own cost at about 15, 6, and 0.9 ms. At 256 agents the grid makes a step 9%
//! slower than the scan (its build, ~0.02 ms per step). A first grid ran 27 binary
//! searches per query (one per cell) and was slower than the scan at 1,024
//! agents (1.9 against 1.2 ms per step); looking up (x, y) columns in a hash
//! map, and reusing buffers, fixed that. Rerun the scan against grids with
//! reference/spatial_benchmark.py; quick check: perf.py's `collision` against
//! `collision-grid`.
//!
//! Only SimpleCollision (`pairs_within`) uses this among the built-ins. The
//! point queries (`SpatialIndex::within`, `TruthContacts::within`, and
//! `KnownContacts::within` over belief) are for research plugins, such as a
//! range-limited sensor; the tests and the plugin API chapter cover them.
//!
//! The index is built after motion.
use crate::{Entity, EntitySnapshot, Vec3, scenario::SpatialLookup};
use std::collections::HashMap;

/// An integer grid cell: position divided by the cell width, rounded down.
type Cell = [i64; 3];

/// Cells beyond this many widths from the origin fall back to the scan, so cell
/// arithmetic never overflows or loses precision.
const MAX_CELL: f64 = (1_u64 << 52) as f64;

/// A query scans instead when its columns times this exceed the entity count.
/// DEVNOTE: looking up one column cost about as much as 30 distance tests in
/// reference/spatial_benchmark.py (sparse-4096: ~23 ns per lookup
/// against ~0.7 ns per test). With the fallback at one column per entity, 5 m
/// cells for a 50 m range (441 columns per query) made collision checking three
/// times slower than the scan (large-range-4096: 12.0 against 3.9 ms per step).
const COLUMN_COST_IN_TESTS: i128 = 32;

/// The one distance test every answer uses. It is written as "not at or beyond
/// the range" so that a non-finite distance counts as within, exactly as in the
/// SimpleCollision check this replaced.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub(crate) fn is_within(position_world_m: Vec3, other_world_m: Vec3, range_m: f64) -> bool {
    !((position_world_m - other_world_m).norm() >= range_m)
}

/// Current positions, in entity order, and the grid if the mission uses one.
pub struct SpatialIndex {
    lookup: SpatialLookup,
    ids: Vec<i32>,
    positions_world_m: Vec<Vec3>,
    grid: Option<Grid>,
}

impl SpatialIndex {
    pub(crate) fn build(lookup: SpatialLookup, entities: &[Entity]) -> Self {
        let positions_world_m: Vec<_> = entities
            .iter()
            .map(|entity| entity.truth().position_world_m)
            .collect();
        // DEVNOTE: the grid is built even when every query will fall back to the
        // scan (few agents, or cells much smaller than the range): about 0.02 ms
        // per step with 256 agents, 9% of the step, in
        // reference/spatial_benchmark.py. If that ever matters, skip the
        // build below the entity count a nine-column query needs (9 times
        // COLUMN_COST_IN_TESTS).
        let grid = match lookup {
            SpatialLookup::Scan => None,
            SpatialLookup::Grid { cell_width_m } => Grid::build(cell_width_m, &positions_world_m),
        };
        Self {
            lookup,
            ids: entities.iter().map(Entity::id).collect(),
            positions_world_m,
            grid,
        }
    }

    /// Rebuild if any entity moved, or entities were added, removed, or reordered,
    /// since the last build. Checking costs one pass over the entities.
    pub(crate) fn refresh(&mut self, entities: &[Entity]) {
        let current = entities.len() == self.ids.len()
            && entities
                .iter()
                .zip(self.ids.iter().zip(&self.positions_world_m))
                .all(|(entity, (id, position_world_m))| {
                    entity.id() == *id && entity.truth().position_world_m == *position_world_m
                });
        if !current {
            *self = Self::build(self.lookup, entities);
        }
    }

    /// Indices of the entities within `range_m` of `position_world_m`, ascending.
    /// Positions are those when the index was last refreshed: for an
    /// interaction, the start of its step.
    pub fn within(&self, position_world_m: Vec3, range_m: f64) -> Vec<usize> {
        let within =
            |&index: &usize| is_within(position_world_m, self.positions_world_m[index], range_m);
        let mut found = Vec::new();
        if self.candidates(position_world_m, range_m, &mut found) {
            found.sort_unstable();
            found.retain(within);
        } else {
            found.extend((0..self.positions_world_m.len()).filter(within));
        }
        found
    }

    /// Every pair of entity indices `(first, second)` with `first < second` closer
    /// than `range_m`, in the order a loop over all pairs would visit them: by
    /// `first`, then by `second`. The pairs are found as the loop advances, so a
    /// caller may change entities (for example their health) between pairs.
    pub fn pairs_within(&self, range_m: f64) -> Pairs<'_> {
        Pairs {
            index: self,
            range_m,
            first: 0,
            partners: Vec::new(),
            next_partner: 0,
            next_first: 0,
        }
    }

    /// Entities later than `first` that are within range of it, ascending.
    fn later_within(&self, first: usize, range_m: f64, partners: &mut Vec<usize>) {
        partners.clear();
        let position_world_m = self.positions_world_m[first];
        let within =
            |&second: &usize| is_within(position_world_m, self.positions_world_m[second], range_m);
        if self.candidates(position_world_m, range_m, partners) {
            partners.retain(|&second| second > first);
            partners.sort_unstable();
            partners.retain(within);
        } else {
            partners.extend((first + 1..self.positions_world_m.len()).filter(within));
        }
    }

    /// Adds entity indices that might be within range to `found`, in no
    /// particular order. False (nothing added) when the answer should come from
    /// scanning every entity instead.
    fn candidates(&self, position_world_m: Vec3, range_m: f64, found: &mut Vec<usize>) -> bool {
        self.grid.as_ref().is_some_and(|grid| {
            grid.candidates(
                position_world_m,
                range_m,
                self.positions_world_m.len(),
                found,
            )
        })
    }
}

/// The iterator returned by `SpatialIndex::pairs_within`.
pub struct Pairs<'a> {
    index: &'a SpatialIndex,
    range_m: f64,
    first: usize,
    partners: Vec<usize>,
    next_partner: usize,
    next_first: usize,
}

impl Iterator for Pairs<'_> {
    type Item = (usize, usize);

    fn next(&mut self) -> Option<(usize, usize)> {
        loop {
            if let Some(&second) = self.partners.get(self.next_partner) {
                self.next_partner += 1;
                return Some((self.first, second));
            }
            if self.next_first >= self.index.positions_world_m.len() {
                return None;
            }
            self.first = self.next_first;
            self.next_first += 1;
            self.next_partner = 0;
            self.index
                .later_within(self.first, self.range_m, &mut self.partners);
        }
    }
}

/// A sparse grid: each occupied (x, y) column of cells maps to the entities in
/// it, as (z cell, entity index) pairs, so memory grows with the number of
/// agents, not the map size. The map is only looked up, never iterated, so its
/// order can't reach results.
///
/// Keying by column, not by 3-D cell, keeps a query to a few lookups when
/// agents are spread out horizontally, the usual case. The cost is agents
/// stacked above one another: a query tests every entry in its columns, however
/// far away in height, so many agents over the same spot bring back the
/// every-pair cost. Keying by 3-D cell would fix that but costs more lookups in
/// the spread-out case; change it if a mission stacks agents vertically.
struct Grid {
    cell_width_m: f64,
    columns: HashMap<[i64; 2], Vec<(i64, usize)>>,
}

impl Grid {
    /// `None` when some position has no cell (not finite, or too far out); the
    /// whole tick is then answered by scanning.
    fn build(cell_width_m: f64, positions_world_m: &[Vec3]) -> Option<Self> {
        let mut grid = Self {
            cell_width_m,
            columns: HashMap::new(),
        };
        for (index, position_world_m) in positions_world_m.iter().enumerate() {
            let [x, y, z] = grid.cell(*position_world_m)?;
            grid.columns.entry([x, y]).or_default().push((z, index));
        }
        Some(grid)
    }

    fn cell(&self, position_world_m: Vec3) -> Option<Cell> {
        let axis = |value_m: f64| {
            let cell = (value_m / self.cell_width_m).floor();
            // False for NaN, so a NaN position has no cell.
            (cell.abs() < MAX_CELL).then_some(cell as i64)
        };
        Some([
            axis(position_world_m.x)?,
            axis(position_world_m.y)?,
            axis(position_world_m.z)?,
        ])
    }

    /// Adds the entities in every cell that overlaps a box around the query
    /// sphere. The box is padded slightly, so rounding in the division or in the
    /// exact distance test can never leave out an entity that the test accepts:
    /// an entity closer than the range is within the range on every axis, and
    /// rounding a larger coordinate down to a cell never gives a smaller cell.
    /// False (scan instead) when the box has no cells, or when looking up its
    /// columns would cost more than testing every entity.
    fn candidates(
        &self,
        position_world_m: Vec3,
        range_m: f64,
        entity_count: usize,
        found: &mut Vec<usize>,
    ) -> bool {
        // A NaN range accepts every entity (see `is_within`); only the scan finds them all.
        if range_m.is_nan() {
            return false;
        }
        let reach_m = |value_m: f64| range_m.max(0.0) + 1e-9 * (value_m.abs() + range_m.abs());
        let (Some(low), Some(high)) = (
            self.cell(position_world_m.map(|value_m| value_m - reach_m(value_m))),
            self.cell(position_world_m.map(|value_m| value_m + reach_m(value_m))),
        ) else {
            return false;
        };
        let columns = (high[0] - low[0] + 1) as i128 * (high[1] - low[1] + 1) as i128;
        if columns * COLUMN_COST_IN_TESTS > entity_count as i128 {
            return false;
        }
        for x in low[0]..=high[0] {
            for y in low[1]..=high[1] {
                let Some(column) = self.columns.get(&[x, y]) else {
                    continue;
                };
                found.extend(
                    column
                        .iter()
                        .filter(|(z, _)| (low[2]..=high[2]).contains(z))
                        .map(|(_, index)| *index),
                );
            }
        }
        true
    }
}

/// The true state of every entity, as world plugins see it, with the question
/// "which of these are within range of a point?".
#[derive(Clone, Copy)]
pub struct TruthContacts<'a> {
    list: &'a [EntitySnapshot],
    /// Set by the engine, whose lists are sorted by id; `None` always scans.
    index: Option<&'a SpatialIndex>,
}

impl<'a> TruthContacts<'a> {
    pub(crate) fn new(list: &'a [EntitySnapshot], index: &'a SpatialIndex) -> Self {
        Self {
            list,
            index: Some(index),
        }
    }

    pub fn iter(&self) -> std::slice::Iter<'a, EntitySnapshot> {
        self.list.iter()
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    // To add "the k nearest contacts" when a plugin needs one: take a query
    // position, break distance ties by id so answers are deterministic, and let
    // the caller filter before choosing (FollowNearest skips teammates first;
    // choosing the nearest and then dropping teammates gives a different answer).
    // A grid answers it poorly, because the nearest cells need not hold the
    // nearest agents. A k-d tree or R-tree behind the same methods could answer
    // both questions without plugin changes, and handles clustered agents better.

    /// Every contact closer than `range_m` to `position_world_m`, in this list's
    /// order. Nothing is filtered out: the asking entity and removed entities
    /// are included when in range.
    pub fn within(&self, position_world_m: Vec3, range_m: f64) -> Vec<&'a EntitySnapshot> {
        let within = |contact: &&EntitySnapshot| {
            is_within(position_world_m, contact.truth.position_world_m, range_m)
        };
        let mut candidates = Vec::new();
        let Some(index) = self
            .index
            .filter(|index| index.candidates(position_world_m, range_m, &mut candidates))
        else {
            return self.list.iter().filter(within).collect();
        };
        // Candidates are entity indices; find each by id in this id-sorted list.
        let mut found: Vec<usize> = candidates
            .into_iter()
            .filter_map(|candidate| {
                let id = index.ids[candidate];
                self.list
                    .binary_search_by_key(&id, |contact| contact.id)
                    .ok()
            })
            .collect();
        found.sort_unstable();
        found
            .into_iter()
            .map(|position| &self.list[position])
            .filter(within)
            .collect()
    }
}

impl<'a> From<&'a [EntitySnapshot]> for TruthContacts<'a> {
    /// A plain list, answered by scanning (for tests and plugin initialization).
    fn from(list: &'a [EntitySnapshot]) -> Self {
        Self { list, index: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EntityKind, KinematicState};

    fn index(positions: &[Vec3], cell_width_m: f64) -> SpatialIndex {
        let ids: Vec<i32> = (1..=positions.len() as i32).collect();
        index_with_ids(positions, &ids, cell_width_m)
    }

    fn index_with_ids(positions: &[Vec3], ids: &[i32], cell_width_m: f64) -> SpatialIndex {
        let positions_world_m = positions.to_vec();
        SpatialIndex {
            lookup: SpatialLookup::Grid { cell_width_m },
            ids: ids.to_vec(),
            grid: Grid::build(cell_width_m, &positions_world_m),
            positions_world_m,
        }
    }

    fn scan(positions: &[Vec3]) -> SpatialIndex {
        SpatialIndex {
            lookup: SpatialLookup::Scan,
            ids: (1..=positions.len() as i32).collect(),
            positions_world_m: positions.to_vec(),
            grid: None,
        }
    }

    /// True when the grid, not the scan fallback, answers this query.
    fn answered_by_grid(index: &SpatialIndex, position_world_m: Vec3, range_m: f64) -> bool {
        index.grid.as_ref().is_some_and(|grid| {
            let count = index.positions_world_m.len();
            grid.candidates(position_world_m, range_m, count, &mut Vec::new())
        })
    }

    /// Agents 100 m apart along a line far from everything else: they make the
    /// population large enough for the grid to answer small queries.
    fn far_away(count: usize) -> Vec<Vec3> {
        (0..count)
            .map(|index| Vec3::new(1e6 + index as f64 * 100.0, 0.0, 0.0))
            .collect()
    }

    /// Deterministic points with a mix of spreads, including negative coordinates.
    fn points(count: usize, spread_m: f64) -> Vec<Vec3> {
        let mut state = 12_345_u64;
        let mut next = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            ((state >> 11) as f64 / (1_u64 << 53) as f64 - 0.5) * spread_m
        };
        (0..count)
            .map(|_| Vec3::new(next(), next(), next() * 0.1))
            .collect()
    }

    #[test]
    fn grid_and_scan_give_the_same_pairs_and_neighbours() {
        for (spread_m, range_m, cell_width_m) in [
            (200.0, 5.0, 5.0),
            (200.0, 5.0, 2.0),
            (200.0, 5.0, 17.0),
            (80.0, 8.0, 5.0),
            (1e6, 5e4, 5e4),
        ] {
            let positions = points(2000, spread_m);
            let grid = index(&positions, cell_width_m);
            let probe = grid.grid.as_ref().expect("every point has a cell");
            assert!(probe.candidates(positions[0], range_m, positions.len(), &mut Vec::new()));
            let scan = scan(&positions);
            let pairs: Vec<_> = scan.pairs_within(range_m).collect();
            assert!(!pairs.is_empty(), "a case with no pairs checks nothing");
            assert_eq!(grid.pairs_within(range_m).collect::<Vec<_>>(), pairs);
            for position in positions.iter().take(50) {
                assert_eq!(
                    grid.within(*position, range_m),
                    scan.within(*position, range_m)
                );
            }
        }
    }

    #[test]
    fn points_at_the_range_and_on_cell_faces_are_handled_like_the_scan() {
        // Exactly at the range is outside (strict test); just inside is within.
        let range_m = 5.0;
        let just_inside = 5.0 - 5.0 * f64::EPSILON;
        let mut positions = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(5.0, 0.0, 0.0),
            Vec3::new(-just_inside, 0.0, 0.0),
            Vec3::new(0.0, -5.0, 0.0),
            Vec3::new(0.0, 0.0, just_inside),
            Vec3::new(-10.0, -10.0, -10.0),
            Vec3::new(-10.0, -10.0, -10.0),
        ];
        positions.extend(far_away(5000));
        let scanned: Vec<_> = scan(&positions).pairs_within(range_m).collect();
        assert_eq!(scanned, vec![(0, 2), (0, 4), (5, 6)]);
        for cell_width_m in [10.0, 5.0, 2.5, 1.0] {
            let grid = index(&positions, cell_width_m);
            for position in &positions[..7] {
                assert!(
                    answered_by_grid(&grid, *position, range_m),
                    "{cell_width_m} m"
                );
            }
            assert_eq!(grid.pairs_within(range_m).collect::<Vec<_>>(), scanned);
        }
    }

    #[test]
    fn positions_without_a_cell_fall_back_to_the_scan() {
        let positions = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(f64::NAN, 0.0, 0.0)];
        assert!(index(&positions, 5.0).grid.is_none());
        // A NaN distance counts as within, as in the collision check it replaced.
        assert_eq!(
            scan(&positions).pairs_within(5.0).collect::<Vec<_>>(),
            vec![(0, 1)]
        );
        let far = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(1e300, 0.0, 0.0)];
        assert!(index(&far, 5.0).grid.is_none());
    }

    #[test]
    fn large_ranges_and_bad_ranges_match_the_scan() {
        let positions = points(100, 50.0);
        let grid = index(&positions, 1.0);
        let scan = scan(&positions);
        for range_m in [0.0, -1.0, 1e6, f64::NAN, f64::INFINITY] {
            assert_eq!(
                grid.pairs_within(range_m).collect::<Vec<_>>(),
                scan.pairs_within(range_m).collect::<Vec<_>>(),
                "range {range_m}"
            );
        }
    }

    #[test]
    fn truth_contacts_answer_in_list_order_including_self() {
        // Ids 1 to 4 near the origin (3 is 30 m out; 4 is inactive), then fillers.
        let near = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(3.0, 0.0, 0.0),
            Vec3::new(30.0, 0.0, 0.0),
            Vec3::new(0.0, -4.0, 0.0),
        ];
        let positions: Vec<_> = near.iter().copied().chain(far_away(600)).collect();
        let list: Vec<_> = positions
            .iter()
            .enumerate()
            .map(|(index, position_world_m)| EntitySnapshot {
                id: index as i32 + 1,
                team_id: 1,
                sub_swarm_id: 0,
                active: index != 3,
                kind: EntityKind::Sphere,
                truth: KinematicState {
                    position_world_m: *position_world_m,
                    ..KinematicState::default()
                },
            })
            .collect();
        // The index holds entities in a different order from the id-sorted list.
        let order: Vec<usize> = (0..positions.len()).rev().collect();
        let reordered: Vec<_> = order.iter().map(|&index| positions[index]).collect();
        let ids: Vec<_> = order.iter().map(|&index| index as i32 + 1).collect();
        let grid = index_with_ids(&reordered, &ids, 5.0);
        assert!(answered_by_grid(&grid, near[0], 5.0));

        let ids = |found: Vec<&EntitySnapshot>| found.iter().map(|c| c.id).collect::<Vec<_>>();
        let scanned = TruthContacts::from(&list[..]).within(near[0], 5.0);
        assert_eq!(ids(scanned), vec![1, 2, 4]);
        let gridded = TruthContacts::new(&list, &grid).within(near[0], 5.0);
        assert_eq!(ids(gridded), vec![1, 2, 4]);
    }
}
