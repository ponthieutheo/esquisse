//! File › Import › DEM: USGS digital elevation models, as ASCII .dem files or SDTS raster
//! transfers (.ddf), turned into a triangulated terrain (a TIN) by greedy insertion: starting
//! from the four corners, the grid point farthest from the surface is added and the mesh
//! re-triangulated (Delaunay) until the number of points asked for in the options is
//! reached. Optionally the terrain gets a grey gradient texture of its elevations
//! (material "DEM-Default").

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use esq_core::glam::{DVec2, DVec3};
use esq_core::model::Model;

use crate::build::{BuildOpts, MatIn, MeshIn, PolyIn, SceneIn};
use crate::iso8211::{self, Value};
use crate::options::DemImport;

const NOT_SUPPORTED: &str = "Format de fichier non pris en charge";
const INSUFFICIENT: &str = "Données insuffisantes";

/// Elevations below this are "no data" (-32767 void, -32766 fill, -32000 in some old files).
const VOID: f64 = -32000.0;

/// Inches per unit of the DEM unit codes (0 radians, 1 feet, 2 metres, 3 arc-seconds), as
/// SketchUp converts them: a metre is 39.37", an arc-second 30 m, a radian 200 000".
fn inches_per(unit: i32) -> f64 {
    match unit {
        0 => 200_000.0 * 30.0 * 39.37,
        1 => 12.0,
        2 => 39.37,
        3 => 30.0 * 39.37,
        _ => 1.0,
    }
}

fn unit_name(unit: i32) -> &'static str {
    match unit {
        0 => "Radians",
        1 => "Pieds",
        2 => "Mètres",
        3 => "Arcsecondes",
        _ => "N/D",
    }
}

/// An elevation grid: columns west to east, rows south to north.
#[derive(Clone, Debug, Default)]
pub struct Grid {
    pub title: String,
    pub width: usize,
    pub height: usize,
    /// Elevations in z-resolution units, row after row; NaN where there is no data.
    pub z: Vec<f64>,
    /// Units of the ground coordinates and of the elevations (0 radians, 1 feet, 2 metres,
    /// 3 arc-seconds).
    pub ground_unit: i32,
    pub elev_unit: i32,
    /// Spacing of the columns, of the rows, and the unit of the elevation values.
    pub res: [f64; 3],
    /// Lowest and highest elevation (elevation units).
    pub min: f64,
    pub max: f64,
}

impl Grid {
    #[cfg(test)]
    fn at(&self, x: usize, y: usize) -> f64 {
        self.z[y * self.width + x]
    }

    pub fn missing(&self) -> usize {
        self.z.iter().filter(|v| v.is_nan()).count()
    }

    /// Lowest and highest value present (z-resolution units).
    fn range(&self) -> Option<(f64, f64)> {
        let mut it = self.z.iter().copied().filter(|v| !v.is_nan());
        let first = it.next()?;
        Some(it.fold((first, first), |(lo, hi), v| (lo.min(v), hi.max(v))))
    }

    /// Sets the elevation range from the data when the file gave none.
    fn finish(&mut self) {
        if let Some((lo, hi)) = self.range()
            && self.min == 0.0
            && self.max == 0.0
        {
            self.min = lo * self.res[2];
            self.max = hi * self.res[2];
        }
    }
}

// ------------------------------------------------------------------------------------------
// USGS DEM (ASCII)

/// A reader of the numbers of a DEM, which may touch ("-32767-32767") and use Fortran
/// exponents ("0.25D+03").
struct Cursor<'a> {
    b: &'a [u8],
    pos: usize,
}

impl Cursor<'_> {
    fn skip_space(&mut self) {
        while self.pos < self.b.len() && (self.b[self.pos].is_ascii_whitespace() || self.b[self.pos] == 0) {
            self.pos += 1;
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.pos;
        while self.pos < self.b.len() && self.b[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
        self.pos - start
    }

    fn int(&mut self) -> Option<i64> {
        self.skip_space();
        let start = self.pos;
        if matches!(self.b.get(self.pos), Some(b'-' | b'+')) {
            self.pos += 1;
        }
        if self.digits() == 0 {
            self.pos = start;
            return None;
        }
        std::str::from_utf8(&self.b[start..self.pos]).ok()?.parse().ok()
    }

    fn num(&mut self) -> Option<f64> {
        self.skip_space();
        let start = self.pos;
        if matches!(self.b.get(self.pos), Some(b'-' | b'+')) {
            self.pos += 1;
        }
        let mut n = self.digits();
        if self.b.get(self.pos) == Some(&b'.') {
            self.pos += 1;
            n += self.digits();
        }
        if n == 0 {
            self.pos = start;
            return None;
        }
        if matches!(self.b.get(self.pos), Some(b'D' | b'd' | b'E' | b'e')) {
            let save = self.pos;
            self.pos += 1;
            if matches!(self.b.get(self.pos), Some(b'-' | b'+')) {
                self.pos += 1;
            }
            if self.digits() == 0 {
                self.pos = save;
            }
        }
        let t = String::from_utf8_lossy(&self.b[start..self.pos]).replace(['D', 'd'], "E");
        t.parse().ok()
    }
}

/// A fixed-width field of the A record.
fn fixed(b: &[u8], start: usize, len: usize) -> String {
    let end = (start + len).min(b.len());
    String::from_utf8_lossy(b.get(start..end).unwrap_or(&[])).trim().to_string()
}

fn fixed_int(b: &[u8], start: usize, len: usize) -> Option<i64> {
    fixed(b, start, len).parse().ok()
}

fn fixed_num(b: &[u8], start: usize, len: usize) -> Option<f64> {
    fixed(b, start, len).replace(['D', 'd'], "E").parse().ok()
}

/// Where the B records (profiles) begin: 1024 in the standard layout, 864 in the oldest
/// files, 893 or 918 in some undocumented variants.
fn data_start(b: &[u8]) -> Option<usize> {
    let mut c = Cursor { b, pos: 864 };
    let (r, k) = (c.int(), c.int());
    if c.pos < 1024 && r == Some(1) && k == Some(1) {
        return Some(864);
    }
    [1024, 893, 918].into_iter().find(|&start| {
        let mut c = Cursor { b, pos: start };
        c.int() == Some(1) && matches!(c.int(), Some(0 | 1))
    })
}

struct Profile {
    col: i64,
    x0: f64,
    y0: f64,
    /// Elevation of the local datum, added to the values.
    datum: f64,
    values: Vec<f64>,
}

/// The profile whose header starts at `pos`, if it is one.
fn profile_at(b: &[u8], pos: usize, expect: i64) -> Option<(Profile, usize)> {
    let mut c = Cursor { b, pos };
    let (row, col, m, n) = (c.int()?, c.int()?, c.int()?, c.int()?);
    if row < 1 || n != 1 || !(1..=1_000_000).contains(&m) || !(col == expect || col == expect + 1) {
        return None;
    }
    let (x0, y0, datum) = (c.num()?, c.num()?, c.num()?);
    // Lowest and highest elevation of the profile.
    c.num()?;
    c.num()?;
    let mut values = Vec::with_capacity(m as usize);
    for _ in 0..m {
        let v = c.int().map_or(f64::NAN, |v| v as f64);
        values.push(if v <= VOID { f64::NAN } else { v });
    }
    Some((Profile { col, x0, y0, datum, values }, c.pos))
}

/// Reads a USGS DEM (ASCII).
pub fn read_usgs(b: &[u8]) -> Result<Grid> {
    if b.len() < 864 {
        bail!(INSUFFICIENT);
    }
    let res = [fixed_num(b, 816, 12), fixed_num(b, 828, 12), fixed_num(b, 840, 12)];
    let (Some(rx), Some(ry), Some(rz)) = (res[0], res[1], res[2]) else { bail!(NOT_SUPPORTED) };
    let columns = fixed_int(b, 858, 6).unwrap_or(0);
    let Some(start) = data_start(b) else { bail!(NOT_SUPPORTED) };
    if columns < 1 || rx <= 0.0 || ry <= 0.0 {
        bail!(INSUFFICIENT);
    }
    // The profiles: each starts on a new 1024-byte record (1025 or 1026 with line ends), or
    // right after the previous one in loosely written files.
    let mut profiles: Vec<Profile> = Vec::new();
    let mut pos = start;
    for k in 0..columns {
        let expect = profiles.last().map_or(k.min(1), |p| p.col + 1);
        let mut candidates = Vec::new();
        if k == 0 {
            candidates.push(pos);
        } else {
            for rec in [1024, 1025, 1026] {
                let off = pos.saturating_sub(start);
                candidates.push(start + off.div_ceil(rec) * rec);
            }
            candidates.push(pos);
        }
        let found = candidates.into_iter().find_map(|at| profile_at(b, at, expect).or_else(|| profile_at(b, at, k)));
        let Some((p, next)) = found else { break };
        pos = next;
        profiles.push(p);
    }
    if profiles.is_empty() {
        bail!(INSUFFICIENT);
    }
    // Placed on one grid by their first point.
    let xmin = profiles.iter().map(|p| p.x0).fold(f64::INFINITY, f64::min);
    let ymin = profiles.iter().map(|p| p.y0).fold(f64::INFINITY, f64::min);
    let place = |p: &Profile| ((p.x0 - xmin) / rx).round().max(0.0) as usize;
    let row0 = |p: &Profile| ((p.y0 - ymin) / ry).round().max(0.0) as usize;
    let width = profiles.iter().map(|p| place(p) + 1).max().unwrap_or(0).max(profiles.len());
    let height = profiles.iter().map(|p| row0(p) + p.values.len()).max().unwrap_or(0);
    if width < 2 || height < 2 {
        bail!(INSUFFICIENT);
    }
    if width.saturating_mul(height) > 100_000_000 {
        bail!("{} rangées dépassent la limite du programme", height);
    }
    // Columns placed by their x unless the spacing does not fit, then one after the other.
    let by_x = profiles.iter().map(&place).collect::<std::collections::HashSet<_>>().len() == profiles.len();
    let rz = if rz > 0.0 { rz } else { 1.0 };
    let mut z = vec![f64::NAN; width * height];
    for (i, p) in profiles.iter().enumerate() {
        let x = if by_x { place(p) } else { i };
        for (k, v) in p.values.iter().enumerate() {
            let y = row0(p) + k;
            if x < width && y < height {
                z[y * width + x] = v + p.datum / rz;
            }
        }
    }
    let mut g = Grid {
        title: fixed(b, 0, 40),
        width,
        height,
        z,
        ground_unit: fixed_int(b, 528, 6).unwrap_or(2) as i32,
        elev_unit: fixed_int(b, 534, 6).unwrap_or(2) as i32,
        res: [rx, ry, rz],
        min: fixed_num(b, 738, 24).unwrap_or(0.0),
        max: fixed_num(b, 762, 24).unwrap_or(0.0),
    };
    g.finish();
    Ok(g)
}

// ------------------------------------------------------------------------------------------
// SDTS raster transfers

/// The modules of an SDTS transfer, from its catalog (any .ddf of the transfer may be
/// chosen: the catalog is found beside it).
struct Transfer {
    dir: PathBuf,
    files: Vec<(String, String)>,
}

/// A file of the directory whose name matches `name` regardless of case.
fn beside(dir: &Path, name: &str) -> Option<PathBuf> {
    let name = crate::tex::safe_name(name)?;
    let direct = dir.join(&name);
    if direct.is_file() {
        return Some(direct);
    }
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.eq_ignore_ascii_case(&name)))
}

impl Transfer {
    fn open(path: &Path) -> Result<Transfer> {
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_uppercase();
        let catd = if name.ends_with("CATD.DDF") {
            path.to_path_buf()
        } else {
            let prefix: String = name.chars().take(4).collect();
            let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)?
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.to_ascii_uppercase().ends_with("CATD.DDF"))
                })
                .collect();
            found.sort_by_key(|p| {
                !p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_uppercase().starts_with(&prefix)
            });
            found.into_iter().next().context("Fichier introuvable")?
        };
        let cat = iso8211::read(&std::fs::read(&catd).context("Fichier introuvable")?)?;
        let files = cat
            .records
            .iter()
            .filter_map(|r| Some((r.get("NAME")?.as_str().to_string(), r.get("FILE")?.as_str().to_string())))
            .collect();
        Ok(Transfer { dir, files })
    }

    fn module(&self, name: &str) -> Result<iso8211::Module> {
        let file = self.files.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, f)| f.clone());
        let path = file.and_then(|f| beside(&self.dir, &f)).with_context(|| format!("module {name} introuvable"))?;
        iso8211::read(&std::fs::read(path)?)
    }
}

fn first_f64(m: &iso8211::Module, label: &str) -> Option<f64> {
    m.records.iter().find_map(|r| r.get(label)?.as_f64())
}

fn first_str(m: &iso8211::Module, label: &str) -> Option<String> {
    m.records.iter().find_map(|r| Some(r.get(label)?.as_str().trim().to_string())).filter(|s| !s.is_empty())
}

/// Reads an SDTS raster DEM transfer.
pub fn read_sdts(path: &Path) -> Result<Grid> {
    let t = Transfer::open(path)?;
    let iref = t.module("IREF")?;
    let ldef = t.module("LDEF")?;
    let title = t.module("IDEN").ok().and_then(|m| first_str(&m, "TITL")).unwrap_or_default();
    let geo = t.module("XREF").ok().and_then(|m| first_str(&m, "RSNM")).is_some_and(|s| s.eq_ignore_ascii_case("GEO"));
    let ddsh = t.module("DDSH").ok();
    let unit = ddsh.as_ref().and_then(|m| first_str(m, "UNIT")).unwrap_or_else(|| "METERS".into()).to_ascii_uppercase();
    let fmt = ddsh.as_ref().and_then(|m| first_str(m, "FMT")).unwrap_or_else(|| "BI16".into()).to_ascii_uppercase();
    let top_down = t
        .module("RSDF")
        .ok()
        .and_then(|m| first_str(&m, "SCOR"))
        .is_none_or(|s| s.to_ascii_uppercase().starts_with('T'));
    let width = first_f64(&ldef, "NCOL").context(INSUFFICIENT)? as usize;
    let height = first_f64(&ldef, "NROW").context(INSUFFICIENT)? as usize;
    let cells = first_str(&ldef, "CMNM").unwrap_or_else(|| "CEL0".into());
    if width < 2 || height < 2 || width.saturating_mul(height) > 100_000_000 {
        bail!(INSUFFICIENT);
    }
    let mut z = vec![f64::NAN; width * height];
    let cel = t.module(&cells)?;
    for r in &cel.records {
        let (Some(row), Some(col)) = (r.get("ROWI").and_then(Value::as_f64), r.get("COLI").and_then(Value::as_f64))
        else {
            continue;
        };
        let (row, col) = (row as usize, col as usize);
        if row < 1 || row > height || col < 1 {
            continue;
        }
        let y = if top_down { height - row } else { row - 1 };
        let Some(vals) = r.fields.iter().find(|f| f.tag == "CVLS") else { continue };
        for (k, (_, v)) in vals.values.iter().enumerate() {
            let x = col - 1 + k;
            if x >= width {
                break;
            }
            let e = match v {
                Value::Bytes(b) if fmt == "BFP32" && b.len() == 4 => {
                    f32::from_be_bytes([b[0], b[1], b[2], b[3]]) as f64
                }
                Value::Bytes(b) if b.len() == 4 => i32::from_be_bytes([b[0], b[1], b[2], b[3]]) as f64,
                Value::Bytes(b) if b.len() == 2 => i16::from_be_bytes([b[0], b[1]]) as f64,
                other => other.as_f64().unwrap_or(f64::NAN),
            };
            z[y * width + x] = if e <= VOID { f64::NAN } else { e };
        }
    }
    let (elev_unit, rz) = match unit.as_str() {
        u if u.starts_with("FEET") || u.starts_with("FOOT") => (1, 1.0),
        u if u.starts_with("DECIMETER") => (2, 0.1),
        u if u.starts_with("CENTIMETER") => (2, 0.01),
        _ => (2, 1.0),
    };
    let mut g = Grid {
        title,
        width,
        height,
        z,
        ground_unit: if geo { 3 } else { 2 },
        elev_unit,
        res: [first_f64(&iref, "XHRS").unwrap_or(30.0), first_f64(&iref, "YHRS").unwrap_or(30.0), rz],
        min: 0.0,
        max: 0.0,
    };
    g.finish();
    Ok(g)
}

// ------------------------------------------------------------------------------------------
// Greedy insertion

const NONE: u32 = u32::MAX;

#[derive(Clone, Debug)]
struct Tri {
    v: [u32; 3],
    /// Neighbour across edge i (from v[i] to v[i+1]).
    adj: [u32; 3],
    stamp: u32,
    cand: Option<[i64; 2]>,
}

struct Cand {
    err: f64,
    tri: u32,
    stamp: u32,
}

impl PartialEq for Cand {
    fn eq(&self, o: &Self) -> bool {
        self.cmp(o) == Ordering::Equal
    }
}
impl Eq for Cand {}
impl PartialOrd for Cand {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Cand {
    fn cmp(&self, o: &Self) -> Ordering {
        self.err.total_cmp(&o.err).then(o.tri.cmp(&self.tri))
    }
}

fn orient(a: [i64; 2], b: [i64; 2], c: [i64; 2]) -> i128 {
    (b[0] - a[0]) as i128 * (c[1] - a[1]) as i128 - (b[1] - a[1]) as i128 * (c[0] - a[0]) as i128
}

/// Positive when `d` lies inside the circle through the counter-clockwise `a`, `b`, `c`.
fn in_circle(a: [i64; 2], b: [i64; 2], c: [i64; 2], d: [i64; 2]) -> i128 {
    let row = |p: [i64; 2]| {
        let (x, y) = ((p[0] - d[0]) as i128, (p[1] - d[1]) as i128);
        (x, y, x * x + y * y)
    };
    let (ax, ay, a2) = row(a);
    let (bx, by, b2) = row(b);
    let (cx, cy, c2) = row(c);
    ax * (by * c2 - b2 * cy) - ay * (bx * c2 - b2 * cx) + a2 * (bx * cy - by * cx)
}

/// A terrain approximated by greedy insertion of its grid points (Garland & Heckbert, 1995).
pub struct Tin<'a> {
    width: usize,
    z: &'a [f64],
    pub pts: Vec<[i64; 2]>,
    tris: Vec<Tri>,
    used: Vec<bool>,
    heap: BinaryHeap<Cand>,
}

impl<'a> Tin<'a> {
    /// The two triangles of the grid's corners; `z` has no missing values.
    pub fn new(width: usize, height: usize, z: &'a [f64]) -> Tin<'a> {
        let (w, h) = (width as i64 - 1, height as i64 - 1);
        let mut t = Tin {
            width,
            z,
            pts: vec![[0, 0], [w, 0], [w, h], [0, h]],
            tris: vec![
                Tri { v: [0, 1, 2], adj: [NONE, NONE, 1], stamp: 0, cand: None },
                Tri { v: [0, 2, 3], adj: [0, NONE, NONE], stamp: 0, cand: None },
            ],
            used: vec![false; width * height],
            heap: BinaryHeap::new(),
        };
        for p in [[0, 0], [w, 0], [w, h], [0, h]] {
            let i = t.index(p);
            t.used[i] = true;
        }
        t.scan(0);
        t.scan(1);
        t
    }

    fn index(&self, p: [i64; 2]) -> usize {
        p[1] as usize * self.width + p[0] as usize
    }

    fn height_at(&self, p: [i64; 2]) -> f64 {
        self.z[self.index(p)]
    }

    pub fn triangles(&self) -> impl Iterator<Item = [usize; 3]> + '_ {
        self.tris.iter().map(|t| t.v.map(|v| v as usize))
    }

    /// The grid point of a triangle farthest from its plane, queued by that distance.
    fn scan(&mut self, t: u32) {
        let tri = &self.tris[t as usize];
        let [a, b, c] = tri.v.map(|v| self.pts[v as usize]);
        let area = orient(a, b, c) as f64;
        let mut best: Option<(f64, [i64; 2])> = None;
        if area > 0.0 {
            let (za, zb, zc) = (self.height_at(a), self.height_at(b), self.height_at(c));
            let (x0, x1) = (a[0].min(b[0]).min(c[0]), a[0].max(b[0]).max(c[0]));
            let (y0, y1) = (a[1].min(b[1]).min(c[1]), a[1].max(b[1]).max(c[1]));
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let p = [x, y];
                    let (wa, wb, wc) = (orient(b, c, p), orient(c, a, p), orient(a, b, p));
                    if wa < 0 || wb < 0 || wc < 0 || self.used[self.index(p)] {
                        continue;
                    }
                    let plane = (wa as f64 * za + wb as f64 * zb + wc as f64 * zc) / area;
                    let err = (self.height_at(p) - plane).abs();
                    if best.is_none_or(|(e, _)| err > e) {
                        best = Some((err, p));
                    }
                }
            }
        }
        let tri = &mut self.tris[t as usize];
        tri.stamp += 1;
        tri.cand = None;
        // Points closer than this to the surface are not worth adding.
        if let Some((err, p)) = best.filter(|b| b.0 >= 1e-4) {
            tri.cand = Some(p);
            self.heap.push(Cand { err, tri: t, stamp: tri.stamp });
        }
    }

    fn replace_adj(&mut self, t: u32, old: u32, new: u32) {
        if t == NONE {
            return;
        }
        for a in self.tris[t as usize].adj.iter_mut() {
            if *a == old {
                *a = new;
            }
        }
    }

    fn push_tri(&mut self, v: [u32; 3], adj: [u32; 3]) -> u32 {
        self.tris.push(Tri { v, adj, stamp: 0, cand: None });
        self.tris.len() as u32 - 1
    }

    /// Adds the most important point; false when no point is worth adding.
    pub fn insert_next(&mut self) -> bool {
        while let Some(c) = self.heap.pop() {
            let tri = &self.tris[c.tri as usize];
            if tri.stamp != c.stamp {
                continue;
            }
            let Some(p) = tri.cand else { continue };
            self.insert(c.tri, p);
            return true;
        }
        false
    }

    fn insert(&mut self, t: u32, p: [i64; 2]) {
        let pi = self.pts.len() as u32;
        self.pts.push(p);
        let ui = self.index(p);
        self.used[ui] = true;
        let tv = self.tris[t as usize].v;
        let [a, b, c] = tv.map(|v| self.pts[v as usize]);
        let o = [orient(a, b, p), orient(b, c, p), orient(c, a, p)];
        let mut touched = vec![t];
        let mut legal: Vec<(u32, usize)> = Vec::new();
        match o.iter().position(|&x| x == 0) {
            None => {
                // Inside: three triangles around the point.
                let [va, vb, vc] = tv;
                let [nab, nbc, nca] = self.tris[t as usize].adj;
                let t1 = self.push_tri([vb, vc, pi], [nbc, NONE, t]);
                let t2 = self.push_tri([vc, va, pi], [nca, t, t1]);
                self.tris[t1 as usize].adj[1] = t2;
                self.tris[t as usize].v = [va, vb, pi];
                self.tris[t as usize].adj = [nab, t1, t2];
                self.replace_adj(nbc, t, t1);
                self.replace_adj(nca, t, t2);
                touched.extend([t1, t2]);
                legal.extend([(t, 0), (t1, 0), (t2, 0)]);
            }
            Some(e) => {
                // On an edge: both triangles sharing it are split in two.
                let rot = |k: usize| (e + k) % 3;
                let (va, vb, vc) = (tv[rot(0)], tv[rot(1)], tv[rot(2)]);
                let adj = self.tris[t as usize].adj;
                let (n, nbc, nca) = (adj[rot(0)], adj[rot(1)], adj[rot(2)]);
                // t → (a, p, c) and t1 = (p, b, c).
                let t1 = self.push_tri([pi, vb, vc], [NONE, nbc, t]);
                self.tris[t as usize].v = [va, pi, vc];
                self.tris[t as usize].adj = [NONE, t1, nca];
                self.replace_adj(nbc, t, t1);
                touched.push(t1);
                legal.extend([(t, 2), (t1, 1)]);
                if n != NONE {
                    let nv = self.tris[n as usize].v;
                    let j = (0..3).find(|&j| nv[j] == vb && nv[(j + 1) % 3] == va).unwrap_or(0);
                    let vd = nv[(j + 2) % 3];
                    let nadj = self.tris[n as usize].adj;
                    let (n_ad, n_db) = (nadj[(j + 1) % 3], nadj[(j + 2) % 3]);
                    // n → (b, p, d) and n1 = (p, a, d).
                    let n1 = self.push_tri([pi, va, vd], [t, n_ad, n]);
                    self.tris[n as usize].v = [vb, pi, vd];
                    self.tris[n as usize].adj = [t1, n1, n_db];
                    self.replace_adj(n_ad, n, n1);
                    self.tris[t as usize].adj[0] = n1;
                    self.tris[t1 as usize].adj[0] = n;
                    touched.extend([n, n1]);
                    legal.extend([(n, 2), (n1, 1)]);
                }
            }
        }
        // Delaunay: flip the edges facing the new point while the neighbour's far vertex
        // lies in their circle.
        while let Some((t, e)) = legal.pop() {
            let tri = self.tris[t as usize].clone();
            let n = tri.adj[e];
            if n == NONE {
                continue;
            }
            let (vu, vv, vp) = (tri.v[e], tri.v[(e + 1) % 3], tri.v[(e + 2) % 3]);
            let nv = self.tris[n as usize].v;
            let Some(j) = (0..3).find(|&j| nv[j] == vv && nv[(j + 1) % 3] == vu) else { continue };
            let vd = nv[(j + 2) % 3];
            let [u, v, pp, d] = [vu, vv, vp, vd].map(|i| self.pts[i as usize]);
            if in_circle(u, v, pp, d) <= 0 {
                continue;
            }
            let (t_vp, t_pu) = (tri.adj[(e + 1) % 3], tri.adj[(e + 2) % 3]);
            let nadj = self.tris[n as usize].adj;
            let (n_ud, n_dv) = (nadj[(j + 1) % 3], nadj[(j + 2) % 3]);
            self.tris[t as usize].v = [vp, vu, vd];
            self.tris[t as usize].adj = [t_pu, n_ud, n];
            self.tris[n as usize].v = [vp, vd, vv];
            self.tris[n as usize].adj = [t, n_dv, t_vp];
            self.replace_adj(n_ud, n, t);
            self.replace_adj(t_vp, t, n);
            touched.extend([t, n]);
            legal.extend([(t, 1), (n, 1)]);
        }
        touched.sort_unstable();
        touched.dedup();
        for t in touched {
            self.scan(t);
        }
    }
}

// ------------------------------------------------------------------------------------------
// Import

/// Reads a DEM file (.dem, or a .ddf module of an SDTS transfer).
pub fn read(path: &Path) -> Result<Grid> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if ext == "ddf" { read_sdts(path) } else { read_usgs(&std::fs::read(path).context("Fichier introuvable")?) }
}

/// The "Résultats de l'importation" of a DEM.
fn report(g: &Grid, faces: usize, points: usize) -> String {
    let mut r = String::new();
    let cells = (g.width * g.height).max(1);
    let missing = g.missing();
    let vunit = unit_name(g.elev_unit);
    let _ = writeln!(r, "Titre : {}", g.title);
    let _ = writeln!(r, "Dimensions : {}x{}", g.height, g.width);
    let _ = writeln!(r, "Manque : {missing} ({:.0}%)", missing as f64 * 100.0 / cells as f64);
    let _ = writeln!(r, "Élévation minimum/maximum : {:.2}/{:.2} ({vunit})", g.min, g.max);
    let _ = writeln!(r, "Résolution spatiale X/Y/Z : {:.2}/{:.2}/{:.2} ({vunit})", g.res[0], g.res[1], g.res[2]);
    let _ = writeln!(r, "Faces générées par TIN : \t{faces}");
    let _ = writeln!(r, "Points générés par TIN : \t{points}");
    r
}

/// The grey gradient of the elevations (lowest black, highest white), north at the top.
fn gradient(g: &Grid, z: &[f64]) -> Vec<u8> {
    let (lo, hi) = g.range().unwrap_or((0.0, 1.0));
    let span = if hi > lo { hi - lo } else { 1.0 };
    let mut img = image::GrayImage::new(g.width as u32, g.height as u32);
    for (x, y, px) in img.enumerate_pixels_mut() {
        let v = z[(g.height - 1 - y as usize) * g.width + x as usize];
        px.0 = [(((v - lo) / span).clamp(0.0, 1.0) * 255.0) as u8];
    }
    let mut png = Vec::new();
    let _ = image::DynamicImage::ImageLuma8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png);
    png
}

/// The terrain of a grid, as a model (millimetres), and the import report.
pub fn terrain(g: &Grid, o: &DemImport) -> Result<(Model, String)> {
    if g.width < 2 || g.height < 2 {
        bail!(INSUFFICIENT);
    }
    // Holes in the data take the lowest elevation.
    let (lo, _) = g.range().context(INSUFFICIENT)?;
    let z: Vec<f64> = g.z.iter().map(|v| if v.is_nan() { lo } else { *v }).collect();
    let points = (o.max_points as usize).clamp(4, g.width * g.height);
    let mut tin = Tin::new(g.width, g.height, &z);
    while tin.pts.len() < points && tin.insert_next() {}
    let mm = 25.4;
    let s = DVec3::new(
        g.res[0] * inches_per(g.ground_unit) * mm,
        g.res[1] * inches_per(g.ground_unit) * mm,
        g.res[2] * inches_per(g.elev_unit) * mm,
    );
    let pos =
        tin.pts.iter().map(|p| DVec3::new(p[0] as f64, p[1] as f64, z[p[1] as usize * g.width + p[0] as usize]) * s);
    let mut mesh = MeshIn { pos: pos.collect(), ..Default::default() };
    let mut scene = SceneIn::default();
    let mat = if o.gradient_texture {
        let texture = crate::tex::texture_from_bytes("DEM-Default.png", gradient(g, &z), None);
        scene.materials.push(MatIn { name: "DEM-Default".into(), color: [128, 128, 128], opacity: 1.0, texture });
        Some(0)
    } else {
        None
    };
    let (w, h) = (g.width as f64, g.height as f64);
    for t in tin.triangles() {
        let uv = if mat.is_some() {
            t.iter().map(|&i| DVec2::new(tin.pts[i][0] as f64 / w, tin.pts[i][1] as f64 / h)).collect()
        } else {
            Vec::new()
        };
        mesh.polys.push(PolyIn { outer: t.map(|i| i as u32).to_vec(), mat, uv, ..Default::default() });
    }
    let faces = mesh.polys.len();
    let mi = scene.add_mesh(mesh);
    scene.root.meshes.push(mi);
    let model = crate::build::build(&scene, BuildOpts::default());
    Ok((model, report(g, faces, points)))
}

/// File › Import of a DEM.
pub fn import(path: &Path, o: &DemImport) -> Result<(Model, String)> {
    let g = read(path).map_err(|e| anyhow::anyhow!("Erreurs DEM:\n{}: {e:#}", path.display()))?;
    terrain(&g, o)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A USGS DEM in metres, 30 m apart, with profiles of different lengths and a void.
    fn usgs(columns: &[(f64, Vec<i32>)]) -> Vec<u8> {
        let mut a = vec![b' '; 1024];
        let put = |a: &mut Vec<u8>, at: usize, s: &str| a[at..at + s.len()].copy_from_slice(s.as_bytes());
        put(&mut a, 0, "COLLINE D'ESSAI");
        put(&mut a, 144, &format!("{:>6}{:>6}{:>6}{:>6}", 1, 1, 1, 31));
        put(&mut a, 528, &format!("{:>6}{:>6}{:>6}", 2, 2, 4));
        put(&mut a, 738, &format!("{:>24}{:>24}", "0.100000000000000D+03", "0.400000000000000D+03"));
        put(&mut a, 810, &format!("{:>6}", 0));
        put(&mut a, 816, "0.300000E+020.300000E+020.100000E+01");
        put(&mut a, 852, &format!("{:>6}{:>6}", 1, columns.len()));
        for (k, (y0, vals)) in columns.iter().enumerate() {
            let mut rec = format!(
                "{:>6}{:>6}{:>6}{:>6}{:>24}{:>24}{:>24}{:>24}{:>24}",
                1,
                k + 1,
                vals.len(),
                1,
                format!("{:.15E}", 600000.0 + 30.0 * k as f64).replace('E', "D+0"),
                format!("{y0:.1}"),
                "0.0",
                "0.0",
                "0.0"
            );
            for v in vals {
                rec.push_str(&format!("{v:>6}"));
            }
            let mut r = rec.into_bytes();
            r.resize(r.len().div_ceil(1024) * 1024, b' ');
            a.extend(r);
        }
        a
    }

    #[test]
    fn reads_usgs_profiles_into_a_grid() {
        let b = usgs(&[
            (4_000_000.0, vec![100, 110, 120]),
            (4_000_030.0, vec![200, -32767]),
            (4_000_000.0, vec![300, 310, 400]),
        ]);
        let g = read_usgs(&b).unwrap();
        assert_eq!((g.width, g.height), (3, 3));
        assert_eq!(g.title, "COLLINE D'ESSAI");
        assert_eq!((g.ground_unit, g.elev_unit), (2, 2));
        assert_eq!(g.res, [30.0, 30.0, 1.0]);
        assert_eq!((g.min, g.max), (100.0, 400.0));
        assert_eq!(g.at(0, 2), 120.0);
        // The second profile starts one row up; its void and the row below it are missing.
        assert_eq!(g.at(1, 1), 200.0);
        assert!(g.at(1, 0).is_nan() && g.at(1, 2).is_nan());
        assert_eq!(g.missing(), 2);
        // Numbers that touch.
        let mut c = Cursor { b: b"-32767-32767 0.5D+02-1", pos: 0 };
        assert_eq!((c.int(), c.int(), c.num(), c.int()), (Some(-32767), Some(-32767), Some(50.0), Some(-1)));
    }

    /// A 33 × 33 grid: a cone 100 high in the middle.
    fn cone() -> Grid {
        let n = 33;
        let z = (0..n * n)
            .map(|i| {
                let (x, y) = ((i % n) as f64 - 16.0, (i / n) as f64 - 16.0);
                (100.0 - 6.0 * (x * x + y * y).sqrt()).max(0.0)
            })
            .collect();
        Grid {
            title: "Cône".into(),
            width: n,
            height: n,
            z,
            ground_unit: 2,
            elev_unit: 2,
            res: [10.0, 10.0, 1.0],
            min: 0.0,
            max: 100.0,
        }
    }

    #[test]
    fn greedy_insertion_keeps_a_delaunay_triangulation() {
        let g = cone();
        let mut tin = Tin::new(g.width, g.height, &g.z);
        while tin.pts.len() < 120 && tin.insert_next() {}
        assert_eq!(tin.pts.len(), 120);
        // The summit goes in first.
        assert_eq!(tin.pts[4], [16, 16]);
        let tris: Vec<[usize; 3]> = tin.triangles().collect();
        // Counter-clockwise, covering the whole grid, Delaunay.
        let area: i128 = tris.iter().map(|t| orient(tin.pts[t[0]], tin.pts[t[1]], tin.pts[t[2]])).sum();
        assert_eq!(area, 2 * 32 * 32);
        for t in &tris {
            let [a, b, c] = t.map(|i| tin.pts[i]);
            assert!(orient(a, b, c) > 0);
            for p in &tin.pts {
                assert!(in_circle(a, b, c, *p) <= 0, "{t:?} {p:?}");
            }
        }
        // Euler: 2 n − h − 2 faces for n points with h on the hull.
        let hull = tin.pts.iter().filter(|p| p[0] == 0 || p[1] == 0 || p[0] == 32 || p[1] == 32).count();
        assert_eq!(tris.len(), 2 * tin.pts.len() - hull - 2);
    }

    #[test]
    fn terrain_with_gradient_texture_and_report() {
        let g = cone();
        let o = DemImport { max_points: 60, gradient_texture: true };
        let (m, report) = terrain(&g, &o).unwrap();
        let faces = m.root.faces.len();
        assert!(report.contains("Points générés par TIN : \t60"), "{report}");
        assert!(report.contains(&format!("Faces générées par TIN : \t{faces}")), "{report}");
        assert!(report.contains("Dimensions : 33x33") && report.contains("(Mètres)"), "{report}");
        // 320 m wide (10 m apart, metres of 39.37"), the summit 100 m up.
        let (lo, hi) = crate::build::bounds(&m).unwrap();
        let metre = 39.37 * 25.4;
        assert!(((hi.x - lo.x) - 320.0 * metre).abs() < 1e-6, "{lo} {hi}");
        assert!((hi.z - 100.0 * metre).abs() < 1e-6);
        // Textured with the gradient, every face.
        let mat = m.materials.values().next().unwrap();
        assert_eq!(mat.name, "DEM-Default");
        let t = mat.ext.texture.as_ref().unwrap();
        assert_eq!(t.px, [33, 33]);
        assert!(m.root.faces.values().all(|f| f.front.is_some() && f.tex.is_some()));
        // Without the option, no material.
        let (m, _) = terrain(&g, &DemImport::default()).unwrap();
        assert!(m.materials.is_empty() && m.root.faces.values().all(|f| f.front.is_none()));
    }

    #[test]
    fn reads_an_sdts_transfer() {
        use crate::iso8211::write_module;
        let dir = std::env::temp_dir().join(format!("esq_sdts_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let text = |parts: &[&str]| {
            let mut v = parts.join("\x1f").into_bytes();
            v.push(0x1e);
            v
        };
        let cat: Vec<Vec<(&str, Vec<u8>)>> = ["IDEN", "IREF", "XREF", "LDEF", "DDSH", "RSDF", "CEL0"]
            .iter()
            .map(|n| vec![("CATD", text(&["CATD", "1", n, "x", &format!("TESTt{n}.DDF")]))])
            .collect();
        let write = |name: &str, b: Vec<u8>| std::fs::write(dir.join(format!("TESTt{name}.DDF")), b).unwrap();
        write(
            "CATD",
            write_module(&[("CATD", "Catalog/Directory", "MODN!RCID!NAME!TYPE!FILE", "(A,I,3A)")], &cat, false),
        );
        let one = |tag: &'static str, labels: &'static str, fmt: &'static str, vals: &[&str]| {
            write_module(&[(tag, "x", labels, fmt)], &[vec![(tag, text(vals))]], false)
        };
        write("IDEN", one("IDEN", "MODN!RCID!TITL", "(A,I,A)", &["IDEN", "1", "MONT TEST"]));
        write("IREF", one("IREF", "MODN!RCID!XHRS!YHRS", "(A,I,2R)", &["IREF", "1", "30.0", "30.0"]));
        write("XREF", one("XREF", "MODN!RCID!RSNM!ZONE", "(A,I,2A)", &["XREF", "1", "UTM", "16"]));
        write("LDEF", one("LDEF", "MODN!RCID!CMNM!NROW!NCOL", "(A,I,A,2I)", &["LDEF", "1", "CEL0", "2", "3"]));
        write("DDSH", one("DDSH", "MODN!RCID!FMT!UNIT", "(A,I,2A)", &["DDSH", "1", "BI16", "FEET"]));
        write("RSDF", one("RSDF", "MODN!RCID!SCOR", "(A,I,A)", &["RSDF", "1", "TL"]));
        let row = |r: u8, vals: [i16; 3]| {
            let mut cell = format!("CEL0{r:05}00001").into_bytes();
            cell.push(0x1e);
            let mut v: Vec<u8> = vals.iter().flat_map(|x| x.to_be_bytes()).collect();
            v.push(0x1e);
            vec![("CELL", cell), ("CVLS", v)]
        };
        write(
            "CEL0",
            write_module(
                &[("CELL", "Cell", "MODN!ROWI!COLI", "(A(4),2I(5))"), ("CVLS", "Cell Values", "*ELEVATION", "(B(16))")],
                &[row(1, [10, 11, 12]), row(2, [20, -32767, 22])],
                true,
            ),
        );
        // Opened from any module of the transfer.
        let g = read(&dir.join("TESTtLDEF.DDF")).unwrap();
        assert_eq!(g.title, "MONT TEST");
        assert_eq!((g.width, g.height, g.ground_unit, g.elev_unit), (3, 2, 2, 1));
        // The first row is the northern one.
        assert_eq!((g.at(0, 1), g.at(2, 1), g.at(0, 0)), (10.0, 12.0, 20.0));
        assert!(g.at(1, 0).is_nan());
        assert_eq!((g.min, g.max), (10.0, 22.0));
    }
}
