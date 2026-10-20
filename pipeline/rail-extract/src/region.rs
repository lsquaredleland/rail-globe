//! Region polygons and a fast, exact point-in-polygon test.
//!
//! A point is tested in three steps, cheapest first:
//!   1. the region's bounding box (two comparisons per axis);
//!   2. a grid over the box whose cells are marked inside, outside or edge (one lookup; most points stop here);
//!   3. for points in edge cells only, an even-odd crossing test against the polygon edges of the point's horizontal band,
//!      in integer arithmetic (1e-7 degrees), so the answer is exact.
//!
//! This keeps a large outline (China: about 1.6 MB of GeoJSON) as cheap per point as a small one.

use crate::cache::BBox;
use std::path::Path;

pub struct Region {
    pub slug: String,
    pub bbox: BBox,       // min lat, min lon, max lat, max lon
    edges: Vec<[i64; 4]>, // lon1, lat1, lon2, lat2
    // bands over latitude: edges whose latitude range overlaps each band
    band_h: i64,
    band_start: Vec<u32>,
    band_edges: Vec<u32>,
    // grid over the bbox: 0 outside, 1 inside, 2 edge
    gw: usize,
    gh: usize,
    cell_w: i64,
    cell_h: i64,
    grid: Vec<u8>,
}

fn deg7(v: f64) -> i64 {
    (v * 1e7).round() as i64
}

pub fn rings_of(geom: &serde_json::Value) -> Vec<Vec<(i64, i64)>> {
    let ring = |r: &serde_json::Value| -> Vec<(i64, i64)> {
        r.as_array().expect("ring").iter().map(|p| (deg7(p[0].as_f64().expect("lon")), deg7(p[1].as_f64().expect("lat")))).collect()
    };
    match geom["type"].as_str() {
        Some("Polygon") => geom["coordinates"].as_array().expect("coordinates").iter().map(ring).collect(),
        Some("MultiPolygon") => {
            geom["coordinates"].as_array().expect("coordinates").iter().flat_map(|p| p.as_array().expect("polygon").iter().map(ring)).collect()
        }
        Some("Feature") => rings_of(&geom["geometry"]),
        Some("FeatureCollection") => geom["features"].as_array().expect("features").iter().flat_map(rings_of).collect(),
        t => panic!("unsupported GeoJSON type {t:?}"),
    }
}

impl Region {
    pub fn load(slug: &str, path: &Path) -> Region {
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read region outline {}: {e}", path.display())))
                .expect("parse region outline");
        Region::new(slug, rings_of(&v))
    }

    pub fn new(slug: &str, rings: Vec<Vec<(i64, i64)>>) -> Region {
        let mut edges = Vec::new();
        let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
        for r in &rings {
            for w in r.windows(2) {
                let (a, b) = (w[0], w[1]);
                if a == b {
                    continue;
                }
                edges.push([a.0, a.1, b.0, b.1]);
            }
            // close the ring if the file does not
            if let (Some(&a), Some(&b)) = (r.last(), r.first()) {
                if a != b {
                    edges.push([a.0, a.1, b.0, b.1]);
                }
            }
            for &(x, y) in r {
                x0 = x0.min(x);
                x1 = x1.max(x);
                y0 = y0.min(y);
                y1 = y1.max(y);
            }
        }
        assert!(!edges.is_empty(), "region {slug} has no polygon");

        // bands: about 4 edges per band on average
        let nb = (edges.len() / 4).clamp(1, 1 << 16) as i64;
        let band_h = ((y1 - y0) / nb + 1).max(1);
        let nbands = ((y1 - y0) / band_h + 1) as usize;
        let mut lists: Vec<Vec<u32>> = vec![Vec::new(); nbands];
        for (i, e) in edges.iter().enumerate() {
            let (lo, hi) = (e[1].min(e[3]), e[1].max(e[3]));
            for list in &mut lists[((lo - y0) / band_h) as usize..=((hi - y0) / band_h) as usize] {
                list.push(i as u32);
            }
        }
        let mut band_start = Vec::with_capacity(nbands + 1);
        let mut band_edges = Vec::new();
        for l in &lists {
            band_start.push(band_edges.len() as u32);
            band_edges.extend_from_slice(l);
        }
        band_start.push(band_edges.len() as u32);

        // grid: up to 512 cells on the longer side
        let span = (x1 - x0).max(y1 - y0).max(1);
        let cell = (span / 512 + 1).max(1);
        let gw = ((x1 - x0) / cell + 1) as usize;
        let gh = ((y1 - y0) / cell + 1) as usize;
        let mut reg = Region {
            slug: slug.to_string(),
            bbox: [y0 as i32, x0 as i32, y1 as i32, x1 as i32],
            edges,
            band_h,
            band_start,
            band_edges,
            gw,
            gh,
            cell_w: cell,
            cell_h: cell,
            grid: vec![0u8; gw * gh],
        };
        // every cell an edge's bbox touches is an edge cell (conservative)
        for e in &reg.edges {
            let cx0 = ((e[0].min(e[2]) - x0) / cell) as usize;
            let cx1 = ((e[0].max(e[2]) - x0) / cell) as usize;
            let cy0 = ((e[1].min(e[3]) - y0) / cell) as usize;
            let cy1 = ((e[1].max(e[3]) - y0) / cell) as usize;
            for cy in cy0..=cy1 {
                for cx in cx0..=cx1 {
                    reg.grid[cy * gw + cx] = 2;
                }
            }
        }
        // cells without an edge are wholly inside or wholly outside: test their centre once
        for cy in 0..gh {
            for cx in 0..gw {
                if reg.grid[cy * gw + cx] == 2 {
                    continue;
                }
                let px = x0 + cx as i64 * cell + cell / 2;
                let py = y0 + cy as i64 * cell + cell / 2;
                reg.grid[cy * gw + cx] = reg.exact(px, py) as u8;
            }
        }
        reg
    }

    /// even-odd crossing test against the edges of the point's band (ray towards +x)
    fn exact(&self, px: i64, py: i64) -> bool {
        let y0 = self.bbox[0] as i64;
        let b = ((py - y0) / self.band_h) as usize;
        if b + 1 >= self.band_start.len() {
            return false;
        }
        let mut inside = false;
        for &ei in &self.band_edges[self.band_start[b] as usize..self.band_start[b + 1] as usize] {
            let [x1, y1, x2, y2] = self.edges[ei as usize];
            if (y1 > py) != (y2 > py) {
                // does the edge cross the ray to the right of the point? (x of the crossing > px)
                let lhs = (x2 - x1) as i128 * (py - y1) as i128;
                let rhs = (px - x1) as i128 * (y2 - y1) as i128;
                if if y2 > y1 { lhs > rhs } else { lhs < rhs } {
                    inside = !inside;
                }
            }
        }
        inside
    }

    #[inline]
    pub fn contains(&self, lat: i32, lon: i32) -> bool {
        let b = &self.bbox;
        if lat < b[0] || lat > b[2] || lon < b[1] || lon > b[3] {
            return false;
        }
        let (px, py) = (lon as i64, lat as i64);
        let cx = ((px - b[1] as i64) / self.cell_w) as usize;
        let cy = ((py - b[0] as i64) / self.cell_h) as usize;
        match self.grid[cy.min(self.gh - 1) * self.gw + cx.min(self.gw - 1)] {
            0 => false,
            1 => true,
            _ => self.exact(px, py),
        }
    }
}
