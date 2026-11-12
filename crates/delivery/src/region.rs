//! The region: a nominal square grid, displaced by a layout seed or by
//! explicit hub coordinates. The dispatcher places the hubs and builds each
//! hub's couriers there, so one `region` setting is the whole layout.
//!
//! Hubs are numbered from 1 in row order. Hub `h` has clinics `hA` to `hF`,
//! 4 to 13 km out at angles that turn with the hub number, and hospitals
//! `hH1` and `hH2` at 1.5 and 2.5 km, whose rooftops take only a landing.
//! Each hospital also has a ground pad 300 m east of it for when the rooftop
//! is closed.

use anyhow::{Result, ensure};
use ripple_core::Vec3;
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Mission parameters; `Default` supplies any key the mission leaves out.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RegionConfig {
    /// A square number: 1, 4, 16, 64, 256.
    pub hubs: u32,
    pub hub_spacing_m: f64,
    /// Optional [east, north] coordinates in hub-ID order. Empty uses the
    /// grid, or the `layout` drawn from it.
    pub hub_positions_m: Vec<[f64; 2]>,
    /// The grid displaced by a seed, so a sweep names a layout by one
    /// number.
    pub layout: LayoutConfig,
    /// Hubs on a side of the block the fleet counts and the scripted events
    /// are written for; the dispatcher lays them over every block of the
    /// grid (`Region::tiled_hub`). 0 is the whole region as one block.
    pub block_side: u32,
}

impl Default for RegionConfig {
    fn default() -> Self {
        Self {
            hubs: 1,
            hub_spacing_m: 15_000.0,
            hub_positions_m: Vec::new(),
            layout: LayoutConfig::default(),
            block_side: 0,
        }
    }
}

/// A displaced layout: each hub moves from its grid point to a uniform
/// point in a disk of `jitter_m`, redrawn while it would come within
/// `min_spacing_m` of a hub already placed. Every draw is keyed by the seed,
/// the hub, the attempt and its purpose, apart from the day's request and
/// event numbers; positions are rounded to 0.1 m. Seed 0 is the grid.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LayoutConfig {
    pub seed: u32,
    pub jitter_m: f64,
    pub min_spacing_m: f64,
}

#[derive(Clone, Debug)]
pub struct Hub {
    /// Numbered from 1.
    pub id: u32,
    pub position_world_m: Vec3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SiteKind {
    Clinic,
    /// A hospital rooftop; landing only, unless an order opens its pad.
    Hospital,
}

#[derive(Clone, Debug)]
pub struct Site {
    /// `3A` for hub 3's clinic A; `3H1` for its first hospital.
    pub code: String,
    pub hub: u32,
    pub kind: SiteKind,
    pub position_world_m: Vec3,
    /// The ground pad for a hospital; the site itself for a clinic.
    pub pad_world_m: Vec3,
}

impl Site {
    pub fn rooftop(&self) -> bool {
        self.kind == SiteKind::Hospital
    }
}

#[derive(Clone, Debug)]
pub struct Region {
    pub hubs: Vec<Hub>,
    pub sites: Vec<Site>,
    pub hub_spacing_m: f64,
    /// Hubs on a side of the block the mission is written for; the whole
    /// side when the mission names none.
    pub block_side: u32,
}

/// Clinic radii in km, A to F.
const CLINIC_RADII_KM: [f64; 6] = [4.0, 7.0, 10.0, 13.0, 6.0, 9.0];
const HOSPITAL_RADII_KM: [f64; 2] = [1.5, 2.5];
pub const PAD_OFFSET_M: f64 = 300.0;
pub const ROOFTOP_HEIGHT_M: f64 = 40.0;

/// Hubs on a side of the square grid, checking the region's numbers.
pub fn side(config: &RegionConfig) -> Result<u32> {
    let side = (f64::from(config.hubs).sqrt()).round() as u32;
    ensure!(
        config.hubs >= 1 && side * side == config.hubs,
        "region.hubs must be a square number (1, 4, 16, 64, 256), not {}",
        config.hubs
    );
    ensure!(
        config.hub_spacing_m.is_finite() && config.hub_spacing_m > 0.0,
        "region.hub_spacing_m must be positive"
    );
    ensure!(
        config.hub_positions_m.is_empty()
            || (config.hub_positions_m.len() == config.hubs as usize
                && config
                    .hub_positions_m
                    .iter()
                    .flatten()
                    .all(|v| v.is_finite())),
        "region.hub_positions_m must be empty or contain one finite [east, north] pair per hub"
    );
    ensure!(
        config.block_side == 0 || side.is_multiple_of(config.block_side),
        "region.block_side must be 0 or divide the region's side ({side}), not {}",
        config.block_side
    );
    let layout = &config.layout;
    ensure!(
        layout.jitter_m.is_finite()
            && layout.jitter_m >= 0.0
            && layout.min_spacing_m.is_finite()
            && layout.min_spacing_m >= 0.0,
        "region.layout.jitter_m and min_spacing_m must be finite and not negative"
    );
    ensure!(
        layout.seed == 0 || config.hub_positions_m.is_empty(),
        "region.layout.seed and region.hub_positions_m both place the hubs; give one"
    );
    ensure!(
        layout.seed == 0 || (layout.jitter_m > 0.0 && layout.min_spacing_m > 0.0),
        "region.layout.seed needs positive jitter_m and min_spacing_m"
    );
    Ok(side)
}

impl RegionConfig {
    /// Every hub's position in hub order: the explicit list, the layout
    /// drawn from the seed, or the grid.
    pub fn positions(&self) -> Result<Vec<Vec3>> {
        let side = side(self)?;
        if !self.hub_positions_m.is_empty() {
            return Ok(self
                .hub_positions_m
                .iter()
                .map(|[x, y]| Vec3::new(*x, *y, 0.0))
                .collect());
        }
        if self.layout.seed == 0 {
            return Ok((1..=self.hubs)
                .map(|id| hub_position_m(id, side, self.hub_spacing_m))
                .collect());
        }
        let mut positions: Vec<Vec3> = Vec::new();
        for hub in 1..=self.hubs {
            let grid = hub_position_m(hub, side, self.hub_spacing_m);
            let mut placed = None;
            for attempt in 0..10_000 {
                let uniform = |purpose: &str| {
                    let key = format!(
                        "ripple-layout:{}:{hub}:{attempt}:{purpose}",
                        self.layout.seed
                    );
                    let digest = Sha256::digest(key.as_bytes());
                    let bits = u64::from_be_bytes(digest[..8].try_into().expect("8 bytes")) >> 11;
                    (bits as f64 + 0.5) / 2f64.powi(53)
                };
                let radius_m = self.layout.jitter_m * uniform("radius").sqrt();
                let angle_rad = std::f64::consts::TAU * uniform("angle");
                let point = Vec3::new(
                    grid.x + radius_m * angle_rad.cos(),
                    grid.y + radius_m * angle_rad.sin(),
                    0.0,
                );
                if positions
                    .iter()
                    .all(|other| horizontal_m(point, *other) >= self.layout.min_spacing_m)
                {
                    placed = Some(point);
                    break;
                }
            }
            let Some(point) = placed else {
                anyhow::bail!(
                    "region.layout.seed {}: cannot place hub {hub}; reduce min_spacing_m or jitter_m",
                    self.layout.seed
                );
            };
            positions.push(point);
        }
        Ok(positions
            .into_iter()
            .map(|p| {
                Vec3::new(
                    (p.x * 10.0).round() / 10.0,
                    (p.y * 10.0).round() / 10.0,
                    0.0,
                )
            })
            .collect())
    }
}

impl Region {
    pub fn new(config: &RegionConfig) -> Result<Self> {
        let side = side(config)?;
        let positions = config.positions()?;
        let mut hubs = Vec::new();
        let mut sites = Vec::new();
        for row in 0..side {
            for column in 0..side {
                let id = row * side + column + 1;
                let position_world_m = positions[(id - 1) as usize];
                hubs.push(Hub {
                    id,
                    position_world_m,
                });
                // Angles turn with the hub so neighbors' clinics interleave.
                let turn_deg = 17.0 * f64::from(id);
                for (index, radius_km) in CLINIC_RADII_KM.iter().enumerate() {
                    let angle_rad = (60.0 * index as f64 + turn_deg).to_radians();
                    let position = position_world_m
                        + Vec3::new(
                            radius_km * 1000.0 * angle_rad.cos(),
                            radius_km * 1000.0 * angle_rad.sin(),
                            0.0,
                        );
                    sites.push(Site {
                        code: format!("{id}{}", (b'A' + index as u8) as char),
                        hub: id,
                        kind: SiteKind::Clinic,
                        position_world_m: position,
                        pad_world_m: position,
                    });
                }
                for (index, radius_km) in HOSPITAL_RADII_KM.iter().enumerate() {
                    let angle_rad = (30.0 + 180.0 * index as f64 + turn_deg).to_radians();
                    let position = position_world_m
                        + Vec3::new(
                            radius_km * 1000.0 * angle_rad.cos(),
                            radius_km * 1000.0 * angle_rad.sin(),
                            ROOFTOP_HEIGHT_M,
                        );
                    sites.push(Site {
                        code: format!("{id}H{}", index + 1),
                        hub: id,
                        kind: SiteKind::Hospital,
                        position_world_m: position,
                        pad_world_m: Vec3::new(position.x + PAD_OFFSET_M, position.y, 0.0),
                    });
                }
            }
        }
        Ok(Self {
            hubs,
            sites,
            hub_spacing_m: config.hub_spacing_m,
            block_side: if config.block_side == 0 {
                side
            } else {
                config.block_side
            },
        })
    }

    pub fn hub(&self, id: u32) -> Option<&Hub> {
        self.hubs.get(id.checked_sub(1)? as usize)
    }

    pub fn site(&self, code: &str) -> Option<&Site> {
        let code = code.trim().to_uppercase();
        self.sites.iter().find(|site| site.code == code)
    }

    pub fn sites_of(&self, hub: u32) -> impl Iterator<Item = &Site> {
        self.sites.iter().filter(move |site| site.hub == hub)
    }

    /// Hubs per side of the grid.
    pub fn side(&self) -> u32 {
        (self.hubs.len() as f64).sqrt().round() as u32
    }

    /// The hubs of each cluster: square blocks of `cluster_side` hubs, row
    /// by row, for a fleet manager per cluster.
    pub fn clusters(&self, cluster_side: u32) -> Vec<Vec<u32>> {
        let side = self.side();
        let per_row = side / cluster_side;
        let mut clusters = vec![Vec::new(); (per_row * per_row) as usize];
        for hub in &self.hubs {
            let index = hub.id - 1;
            let (row, column) = (index / side, index % side);
            let cluster = (row / cluster_side) * per_row + column / cluster_side;
            clusters[cluster as usize].push(hub.id);
        }
        clusters
    }

    /// The blocks the region is tiled with, `block_side` hubs a side.
    pub fn blocks(&self) -> usize {
        let per_row = self.side() / self.block_side;
        (per_row * per_row) as usize
    }

    /// Hubs in one block: what a tiled fleet count or scripted event names.
    pub fn block_hubs(&self) -> u32 {
        self.block_side * self.block_side
    }

    /// The region's hub at block-local number `local` (from 1, in row order
    /// within the block) of block `block` (from 0, in row order over the
    /// grid). The block's number picks one of the eight symmetries of the
    /// square, so the same local hub sits in a different corner or edge of
    /// each block while every distance within the block stays the same.
    pub fn tiled_hub(&self, block: usize, local: u32) -> u32 {
        let (side, b) = (self.side(), self.block_side);
        let per_row = side / b;
        let (row, column) = turned((local - 1) / b, (local - 1) % b, b, block % 8);
        let block = block as u32;
        ((block / per_row) * b + row) * side + (block % per_row) * b + column + 1
    }

    /// The block-local number of a region hub: the inverse of `tiled_hub`.
    pub fn local_hub(&self, hub: u32) -> u32 {
        let (side, b) = (self.side(), self.block_side);
        let (row, column) = ((hub - 1) / side, (hub - 1) % side);
        let block = ((row / b) * (side / b) + column / b) as usize;
        (1..=self.block_hubs())
            .find(|&local| self.tiled_hub(block, local) == hub)
            .expect("every symmetry of the block is a bijection")
    }

    /// Hubs other than `hub`, nearest first.
    pub fn neighbors(&self, hub: u32) -> Vec<&Hub> {
        let Some(center) = self.hub(hub) else {
            return Vec::new();
        };
        let mut others: Vec<&Hub> = self.hubs.iter().filter(|h| h.id != hub).collect();
        others.sort_by(|a, b| {
            horizontal_m(a.position_world_m, center.position_world_m)
                .total_cmp(&horizontal_m(b.position_world_m, center.position_world_m))
                .then(a.id.cmp(&b.id))
        });
        others
    }

    /// `hub` and the hubs one grid step away (up to four).
    pub fn group(&self, hub: u32) -> Vec<u32> {
        let mut group = vec![hub];
        if let Some(center) = self.hub(hub) {
            for other in &self.hubs {
                if other.id != hub
                    && horizontal_m(other.position_world_m, center.position_world_m)
                        <= self.hub_spacing_m * 1.1
                {
                    group.push(other.id);
                }
            }
        }
        group
    }
}

/// Where hub `id` sits: the grid is centered on the origin.
pub fn hub_position_m(id: u32, side: u32, spacing_m: f64) -> Vec3 {
    let index = id - 1;
    let (row, column) = (index / side, index % side);
    let offset = f64::from(side - 1) / 2.0;
    Vec3::new(
        (f64::from(column) - offset) * spacing_m,
        (f64::from(row) - offset) * spacing_m,
        0.0,
    )
}

/// A point (row, column) of a square `side` wide under symmetry `s` of the
/// square, 0 to 7: `s & 3` quarter turns, after a mirror when `s & 4` is set.
fn turned(row: u32, column: u32, side: u32, s: usize) -> (u32, u32) {
    let last = side - 1;
    let (mut r, mut c) = if s & 4 != 0 {
        (row, last - column)
    } else {
        (row, column)
    };
    for _ in 0..(s & 3) {
        (r, c) = (c, last - r);
    }
    (r, c)
}

/// Distance in the horizontal plane.
pub fn horizontal_m(a: Vec3, b: Vec3) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The seeded layout reproduces the layouts `region.py` wrote into the
    /// incident sweeps (16 hubs seed 101, 64 hubs seed 2, 256 hubs seed 101),
    /// first and last hub of each.
    #[test]
    fn a_layout_seed_redraws_the_written_layouts() {
        let layout = |hubs, seed| {
            RegionConfig {
                hubs,
                hub_spacing_m: 7000.0,
                layout: LayoutConfig {
                    seed,
                    jitter_m: 1500.0,
                    min_spacing_m: 4000.0,
                },
                ..RegionConfig::default()
            }
            .positions()
            .unwrap()
        };
        let p = layout(16, 101);
        assert_eq!(p[0], Vec3::new(-11709.3, -9794.9, 0.0));
        assert_eq!(p[15], Vec3::new(10544.1, 10993.7, 0.0));
        let p = layout(64, 2);
        assert_eq!(p[0], Vec3::new(-24539.5, -24724.7, 0.0));
        assert_eq!(p[63], Vec3::new(25832.4, 25009.7, 0.0));
        let p = layout(256, 101);
        assert_eq!(p[0], Vec3::new(-53709.3, -51794.9, 0.0));
        assert_eq!(p[255], Vec3::new(53752.8, 51861.7, 0.0));
        assert!(
            p.iter()
                .enumerate()
                .all(|(i, a)| p[..i].iter().all(|b| horizontal_m(*a, *b) >= 4000.0)),
            "hubs at least min_spacing_m apart"
        );
        // Seed 0 is the grid; a seed with an explicit list is refused.
        let grid = layout(16, 0);
        assert_eq!(grid[0], hub_position_m(1, 4, 7000.0));
        let both = RegionConfig {
            hubs: 1,
            hub_positions_m: vec![[0.0, 0.0]],
            layout: LayoutConfig {
                seed: 3,
                jitter_m: 100.0,
                min_spacing_m: 10.0,
            },
            ..RegionConfig::default()
        };
        assert!(both.positions().is_err());
    }

    #[test]
    fn tiling_lays_the_block_over_every_block_turned_and_inverts() {
        let region = Region::new(&RegionConfig {
            hubs: 64,
            hub_spacing_m: 7000.0,
            block_side: 4,
            ..RegionConfig::default()
        })
        .unwrap();
        assert_eq!((region.blocks(), region.block_hubs()), (4, 16));
        let clusters = region.clusters(4);
        for block in 0..4 {
            let mut tiled: Vec<u32> = (1..=16)
                .map(|local| region.tiled_hub(block, local))
                .collect();
            for &hub in &tiled {
                assert_eq!(region.tiled_hub(block, region.local_hub(hub)), hub);
            }
            tiled.sort_unstable();
            assert_eq!(
                tiled, clusters[block],
                "block {block} is a permutation of its hubs"
            );
        }
        // Block 0 keeps the grid's numbering; the next three turn it, so local
        // hub 1 sits at a different corner of each.
        assert_eq!(region.tiled_hub(0, 6), 10);
        let corners: Vec<u32> = (0..4).map(|block| region.tiled_hub(block, 1)).collect();
        assert_eq!(corners, vec![1, 8, 60, 61]);
        // One block the size of the region is the identity.
        let whole = Region::new(&RegionConfig {
            hubs: 16,
            block_side: 4,
            ..RegionConfig::default()
        })
        .unwrap();
        assert!((1..=16).all(|hub| whole.local_hub(hub) == hub && whole.tiled_hub(0, hub) == hub));
        assert!(
            Region::new(&RegionConfig {
                hubs: 16,
                block_side: 3,
                ..RegionConfig::default()
            })
            .is_err()
        );
    }

    #[test]
    fn a_four_hub_region_is_a_centered_square_with_eight_sites_each() {
        let region = Region::new(&RegionConfig {
            hubs: 4,
            hub_spacing_m: 15_000.0,
            ..RegionConfig::default()
        })
        .unwrap();
        assert_eq!(region.hubs.len(), 4);
        assert_eq!(region.sites.len(), 32);
        assert_eq!(
            region.hub(1).unwrap().position_world_m,
            Vec3::new(-7500.0, -7500.0, 0.0)
        );
        assert_eq!(
            region.hub(4).unwrap().position_world_m,
            Vec3::new(7500.0, 7500.0, 0.0)
        );
        assert!(region.hub(0).is_none() && region.hub(5).is_none());
        let far = region.site("3D").unwrap();
        assert_eq!(far.hub, 3);
        let distance_km = horizontal_m(
            far.position_world_m,
            region.hub(3).unwrap().position_world_m,
        ) / 1000.0;
        assert!((distance_km - 13.0).abs() < 1e-9);
        let hospital = region.site("1h2").unwrap();
        assert!(hospital.rooftop());
        assert_eq!(hospital.pad_world_m.z, 0.0);
        assert_eq!(
            hospital.pad_world_m.x - hospital.position_world_m.x,
            PAD_OFFSET_M
        );
        assert_eq!(region.group(1), vec![1, 2, 3]);
        assert_eq!(region.neighbors(1)[2].id, 4);
        assert!(
            Region::new(&RegionConfig {
                hubs: 6,
                hub_spacing_m: 1.0,
                ..RegionConfig::default()
            })
            .is_err()
        );
    }
}
