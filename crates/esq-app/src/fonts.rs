//! System fonts for 3D text. They are read from the Windows font folders and never shipped:
//! the list only reads each file's table directory and its `name` / `OS/2` tables, the
//! outlines are read when a text is built.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use esq_core::glam::DVec2;
use esq_core::text3d::{FONT_EM, FontMetrics, Glyph, GlyphSource, Outline, Seg};
use ttf_parser::{Face, GlyphId, OutlineBuilder};

#[derive(Clone, Debug)]
pub struct FontFile {
    pub path: PathBuf,
    /// Face number inside a `.ttc` collection.
    pub index: u32,
    weight: u16,
}

/// Families with, for each (bold, italic) style, the file that best matches it.
#[derive(Default)]
pub struct FontList {
    pub families: BTreeMap<String, BTreeMap<(bool, bool), FontFile>>,
}

impl FontList {
    pub fn file(&self, family: &str, bold: bool, italic: bool) -> Option<&FontFile> {
        let styles = self.families.get(family)?;
        styles
            .get(&(bold, italic))
            .or_else(|| styles.get(&(bold, false)))
            .or_else(|| styles.get(&(false, false)))
            .or_else(|| styles.values().next())
    }

    /// The styles a family really has.
    pub fn styles(&self, family: &str) -> Vec<(bool, bool)> {
        self.families.get(family).map(|s| s.keys().copied().collect()).unwrap_or_default()
    }
}

pub fn font_list() -> &'static FontList {
    static LIST: OnceLock<FontList> = OnceLock::new();
    LIST.get_or_init(|| {
        let mut dirs =
            vec![PathBuf::from(std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into())).join("Fonts")];
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join(r"Microsoft\Windows\Fonts"));
        }
        scan(&dirs)
    })
}

pub fn scan(dirs: &[PathBuf]) -> FontList {
    let mut list = FontList::default();
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(dir) else { continue };
        let mut files: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
        files.sort();
        for path in files {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            if !matches!(ext.as_str(), "ttf" | "otf" | "ttc") {
                continue;
            }
            for FaceInfo { index, family, bold, italic, weight } in read_faces(&path).unwrap_or_default() {
                let file = FontFile { path: path.clone(), index, weight };
                let styles = list.families.entry(family).or_default();
                // Prefer the weight nearest to regular (400) or bold (700).
                let want = if bold { 700 } else { 400 };
                let better = |old: &FontFile| (weight as i32 - want).abs() < (old.weight as i32 - want).abs();
                match styles.get(&(bold, italic)) {
                    Some(old) if !better(old) => {}
                    _ => {
                        styles.insert((bold, italic), file);
                    }
                }
            }
        }
    }
    list
}

fn be16(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(o)?, *b.get(o + 1)?]))
}

fn be32(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_be_bytes([*b.get(o)?, *b.get(o + 1)?, *b.get(o + 2)?, *b.get(o + 3)?]))
}

fn read_at(f: &mut File, offset: u64, len: usize) -> Option<Vec<u8>> {
    f.seek(SeekFrom::Start(offset)).ok()?;
    let mut v = vec![0u8; len];
    f.read_exact(&mut v).ok()?;
    Some(v)
}

/// One face found in a font file.
struct FaceInfo {
    index: u32,
    family: String,
    bold: bool,
    italic: bool,
    /// OS/2 weight class (400 regular, 700 bold).
    weight: u16,
}

fn read_faces(path: &Path) -> Option<Vec<FaceInfo>> {
    let mut f = File::open(path).ok()?;
    let head = read_at(&mut f, 0, 12)?;
    let offsets: Vec<u64> = if &head[0..4] == b"ttcf" {
        let n = be32(&head, 8)? as usize;
        let table = read_at(&mut f, 12, 4 * n.min(64))?;
        (0..n.min(64)).filter_map(|i| be32(&table, 4 * i).map(u64::from)).collect()
    } else {
        vec![0]
    };
    let mut out = Vec::new();
    for (index, off) in offsets.into_iter().enumerate() {
        let dir = read_at(&mut f, off, 12)?;
        let n = be16(&dir, 4)? as usize;
        let recs = read_at(&mut f, off + 12, 16 * n.min(80))?;
        let find = |tag: &[u8; 4]| -> Option<(u64, usize)> {
            (0..n.min(80)).find_map(|i| {
                let r = &recs[16 * i..16 * i + 16];
                (&r[0..4] == tag).then(|| (be32(r, 8).unwrap_or(0) as u64, be32(r, 12).unwrap_or(0) as usize))
            })
        };
        let Some((no, nl)) = find(b"name") else { continue };
        let name = read_at(&mut f, no, nl.min(1 << 20))?;
        let Some(family) = family_name(&name) else { continue };
        let (mut bold, mut italic, mut weight) = (false, false, 400u16);
        if let Some((oo, ol)) = find(b"OS/2")
            && ol >= 64
            && let Some(os2) = read_at(&mut f, oo, 64)
        {
            weight = be16(&os2, 4).unwrap_or(400);
            let sel = be16(&os2, 62).unwrap_or(0);
            italic = sel & 0x01 != 0;
            bold = sel & 0x20 != 0 || weight >= 600;
        }
        out.push(FaceInfo { index: index as u32, family, bold, italic, weight });
    }
    Some(out)
}

/// Typographic family (name 16) or family (name 1), Windows / Unicode platform, any language
/// preferring US English.
fn family_name(name: &[u8]) -> Option<String> {
    let count = be16(name, 2)? as usize;
    let str_off = be16(name, 4)? as usize;
    let mut best: Option<(u8, String)> = None;
    for i in 0..count {
        let r = 6 + 12 * i;
        let (plat, id, lang, len, off) = (
            be16(name, r)?,
            be16(name, r + 6)?,
            be16(name, r + 4)?,
            be16(name, r + 8)? as usize,
            be16(name, r + 10)? as usize,
        );
        if (id != 1 && id != 16) || (plat != 3 && plat != 0) {
            continue;
        }
        let raw = name.get(str_off + off..str_off + off + len)?;
        let units: Vec<u16> = raw.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect();
        let s = String::from_utf16_lossy(&units);
        if s.is_empty() {
            continue;
        }
        // Higher rank wins: typographic family first, then US English.
        let rank = (id == 16) as u8 * 2 + (lang == 0x0409) as u8;
        if best.as_ref().is_none_or(|(r, _)| rank > *r) {
            best = Some((rank, s));
        }
    }
    best.map(|(_, s)| s)
}

/// Collects a glyph outline as [`Outline`]s, scaled to an em of [`FONT_EM`].
struct Collect {
    k: f64,
    done: Vec<Outline>,
    cur: Option<Outline>,
}

impl Collect {
    fn p(&self, x: f32, y: f32) -> DVec2 {
        DVec2::new(x as f64, y as f64) * self.k
    }
}

impl OutlineBuilder for Collect {
    fn move_to(&mut self, x: f32, y: f32) {
        self.close();
        self.cur = Some(Outline { start: self.p(x, y), segs: Vec::new() });
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.p(x, y);
        if let Some(c) = &mut self.cur {
            c.segs.push(Seg::Line(p));
        }
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (a, b) = (self.p(x1, y1), self.p(x, y));
        if let Some(c) = &mut self.cur {
            c.segs.push(Seg::Quad(a, b));
        }
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (a, b, c3) = (self.p(x1, y1), self.p(x2, y2), self.p(x, y));
        if let Some(c) = &mut self.cur {
            c.segs.push(Seg::Cubic(a, b, c3));
        }
    }
    fn close(&mut self) {
        if let Some(c) = self.cur.take() {
            self.done.push(c);
        }
    }
}

/// Glyphs of one font face at an em of 2048 units, as Windows' `GetGlyphOutline` gives them.
pub struct TtfSource<'a> {
    face: Face<'a>,
    k: f64,
}

impl<'a> TtfSource<'a> {
    pub fn new(data: &'a [u8], index: u32) -> Option<Self> {
        let face = Face::parse(data, index).ok()?;
        let k = FONT_EM / face.units_per_em() as f64;
        Some(TtfSource { face, k })
    }
}

impl GlyphSource for TtfSource<'_> {
    fn metrics(&self) -> FontMetrics {
        let f = &self.face;
        let (win_asc, win_desc) = f
            .tables()
            .os2
            .map(|o| (o.windows_ascender() as f64, o.windows_descender().unsigned_abs() as f64))
            .unwrap_or((f.ascender() as f64, f.descender().unsigned_abs() as f64));
        FontMetrics {
            ascent: f.ascender() as f64 * self.k,
            descent: f.descender().unsigned_abs() as f64 * self.k,
            internal_leading: ((win_asc + win_desc) * self.k - FONT_EM).max(0.0),
            line_gap: f.line_gap() as f64 * self.k,
        }
    }

    fn glyph(&self, c: char) -> Glyph {
        let gid = match self.face.glyph_index(c) {
            Some(g) => g,
            None if c.is_whitespace() => return Glyph { advance: (FONT_EM / 4.0).round(), outlines: Vec::new() },
            None => GlyphId(0),
        };
        let advance = (self.face.glyph_hor_advance(gid).unwrap_or(0) as f64 * self.k).round();
        let mut b = Collect { k: self.k, done: Vec::new(), cur: None };
        self.face.outline_glyph(gid, &mut b);
        b.close();
        Glyph { advance, outlines: b.done }
    }
}

/// Runs `f` with the glyph source of `file`.
pub fn with_source<R>(file: &FontFile, f: impl FnOnce(&dyn GlyphSource) -> R) -> Option<R> {
    let data = std::fs::read(&file.path).ok()?;
    let src = TtfSource::new(&data, file.index)?;
    Some(f(&src))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arial() -> Option<FontFile> {
        font_list().file("Arial", false, false).cloned()
    }

    #[test]
    fn finds_the_system_fonts_and_their_styles() {
        let list = font_list();
        if list.families.is_empty() {
            return; // no fonts on this machine
        }
        if let Some(styles) = list.families.get("Arial") {
            assert!(styles.contains_key(&(false, false)));
            assert!(styles.contains_key(&(true, false)), "Arial Bold");
            assert!(styles.contains_key(&(false, true)), "Arial Italic");
        }
    }

    #[test]
    fn arial_metrics_match_windows() {
        let Some(file) = arial() else { return };
        with_source(&file, |s| {
            let m = s.metrics();
            // Arial at an em of 2048: ascent 1854, descent 434, gap 67.
            assert_eq!(m.ascent.round(), 1854.0);
            assert_eq!(m.descent.round(), 434.0);
            assert_eq!(m.line_gap.round(), 67.0);
            // 'H' is 1479 units wide and has one contour; a space has none.
            let h = s.glyph('H');
            assert_eq!(h.advance, 1479.0);
            assert_eq!(h.outlines.len(), 1);
            assert!(s.glyph(' ').outlines.is_empty());
            // 'O' has an outer contour and a hole.
            assert_eq!(s.glyph('O').outlines.len(), 2);
        })
        .expect("Arial loads");
    }
}
