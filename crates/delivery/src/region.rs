//! The region: a nominal square grid, optionally displaced by explicit hub
//! coordinates. The dispatcher places the hubs and builds each hub's
//! couriers there, so one `region` setting is the whole layout.
//!
//! Hubs are numbered from 1 in row order. Hub `h` has clinics `hA` to `hF`,
//! 4 to 13 km out at angles that turn with the hub number, and hospitals
//! `hH1` and `hH2` at 1.5 and 2.5 km, whose rooftops take only a landing.
//! Each hospital also has a ground pad 300 m east of it for when the rooftop
//! is closed.

use anyhow::{Result, ensure};
use ripple_core::Vec3;
use serde::{Deserialize, Serialize};

/// Mission parameters; `Default` supplies any key the mission leaves out.
/// Serializes too: the dispatcher hands it to each courier it builds.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct RegionConfig {
    /// A square number: 1, 4, 16, 64, 256.
    pub hubs: u32,
    pub hub_spacing_m: f64,
    /// Optional [east, north] coordinates in hub-ID order. Empty uses the grid.
    pub hub_positions_m: Vec<[f64; 2]>,
}

impl Default for RegionConfig {
    fn default() -> Self {
        Self {
            hubs: 1,
            hub_spacing_m: 15_000.0,
            hub_positions_m: Vec::new(),
        }
    }
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
    Ok(side)
}

impl RegionConfig {
    /// Called after `side` has checked the layout and the caller's hub ID.
    pub fn position_m(&self, id: u32) -> Vec3 {
        if self.hub_positions_m.is_empty() {
            hub_position_m(
                id,
                (f64::from(self.hubs).sqrt()).round() as u32,
                self.hub_spacing_m,
            )
        } else {
            let [x, y] = self.hub_positions_m[(id - 1) as usize];
            Vec3::new(x, y, 0.0)
        }
    }
}

impl Region {
    pub fn new(config: &RegionConfig) -> Result<Self> {
        let side = side(config)?;
        let mut hubs = Vec::new();
        let mut sites = Vec::new();
        for row in 0..side {
            for column in 0..side {
                let id = row * side + column + 1;
                let position_world_m = config.position_m(id);
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
    /// by row. A fleet manager per cluster is direction 4 of the plan.
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

/// Distance in the horizontal plane.
pub fn horizontal_m(a: Vec3, b: Vec3) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

#[cfg(test)]
mod tests {
    use super::*;

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
