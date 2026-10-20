//! Geometry for the station model and tile build, on `geo` (shapely/GEOS in the Python version) and `rstar` (shapely's STRtree).

use geo::{
    Area, BooleanOps, BoundingRect, Centroid, Contains, Coord, Distance, Euclidean, Geometry, InteriorPoint, LineString, MultiLineString,
    MultiPolygon, Point, Polygon, Rect,
};
use rstar::{primitives::GeomWithData, primitives::Rectangle, RTree, AABB};
use serde_json::Value;

pub type G = Geometry<f64>;

fn coord(v: &Value, f: &impl Fn(f64, f64) -> (f64, f64)) -> Option<Coord<f64>> {
    let a = v.as_array()?;
    let (x, y) = f(a.first()?.as_f64()?, a.get(1)?.as_f64()?);
    Some(Coord { x, y })
}

fn line(v: &Value, f: &impl Fn(f64, f64) -> (f64, f64)) -> Option<LineString<f64>> {
    Some(LineString::new(v.as_array()?.iter().map(|c| coord(c, f)).collect::<Option<Vec<_>>>()?))
}

fn polygon(v: &Value, f: &impl Fn(f64, f64) -> (f64, f64)) -> Option<Polygon<f64>> {
    let mut rings = v.as_array()?.iter().map(|r| line(r, f));
    let outer = rings.next()??;
    Some(Polygon::new(outer, rings.collect::<Option<Vec<_>>>()?))
}

/// a GeoJSON geometry, with every coordinate passed through `f` (a projection, or identity)
pub fn from_geojson(g: &Value, f: impl Fn(f64, f64) -> (f64, f64)) -> Option<G> {
    let c = &g["coordinates"];
    Some(match g["type"].as_str()? {
        "Point" => Geometry::Point(Point::from(coord(c, &f)?)),
        "LineString" => Geometry::LineString(line(c, &f)?),
        "MultiLineString" => Geometry::MultiLineString(MultiLineString::new(c.as_array()?.iter().map(|l| line(l, &f)).collect::<Option<_>>()?)),
        "Polygon" => Geometry::Polygon(polygon(c, &f)?),
        "MultiPolygon" => Geometry::MultiPolygon(MultiPolygon::new(c.as_array()?.iter().map(|p| polygon(p, &f)).collect::<Option<_>>()?)),
        _ => return None,
    })
}

pub fn id(x: f64, y: f64) -> (f64, f64) {
    (x, y)
}

pub fn bbox(g: &G) -> Option<Rect<f64>> {
    g.bounding_rect()
}

pub fn is_empty(g: &G) -> bool {
    match g {
        Geometry::LineString(l) => l.0.is_empty(),
        Geometry::Polygon(p) => p.exterior().0.is_empty(),
        Geometry::MultiPolygon(m) => m.0.is_empty(),
        Geometry::MultiLineString(m) => m.0.is_empty(),
        Geometry::GeometryCollection(c) => c.0.is_empty(),
        _ => false,
    }
}

/// shapely's distance (planar)
#[inline]
pub fn dist(a: &G, b: &G) -> f64 {
    Euclidean.distance(a, b)
}

#[inline]
pub fn dist_pt(a: &G, p: &Point<f64>) -> f64 {
    Euclidean.distance(a, p)
}

/// polygon contains point (strictly inside or on the boundary is what GEOS calls "contains" minus the boundary; here geo's
/// Contains, which also excludes the boundary)
#[inline]
pub fn contains_pt(g: &G, p: &Point<f64>) -> bool {
    g.contains(p)
}

/// shapely's representative_point, ported from GEOS (InteriorPointArea / InteriorPointLine) so the same point comes out: which
/// station a platform joins and which land a station's probes hit depend on it
pub fn rep_point(g: &G) -> Option<Point<f64>> {
    match g {
        Geometry::Point(p) => Some(*p),
        Geometry::Polygon(p) => area_point(std::slice::from_ref(p)),
        Geometry::MultiPolygon(m) => area_point(&m.0),
        Geometry::LineString(l) => line_point(std::slice::from_ref(l)),
        Geometry::MultiLineString(m) => line_point(&m.0),
        _ => g.interior_point(),
    }
}

/// GEOS InteriorPointArea: per polygon, a horizontal scan line at a y between vertices near the middle; the midpoint of the widest
/// inside section; over several polygons the widest wins
fn area_point(polys: &[Polygon<f64>]) -> Option<Point<f64>> {
    let avg = |a: f64, b: f64| (a + b) / 2.0;
    let mut best: Option<(f64, Point<f64>)> = None;
    for p in polys {
        let shell = p.exterior();
        if shell.0.is_empty() {
            continue;
        }
        let (lo, hi) = shell.0.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), c| (a.min(c.y), b.max(c.y)));
        let centre = avg(lo, hi);
        let (mut lo_y, mut hi_y) = (lo, hi);
        for ring in std::iter::once(shell).chain(p.interiors()) {
            for c in &ring.0 {
                if c.y <= centre {
                    if c.y > lo_y {
                        lo_y = c.y;
                    }
                } else if c.y < hi_y {
                    hi_y = c.y;
                }
            }
        }
        let y = avg(hi_y, lo_y);
        let mut xs: Vec<f64> = Vec::new();
        for ring in std::iter::once(shell).chain(p.interiors()) {
            let (rlo, rhi) = ring.0.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), c| (a.min(c.y), b.max(c.y)));
            if y < rlo || y > rhi {
                continue;
            }
            for w in ring.0.windows(2) {
                let (p0, p1) = (w[0], w[1]);
                if (p0.y > y && p1.y > y) || (p0.y < y && p1.y < y) {
                    continue;
                }
                // GEOS isEdgeCrossingCounted: not horizontal, and an end on the line counts only from below
                if p0.y == p1.y || (p0.y == y && p1.y < y) || (p1.y == y && p0.y < y) {
                    continue;
                }
                let x = if p0.x == p1.x { p0.x } else { p0.x + (y - p0.y) / ((p1.y - p0.y) / (p1.x - p0.x)) };
                xs.push(x);
            }
        }
        // default when the polygon has zero area: its first coordinate
        let mut pt = Point::from(shell.0[0]);
        let mut width = 0.0;
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        for pair in xs.as_chunks::<2>().0 {
            let w = pair[1] - pair[0];
            if w > width {
                width = w;
                pt = Point::new(avg(pair[0], pair[1]), y);
            }
        }
        if best.is_none_or(|(bw, _)| width > bw) {
            best = Some((width, pt));
        }
    }
    best.map(|b| b.1)
}

/// GEOS InteriorPointLine: the interior vertex nearest the length-weighted centroid, else the nearest end point
fn line_point(lines: &[LineString<f64>]) -> Option<Point<f64>> {
    let (mut sx, mut sy, mut total) = (0.0, 0.0, 0.0);
    for l in lines {
        for w in l.0.windows(2) {
            let len = ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2)).sqrt();
            sx += len * (w[0].x + w[1].x) / 2.0;
            sy += len * (w[0].y + w[1].y) / 2.0;
            total += len;
        }
    }
    let first = lines.iter().find_map(|l| l.0.first())?;
    let (cx, cy) = if total > 0.0 { (sx / total, sy / total) } else { (first.x, first.y) };
    let mut best: Option<(f64, Coord<f64>)> = None;
    let add = |c: Coord<f64>, best: &mut Option<(f64, Coord<f64>)>| {
        let d = ((c.x - cx).powi(2) + (c.y - cy).powi(2)).sqrt();
        if best.is_none_or(|(bd, _)| d < bd) {
            *best = Some((d, c));
        }
    };
    for l in lines {
        if l.0.len() > 2 {
            for c in &l.0[1..l.0.len() - 1] {
                add(*c, &mut best);
            }
        }
    }
    if best.is_none() {
        for l in lines {
            if let (Some(a), Some(b)) = (l.0.first(), l.0.last()) {
                add(*a, &mut best);
                add(*b, &mut best);
            }
        }
    }
    best.map(|b| Point::from(b.1))
}

pub fn area(g: &G) -> f64 {
    g.unsigned_area()
}

pub fn centroid(g: &G) -> Option<Point<f64>> {
    g.centroid()
}

/// the polygonal part of a geometry
pub fn polys(g: &G) -> MultiPolygon<f64> {
    match g {
        Geometry::Polygon(p) => MultiPolygon::new(vec![p.clone()]),
        Geometry::MultiPolygon(m) => m.clone(),
        Geometry::GeometryCollection(c) => MultiPolygon::new(c.0.iter().flat_map(|x| polys(x).0).collect()),
        _ => MultiPolygon::new(vec![]),
    }
}

/// union of polygons (shapely.unary_union)
pub fn union_all(mut ps: Vec<MultiPolygon<f64>>) -> MultiPolygon<f64> {
    // pairwise, so each union stays small
    while ps.len() > 1 {
        let mut next = Vec::with_capacity(ps.len() / 2 + 1);
        let mut it = ps.into_iter();
        while let Some(a) = it.next() {
            match it.next() {
                Some(b) => next.push(a.union(&b)),
                None => next.push(a),
            }
        }
        ps = next;
    }
    ps.pop().unwrap_or_else(|| MultiPolygon::new(vec![]))
}

pub fn intersection(a: &MultiPolygon<f64>, b: &Polygon<f64>) -> MultiPolygon<f64> {
    a.intersection(&MultiPolygon::new(vec![b.clone()]))
}

/// shapely 2 point.buffer(r): a 64-sided circle (16 segments per quarter), clockwise from angle 0
pub fn circle(c: Point<f64>, r: f64) -> Polygon<f64> {
    let n = 64;
    let pts: Vec<Coord<f64>> = (0..=n)
        .map(|k| {
            let a = -(k as f64) * std::f64::consts::TAU / n as f64;
            Coord { x: c.x() + r * a.cos(), y: c.y() + r * a.sin() }
        })
        .collect();
    Polygon::new(LineString::new(pts), vec![])
}

/// shapely.make_valid when a polygon is invalid (self-intersecting rings and the like)
pub fn make_valid(g: G) -> G {
    use geo::Validation;
    match &g {
        Geometry::Polygon(_) | Geometry::MultiPolygon(_) if !g.is_valid() => {
            // a union of the polygon with nothing rebuilds it from its rings into valid polygons
            let mp = polys(&g);
            let fixed = mp.union(&MultiPolygon::new(vec![]));
            if fixed.0.len() == 1 {
                Geometry::Polygon(fixed.0.into_iter().next().unwrap())
            } else {
                Geometry::MultiPolygon(fixed)
            }
        }
        _ => g,
    }
}

/// shapely's STRtree: candidates whose bounding box meets a query box, returned sorted by index (shapely returns them in
/// its tree order; sorting makes ties resolve the same way every run)
pub struct Index {
    tree: RTree<GeomWithData<Rectangle<[f64; 2]>, usize>>,
}

impl Index {
    pub fn new(geoms: impl Iterator<Item = Option<Rect<f64>>>) -> Index {
        let items = geoms
            .enumerate()
            .filter_map(|(i, r)| r.map(|r| GeomWithData::new(Rectangle::from_corners([r.min().x, r.min().y], [r.max().x, r.max().y]), i)))
            .collect();
        Index { tree: RTree::bulk_load(items) }
    }
    /// indices whose box meets [x0, x1] x [y0, y1]
    pub fn query(&self, x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<usize> {
        let mut v: Vec<usize> =
            self.tree.locate_in_envelope_intersecting(AABB::from_corners([x0, y0], [x1, y1])).map(|g| g.data).collect();
        v.sort_unstable();
        v
    }
    /// like STRtree.query(g.buffer(r)): boxes meeting g's box grown by r
    pub fn near(&self, g: &G, r: f64) -> Vec<usize> {
        match bbox(g) {
            Some(b) => self.query(b.min().x - r, b.min().y - r, b.max().x + r, b.max().y + r),
            None => vec![],
        }
    }
    pub fn at(&self, p: &Point<f64>) -> Vec<usize> {
        self.query(p.x(), p.y(), p.x(), p.y())
    }
}

/// Disjoint sets over `0..n` with path halving. The root of a set is deterministic, and callers that iterate by root rely on that.
pub struct UnionFind(Vec<usize>);

impl UnionFind {
    pub fn new(n: usize) -> UnionFind {
        UnionFind((0..n).collect())
    }

    pub fn find(&mut self, mut x: usize) -> usize {
        while self.0[x] != x {
            self.0[x] = self.0[self.0[x]];
            x = self.0[x];
        }
        x
    }

    /// joins the sets of `a` and `b`; the smaller root becomes the root
    pub fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.0[a.max(b)] = a.min(b);
        }
    }

    /// joins the sets of `a` and `b`; the root of `b` becomes the root (even if `a` and `b` are already joined, a no-op)
    pub fn link(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        self.0[a] = b;
    }
}

/// `x` rounded to `n` decimals on its exact binary value (ties to even), as a float again. Keeps the short decimals in the JSON side files.
pub fn round(x: f64, n: usize) -> f64 {
    format!("{:.*}", n, x).parse().unwrap()
}

#[cfg(test)]
mod tests {
    use super::round;

    #[test]
    fn rounds_on_the_binary_value() {
        assert_eq!(round(2.675, 2), 2.67);
        assert_eq!(round(0.125, 2), 0.12);
    }
}
