//! The logo and the files made from it (see the parent module).
//!
//! A symbol is a list of shapes painted in order on a 64-unit grid (tile, strokes, arc, discs,
//! polygons), each with its exact signed distance and its SVG element. The word, « Esquisse »,
//! comes from the logo's font through ttf-parser: capitals 30 units high, baseline 47 (the font
//! kerns no pair of the word). The font and its licence are in `assets/logo/police`.
//!
//! Rasterising needs no library: a pixel further than half a diagonal from every edge takes the
//! colour of its centre, the others 64 samples; the letters are filled scanline by scanline
//! (non-zero winding, 64 samples a pixel). PNG through `image`, ICO through `crate::winres`.
//! Same drawing, same pixels.
//!
//! - `ESQ_WRITE_LOGO=compas` (or `1`: the logo named in `assets/logo/LOGO`)
//!   `cargo test -p esq-app logo` writes the set of the logo into `assets/logo` and `docs/media`.
//! - `ESQ_LOGO_OUT=<folder>` writes the whole set into that folder instead (to try a logo
//!   without putting it in the repository; build with `ESQ_LOGO_ASSETS=<folder>`).
//! - `ESQ_LOGO_FONTS=<folder>`: where the fonts are (default `assets/logo/police`).
//! - Without `ESQ_WRITE_LOGO`, the test checks that the set in the repository, if any, is the
//!   one its drawing gives.

use std::f64::consts::TAU;
use std::path::{Path, PathBuf};

use image::ImageEncoder;

type Rgb = [u8; 3];
type Pt = [f64; 2];

pub const RED_CHALK: Rgb = [0xB4, 0x44, 0x2B];
pub const VERMILION: Rgb = [0xE0, 0x45, 0x2B];
pub const PAPER: Rgb = [0xF2, 0xED, 0xE3];
pub const INK: Rgb = [0x1D, 0x24, 0x33];

const GRID: f64 = 64.0;
const CAP_HEIGHT: f64 = 30.0;
const BASELINE: f64 = 47.0;
/// PNG sizes of the icon; the ICO holds those up to 256.
pub const SIZES: [u32; 8] = [16, 24, 32, 48, 64, 128, 256, 512];
/// Height of the PNG of the logo with its word, in pixels.
const WORD_PNG_HEIGHT: u32 = 64;
const SUB: usize = 8;

/// The logos the generator draws (another logo would be another variant).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Logo {
    /// A drawing compass and the arc it has just drawn, paper on a red-chalk tile; Space Grotesk.
    Compas,
}

impl Logo {
    pub const ALL: [Logo; 1] = [Logo::Compas];

    pub fn name(self) -> &'static str {
        match self {
            Logo::Compas => "compas",
        }
    }
    pub fn from_name(s: &str) -> Option<Logo> {
        Logo::ALL.into_iter().find(|l| l.name() == s.trim())
    }
    fn font_file(self) -> &'static str {
        match self {
            Logo::Compas => "SpaceGrotesk-Medium.ttf",
        }
    }
    /// The font's licence (SIL Open Font License 1.1) and its authors, beside it.
    fn licence_files(self) -> [&'static str; 2] {
        match self {
            Logo::Compas => ["SpaceGrotesk-OFL.txt", "AUTEURS.txt"],
        }
    }
    /// Where the word starts, after the symbol.
    fn word_x(self) -> f64 {
        match self {
            Logo::Compas => 68.0,
        }
    }
    /// The application icon.
    pub fn icon(self) -> Vec<(Shape, Rgb)> {
        match self {
            Logo::Compas => compass_shapes(Some(RED_CHALK), PAPER, PAPER),
        }
    }
    /// The symbol beside the word, on a light or a dark background.
    fn word_symbol(self, dark: bool) -> Vec<(Shape, Rgb)> {
        match self {
            Logo::Compas if dark => compass_shapes(None, PAPER, VERMILION),
            Logo::Compas => compass_shapes(None, RED_CHALK, VERMILION),
        }
    }
}

// ---------------------------------------------------------------- shapes

#[derive(Clone, Debug)]
pub enum Shape {
    /// The rounded square of the icon.
    Tile {
        radius: f64,
    },
    /// A polyline with round caps and joins.
    Stroke {
        pts: Vec<Pt>,
        width: f64,
    },
    /// An arc of circle (angles in radians, y downwards, from `a0` increasing to `a1`), round caps.
    Arc {
        c: Pt,
        r: f64,
        a0: f64,
        a1: f64,
        width: f64,
    },
    Disc {
        c: Pt,
        r: f64,
    },
    Polygon(Vec<Pt>),
}

fn sub(a: Pt, b: Pt) -> Pt {
    [a[0] - b[0], a[1] - b[1]]
}

fn len(a: Pt) -> f64 {
    a[0].hypot(a[1])
}

fn unit(a: Pt) -> Pt {
    let l = len(a);
    [a[0] / l, a[1] / l]
}

fn seg_distance(p: Pt, a: Pt, b: Pt) -> f64 {
    let ab = sub(b, a);
    let t = (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / (ab[0] * ab[0] + ab[1] * ab[1])).clamp(0.0, 1.0);
    (p[0] - a[0] - t * ab[0]).hypot(p[1] - a[1] - t * ab[1])
}

fn arc_point(c: Pt, r: f64, a: f64) -> Pt {
    [c[0] + r * a.cos(), c[1] + r * a.sin()]
}

impl Shape {
    /// Signed distance from `p` (negative inside), exact: never more than the true distance.
    fn distance(&self, p: Pt) -> f64 {
        match self {
            Shape::Tile { radius } => {
                let h = GRID / 2.0 - radius;
                let q = [(p[0] - GRID / 2.0).abs() - h, (p[1] - GRID / 2.0).abs() - h];
                q[0].max(0.0).hypot(q[1].max(0.0)) + q[0].max(q[1]).min(0.0) - radius
            }
            Shape::Stroke { pts, width } => {
                pts.windows(2).map(|s| seg_distance(p, s[0], s[1])).fold(f64::MAX, f64::min) - width / 2.0
            }
            Shape::Arc { c, r, a0, a1, width } => {
                let d = sub(p, *c);
                let along = (d[1].atan2(d[0]) - a0).rem_euclid(TAU);
                let to_arc = if along <= a1 - a0 {
                    (len(d) - r).abs()
                } else {
                    len(sub(p, arc_point(*c, *r, *a0))).min(len(sub(p, arc_point(*c, *r, *a1))))
                };
                to_arc - width / 2.0
            }
            Shape::Disc { c, r } => len(sub(p, *c)) - r,
            Shape::Polygon(pts) => {
                let n = pts.len();
                let mut inside = false;
                let mut d = f64::MAX;
                for k in 0..n {
                    let (a, b) = (pts[k], pts[(k + 1) % n]);
                    d = d.min(seg_distance(p, a, b));
                    if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]) {
                        inside = !inside;
                    }
                }
                if inside { -d } else { d }
            }
        }
    }

    fn svg(&self, c: Rgb) -> String {
        let col = hex(c);
        match self {
            Shape::Tile { radius } => {
                format!("<rect width=\"64\" height=\"64\" rx=\"{}\" fill=\"{col}\"/>", num(*radius))
            }
            Shape::Stroke { pts, width } => format!(
                "<path d=\"M{}\" fill=\"none\" stroke=\"{col}\" stroke-width=\"{}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>",
                pts.iter().map(|p| pt(*p)).collect::<Vec<_>>().join("L"),
                num(*width)
            ),
            Shape::Arc { c, r, a0, a1, width } => format!(
                "<path d=\"M{}A{},{} 0 {},1 {}\" fill=\"none\" stroke=\"{col}\" stroke-width=\"{}\" stroke-linecap=\"round\"/>",
                pt(arc_point(*c, *r, *a0)),
                num(*r),
                num(*r),
                u8::from(a1 - a0 > std::f64::consts::PI),
                pt(arc_point(*c, *r, *a1)),
                num(*width)
            ),
            Shape::Disc { c, r } => {
                format!("<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{col}\"/>", num(c[0]), num(c[1]), num(*r))
            }
            Shape::Polygon(pts) => format!(
                "<polygon points=\"{}\" fill=\"{col}\"/>",
                pts.iter().map(|p| pt(*p)).collect::<Vec<_>>().join(" ")
            ),
        }
    }
}

/// « Compas »: two legs tapering from the head to their points, the arc centred on the head
/// through both points and a little past them, the head and its handle.
fn compass_shapes(tile: Option<Rgb>, ink: Rgb, arc: Rgb) -> Vec<(Shape, Rgb)> {
    let (head, left, right) = ([32.0, 15.0], [16.0, 50.0], [48.0, 50.0]);
    let r = len(sub(left, head));
    let angle = |p: Pt| (p[1] - head[1]).atan2(p[0] - head[0]);
    let leg = |tip: Pt| {
        let u = unit(sub(tip, head));
        let n = [-u[1], u[0]];
        let (w0, w1) = (2.5, 0.65); // half widths at the head and at the point
        Shape::Polygon(vec![
            [head[0] + n[0] * w0, head[1] + n[1] * w0],
            [tip[0] + n[0] * w1, tip[1] + n[1] * w1],
            [tip[0] - n[0] * w1, tip[1] - n[1] * w1],
            [head[0] - n[0] * w0, head[1] - n[1] * w0],
        ])
    };
    let mut out: Vec<(Shape, Rgb)> = tile.map(|c| (Shape::Tile { radius: 14.0 }, c)).into_iter().collect();
    out.push((Shape::Arc { c: head, r, a0: angle(right) - 0.1, a1: angle(left) + 0.1, width: 2.4 }, arc));
    out.push((leg(left), ink));
    out.push((leg(right), ink));
    out.push((Shape::Disc { c: head, r: 5.2 }, ink));
    out.push((Shape::Stroke { pts: vec![[32.0, 11.0], [32.0, 6.0]], width: 3.2 }, ink));
    out
}

/// A number as SVG wants it: at most two decimals, no trailing zero.
fn num(v: f64) -> String {
    let s = format!("{:.2}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

fn pt(p: Pt) -> String {
    format!("{},{}", num(p[0]), num(p[1]))
}

fn hex(c: Rgb) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

fn body(shapes: &[(Shape, Rgb)]) -> String {
    shapes.iter().map(|(s, c)| s.svg(*c)).collect()
}

pub fn icon_svg(logo: Logo) -> String {
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 64 64\">{}</svg>\n", body(&logo.icon()))
}

// ---------------------------------------------------------------- the word

/// The outlines of the word, as SVG path data and as polygons (curves flattened).
#[derive(Default)]
struct Outline {
    svg: String,
    polys: Vec<Vec<Pt>>,
    cur: Vec<Pt>,
    last: Pt,
    x0: f64,
    scale: f64,
}

impl Outline {
    fn at(&self, x: f32, y: f32) -> Pt {
        [self.x0 + x as f64 * self.scale, BASELINE - y as f64 * self.scale]
    }
    fn cmd(&mut self, c: char, pts: &[Pt]) {
        self.svg.push(c);
        self.svg.push_str(&pts.iter().map(|p| pt(*p)).collect::<Vec<_>>().join(" "));
    }
}

impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = self.at(x, y);
        self.cmd('M', &[p]);
        self.cur = vec![p];
        self.last = p;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.at(x, y);
        self.cmd('L', &[p]);
        self.cur.push(p);
        self.last = p;
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (c, p, a) = (self.at(x1, y1), self.at(x, y), self.last);
        self.cmd('Q', &[c, p]);
        for i in 1..=8 {
            let t = i as f64 / 8.0;
            let m = 1.0 - t;
            self.cur.push([
                m * m * a[0] + 2.0 * m * t * c[0] + t * t * p[0],
                m * m * a[1] + 2.0 * m * t * c[1] + t * t * p[1],
            ]);
        }
        self.last = p;
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (c1, c2, p, a) = (self.at(x1, y1), self.at(x2, y2), self.at(x, y), self.last);
        self.cmd('C', &[c1, c2, p]);
        for i in 1..=12 {
            let t = i as f64 / 12.0;
            let m = 1.0 - t;
            let (k0, k1, k2, k3) = (m * m * m, 3.0 * m * m * t, 3.0 * m * t * t, t * t * t);
            self.cur.push([
                k0 * a[0] + k1 * c1[0] + k2 * c2[0] + k3 * p[0],
                k0 * a[1] + k1 * c1[1] + k2 * c2[1] + k3 * p[1],
            ]);
        }
        self.last = p;
    }
    fn close(&mut self) {
        self.svg.push('Z');
        let poly = std::mem::take(&mut self.cur);
        if poly.len() > 2 {
            self.polys.push(poly);
        }
    }
}

pub fn fonts_dir() -> PathBuf {
    match std::env::var_os("ESQ_LOGO_FONTS") {
        Some(d) => PathBuf::from(d),
        None => Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/logo/police"),
    }
}

/// « Esquisse » set after the symbol, and the width of the whole logo; `None` without the font.
fn word(logo: Logo) -> Option<(Outline, f64)> {
    let data = std::fs::read(fonts_dir().join(logo.font_file())).ok()?;
    let face = ttf_parser::Face::parse(&data, 0).expect("police du logo illisible");
    let cap = face.capital_height().expect("hauteur des capitales") as f64;
    let mut out = Outline { x0: logo.word_x(), scale: CAP_HEIGHT / cap, ..Default::default() };
    for ch in "Esquisse".chars() {
        let g = face.glyph_index(ch).expect("glyphe");
        face.outline_glyph(g, &mut out);
        out.x0 += face.glyph_hor_advance(g).expect("avance") as f64 * out.scale;
    }
    let width = (out.x0 + 2.0).ceil();
    Some((out, width))
}

/// The logo with its word; `dark_css` gives the light variant the colours of the dark one under a
/// dark colour scheme (for the README, seen in GitHub's light and dark themes).
fn word_svg(logo: Logo, dark: bool, dark_css: bool) -> Option<String> {
    let (w, width) = word(logo)?;
    let text = if dark { PAPER } else { INK };
    let symbol = logo.word_symbol(dark);
    let mut rules = String::new();
    let mut elements = String::new();
    for (i, ((shape, colour), (_, night))) in symbol.iter().zip(logo.word_symbol(true)).enumerate() {
        let mut e = shape.svg(*colour);
        if dark_css && night != *colour {
            e = e.replacen(' ', &format!(" class=\"s{i}\" "), 1);
            let paint = if matches!(shape, Shape::Stroke { .. } | Shape::Arc { .. }) { "stroke" } else { "fill" };
            rules += &format!(".s{i}{{{paint}:{}}}", hex(night));
        }
        elements += &e;
    }
    let style = if dark_css {
        format!("<style>@media (prefers-color-scheme: dark){{{rules}.mot{{fill:{}}}}}</style>", hex(PAPER))
    } else {
        String::new()
    };
    let class = if dark_css { " class=\"mot\"" } else { "" };
    Some(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} 64\">{style}{elements}<path{class} fill=\"{}\" d=\"{}\"/></svg>\n",
        num(width),
        hex(text),
        w.svg
    ))
}

// ---------------------------------------------------------------- rasterising

/// Straight RGBA of a pixel from the colours of its samples (`hits` of `total` covered).
fn mix(sum: [f64; 3], hits: usize, total: usize) -> [u8; 4] {
    if hits == 0 {
        return [0; 4];
    }
    let c = |i: usize| (sum[i] / hits as f64).round() as u8;
    [c(0), c(1), c(2), (255.0 * hits as f64 / total as f64).round() as u8]
}

/// Colour of the topmost shape over `p`.
fn colour_at(shapes: &[(Shape, Rgb)], p: Pt) -> Option<Rgb> {
    shapes.iter().rev().find(|(s, _)| s.distance(p) <= 0.0).map(|(_, c)| *c)
}

/// Pixel (`i`, `j`) of a drawing at `u` units per pixel.
fn shapes_pixel(shapes: &[(Shape, Rgb)], i: u32, j: u32, u: f64) -> [u8; 4] {
    let c = [(i as f64 + 0.5) * u, (j as f64 + 0.5) * u];
    let half = 0.708 * u;
    if shapes.iter().all(|(s, _)| s.distance(c).abs() > half) {
        return colour_at(shapes, c).map_or([0; 4], |k| [k[0], k[1], k[2], 255]);
    }
    let (mut sum, mut hits) = ([0.0; 3], 0);
    for b in 0..SUB {
        for a in 0..SUB {
            let p = [(i as f64 + (a as f64 + 0.5) / SUB as f64) * u, (j as f64 + (b as f64 + 0.5) / SUB as f64) * u];
            if let Some(k) = colour_at(shapes, p) {
                for (s, v) in sum.iter_mut().zip(k) {
                    *s += v as f64;
                }
                hits += 1;
            }
        }
    }
    mix(sum, hits, SUB * SUB)
}

/// The icon of `logo`, `px` pixels square, straight RGBA.
pub fn render_icon(logo: Logo, px: u32) -> Vec<u8> {
    let shapes = logo.icon();
    let u = GRID / px as f64;
    (0..px).flat_map(|j| (0..px).map(move |i| (i, j))).flat_map(|(i, j)| shapes_pixel(&shapes, i, j, u)).collect()
}

/// Samples of each pixel of a row covered by `polys` (non-zero winding): `SUB` lines of `SUB`
/// samples a pixel, `u` units per pixel.
pub fn fill_row(polys: &[Vec<Pt>], j: u32, width: u32, u: f64) -> Vec<usize> {
    let mut hits = vec![0usize; width as usize];
    for b in 0..SUB {
        let y = (j as f64 + (b as f64 + 0.5) / SUB as f64) * u;
        let mut cross: Vec<(f64, i32)> = Vec::new();
        for poly in polys {
            for k in 0..poly.len() {
                let (p, q) = (poly[k], poly[(k + 1) % poly.len()]);
                if (p[1] <= y) != (q[1] <= y) {
                    cross.push((p[0] + (y - p[1]) * (q[0] - p[0]) / (q[1] - p[1]), if q[1] > p[1] { 1 } else { -1 }));
                }
            }
        }
        cross.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (mut k, mut wind) = (0, 0);
        for (i, h) in hits.iter_mut().enumerate() {
            for a in 0..SUB {
                let x = (i as f64 + (a as f64 + 0.5) / SUB as f64) * u;
                while k < cross.len() && cross[k].0 <= x {
                    wind += cross[k].1;
                    k += 1;
                }
                if wind != 0 {
                    *h += 1;
                }
            }
        }
    }
    hits
}

/// The logo with its word, [`WORD_PNG_HEIGHT`] pixels high, for a light or a dark background.
fn render_word(logo: Logo, dark: bool) -> Option<(u32, u32, Vec<u8>)> {
    let (w, width) = word(logo)?;
    let u = GRID / WORD_PNG_HEIGHT as f64;
    let (pw, ph) = ((width / u).ceil() as u32, WORD_PNG_HEIGHT);
    let shapes = logo.word_symbol(dark);
    let text = if dark { PAPER } else { INK };
    let mut out = Vec::with_capacity((pw * ph * 4) as usize);
    for j in 0..ph {
        let hits = fill_row(&w.polys, j, pw, u);
        for i in 0..pw {
            // The symbol stays within its 64 units, the word starts after it.
            let px = if (i as f64 + 0.5) * u < GRID + 2.0 {
                shapes_pixel(&shapes, i, j, u)
            } else {
                let h = hits[i as usize] as f64;
                mix([text[0] as f64 * h, text[1] as f64 * h, text[2] as f64 * h], hits[i as usize], SUB * SUB)
            };
            out.extend_from_slice(&px);
        }
    }
    Some((pw, ph, out))
}

pub fn png(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out).write_image(rgba, w, h, image::ExtendedColorType::Rgba8).unwrap();
    out
}

/// The PNG of the logo with its word for a light and a dark background (`None` without the font).
pub fn word_pngs(logo: Logo) -> Option<(Vec<u8>, Vec<u8>)> {
    let (w, h, light) = render_word(logo, false)?;
    let (w2, h2, dark) = render_word(logo, true)?;
    Some((png(w, h, &light), png(w2, h2, &dark)))
}

/// The ICO of `logo`: its icon at every size up to 256, as PNG.
pub fn ico(logo: Logo) -> Vec<u8> {
    let pngs: Vec<(u16, Vec<u8>)> =
        SIZES.iter().filter(|&&s| s <= 256).map(|&s| (s as u16, png(s, s, &render_icon(logo, s)))).collect();
    crate::winres::write_ico(&pngs.iter().map(|(s, p)| (*s, *s, p.as_slice())).collect::<Vec<_>>())
}

// ---------------------------------------------------------------- the files

enum Content {
    Text(String),
    Png(u32, u32, Vec<u8>),
    Ico(Logo),
}

/// Where a file of the set goes in the repository.
#[derive(Clone, Copy, PartialEq)]
enum Place {
    Assets,
    Docs,
}

/// Every file of the set of `logo`; `None` without its font.
fn files(logo: Logo) -> Option<Vec<(Place, String, Content)>> {
    let mut out = vec![
        (Place::Assets, "LOGO".to_string(), Content::Text(format!("{}\n", logo.name()))),
        (Place::Assets, "esquisse.svg".to_string(), Content::Text(icon_svg(logo))),
        (Place::Assets, "esquisse-mot-clair.svg".to_string(), Content::Text(word_svg(logo, false, false)?)),
        (Place::Assets, "esquisse-mot-sombre.svg".to_string(), Content::Text(word_svg(logo, true, false)?)),
        (Place::Docs, "logo.svg".to_string(), Content::Text(icon_svg(logo))),
        (Place::Docs, "logo-mot.svg".to_string(), Content::Text(word_svg(logo, false, true)?)),
    ];
    for s in SIZES {
        out.push((Place::Assets, format!("esquisse-{s}.png"), Content::Png(s, s, render_icon(logo, s))));
    }
    out.push((Place::Assets, "esquisse.ico".to_string(), Content::Ico(logo)));
    for (dark, name) in [(false, "esquisse-mot-clair.png"), (true, "esquisse-mot-sombre.png")] {
        let (w, h, px) = render_word(logo, dark)?;
        out.push((Place::Assets, name.to_string(), Content::Png(w, h, px)));
    }
    Some(out)
}

fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/logo")
}

fn docs() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/media")
}

fn decoded(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png).ok()?.to_rgba8();
    Some((img.width(), img.height(), img.into_raw()))
}

/// Why `path` is not what the drawing gives, `None` when it is.
fn differs(path: &Path, c: &Content) -> Option<String> {
    let Ok(bytes) = std::fs::read(path) else { return Some("absent".into()) };
    match c {
        Content::Text(t) => {
            (String::from_utf8_lossy(&bytes).replace("\r\n", "\n") != *t).then(|| "texte différent".into())
        }
        Content::Png(w, h, px) => (decoded(&bytes) != Some((*w, *h, px.clone()))).then(|| "pixels différents".into()),
        Content::Ico(logo) => {
            let Ok(got) = crate::winres::parse_ico(&bytes) else { return Some("ICO illisible".into()) };
            let sizes: Vec<u32> = SIZES.iter().copied().filter(|&s| s <= 256).collect();
            let same = got.len() == sizes.len()
                && got
                    .iter()
                    .zip(&sizes)
                    .all(|(g, &s)| g.width as u32 == s && decoded(g.data) == Some((s, s, render_icon(*logo, s))));
            (!same).then(|| "images différentes".into())
        }
    }
}

fn write(path: &Path, c: &Content) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let bytes = match c {
        Content::Text(t) => t.clone().into_bytes(),
        Content::Png(w, h, px) => png(*w, *h, px),
        Content::Ico(logo) => ico(*logo),
    };
    std::fs::write(path, bytes).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The logo in the repository, if any, is what its drawing gives; `ESQ_WRITE_LOGO` writes
    /// a set (see the module).
    #[test]
    fn logo_files_are_up_to_date() {
        let chosen = std::fs::read_to_string(assets().join("LOGO")).ok().and_then(|s| Logo::from_name(&s));
        if let Ok(w) = std::env::var("ESQ_WRITE_LOGO") {
            let logo =
                Logo::from_name(&w).or(chosen).expect("ESQ_WRITE_LOGO : compas, ou 1 (logo de assets/logo/LOGO)");
            let set = files(logo).unwrap_or_else(|| panic!("police absente de {}", fonts_dir().display()));
            let out = std::env::var_os("ESQ_LOGO_OUT").map(PathBuf::from);
            for (place, name, c) in &set {
                let dir = match (&out, place) {
                    (Some(d), _) => d.clone(),
                    (None, Place::Assets) => assets(),
                    (None, Place::Docs) => docs(),
                };
                write(&dir.join(name), c);
            }
            return;
        }
        let Some(logo) = chosen else {
            // No logo chosen yet: none of its files may sit in the repository.
            let stray: Vec<String> =
                ["esquisse.ico", "esquisse-256.png", "esquisse-mot-clair.png", "esquisse-mot-sombre.png"]
                    .iter()
                    .filter(|n| assets().join(n).exists())
                    .map(|n| n.to_string())
                    .collect();
            assert!(stray.is_empty(), "fichiers de logo sans assets/logo/LOGO : {stray:?}");
            return;
        };
        let set = files(logo)
            .unwrap_or_else(|| panic!("police du logo « {} » absente de {}", logo.name(), fonts_dir().display()));
        let stale: Vec<String> = set
            .iter()
            .filter_map(|(place, name, c)| {
                let dir = if *place == Place::Assets { assets() } else { docs() };
                differs(&dir.join(name), c).map(|why| format!("{name} : {why}"))
            })
            .collect();
        assert!(stale.is_empty(), "logo à régénérer (ESQ_WRITE_LOGO=1 cargo test -p esq-app logo) : {stale:#?}");
    }

    #[test]
    fn the_compass_draws_its_arc_through_both_points() {
        let shapes = Logo::Compas.icon();
        let Shape::Arc { c, r, a0, a1, .. } = shapes[1].0 else { panic!("l'arc") };
        for tip in [[16.0, 50.0], [48.0, 50.0]] {
            assert!((len(sub(tip, c)) - r).abs() < 1e-9);
            let a = (tip[1] - c[1]).atan2(tip[0] - c[0]);
            assert!(a > a0 && a < a1, "la pointe est sur l'arc, qui la dépasse un peu");
        }
        let svg = icon_svg(Logo::Compas);
        assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 64 64\"><rect width=\"64\" height=\"64\" rx=\"14\" fill=\"#B4442B\"/>"));
        assert_eq!(svg.matches("<polygon").count(), 2);
        assert!(svg.contains("<circle cx=\"32\" cy=\"15\" r=\"5.2\" fill=\"#F2EDE3\"/>"));
    }

    /// The icon at 16 px: the rounded tile fills the square but its corners, the drawing shows.
    #[test]
    fn the_icon_reads_at_16_pixels() {
        for logo in Logo::ALL {
            let px = render_icon(logo, 16);
            assert_eq!(px.len(), 16 * 16 * 4);
            assert_eq!(px[3], 0, "coin");
            assert_eq!(px[(8 * 16 + 1) * 4 + 3], 255, "tuile");
            let light = px.chunks(4).filter(|p| p[3] == 255 && p[0] > 0xD0).count();
            assert!(light > 12, "{}: {light} pixels du dessin", logo.name());
        }
    }

    #[test]
    fn signed_distances_are_exact() {
        let tile = Shape::Tile { radius: 14.0 };
        assert!((tile.distance([32.0, 32.0]) + 32.0).abs() < 1e-9);
        assert!((tile.distance([0.0, 0.0]) - (14.0 * 2f64.sqrt() - 14.0)).abs() < 1e-9);
        let arc = Shape::Arc { c: [0.0, 0.0], r: 10.0, a0: 0.0, a1: 1.0, width: 2.0 };
        assert!((arc.distance([10.0, 0.0]) + 1.0).abs() < 1e-9);
        // Off the arc: the nearest end, here the one at 1 radian.
        let end = (-10.0 - 10.0 * 1f64.cos()).hypot(10.0 * 1f64.sin());
        assert!(end < 20.0 && (arc.distance([-10.0, 0.0]) - (end - 1.0)).abs() < 1e-9);
        let sq = Shape::Polygon(vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]]);
        assert!((sq.distance([2.0, 2.0]) + 2.0).abs() < 1e-9 && (sq.distance([6.0, 2.0]) - 2.0).abs() < 1e-9);
        let stroke = Shape::Stroke { pts: vec![[0.0, 0.0], [10.0, 0.0]], width: 2.0 };
        assert!((stroke.distance([5.0, 3.0]) - 2.0).abs() < 1e-9 && (stroke.distance([13.0, 4.0]) - 4.0).abs() < 1e-9);
    }

    /// Letters are filled by non-zero winding: a counter turning the other way is a hole.
    #[test]
    fn letters_fill_by_non_zero_winding() {
        let outer = vec![[0.0, 0.0], [8.0, 0.0], [8.0, 8.0], [0.0, 8.0]];
        let hole = vec![[2.0, 2.0], [2.0, 6.0], [6.0, 6.0], [6.0, 2.0]];
        let same = vec![[2.0, 2.0], [6.0, 2.0], [6.0, 6.0], [2.0, 6.0]];
        let full = SUB * SUB;
        let row = fill_row(&[outer.clone(), hole], 4, 8, 1.0);
        assert_eq!((row[0], row[1], row[3], row[4], row[7]), (full, full, 0, 0, full));
        assert!(fill_row(&[outer, same], 4, 8, 1.0).iter().all(|&h| h == full));
        // A half-covered pixel: half its samples.
        assert_eq!(fill_row(&[vec![[0.0, 0.0], [1.5, 0.0], [1.5, 1.0], [0.0, 1.0]]], 0, 2, 1.0)[1], full / 2);
    }

    /// The word comes from the logo's font, in the repository with its licence and its authors.
    #[test]
    fn the_word_comes_from_the_logo_font_under_its_licence() {
        for logo in Logo::ALL {
            let (w, width) = word(logo).unwrap_or_else(|| panic!("police absente de {}", fonts_dir().display()));
            // Contours: E, s, q (2), u, i (2), s, s, e (2).
            assert_eq!(w.polys.len(), 11, "{}", logo.name());
            // Symbol (68), the word in Space Grotesk Medium (181), margin.
            assert!((245.0..260.0).contains(&width), "{} : {width}", logo.name());
            // In the README's dark theme the word turns paper, and so do the compass's legs, head
            // and handle (its arc stays vermilion).
            let readme = word_svg(logo, false, true).unwrap();
            assert_eq!(readme.matches("class=\"s").count(), 4);
            assert!(readme.contains("class=\"mot\""));
            let [licence, authors] =
                logo.licence_files().map(|f| std::fs::read_to_string(fonts_dir().join(f)).unwrap());
            assert!(licence.contains("SIL OPEN FONT LICENSE Version 1.1"));
            assert!(licence.contains("The Space Grotesk Project Authors"));
            assert!(authors.contains("Florian Karsten") && authors.contains("SpaceGrotesk-OFL.txt"));
        }
    }
}
