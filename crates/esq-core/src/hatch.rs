//! Hatch patterns (Window › Hatch Patterns, section fills): AutoCAD .pat definitions, the
//! binary form SketchUp keeps in `.skh` files (`patData`), and the drawing of a pattern.
//!
//! A .pat pattern is a set of line families: each line is drawn at `angle` through
//! `origin`, the next lines of the family are `delta` away (along the line, across it), and
//! `dashes` alternate dash (positive) and gap (negative) lengths; no dashes is a full line.
//! SketchUp stores each family as floats: angle, origin x, y, delta x, y, the number of
//! dash/gap pairs, then the pairs (a full line being one pair 1, 0), read from the shipped
//! ANSI31 and CROSS patterns.

use crate::photo::{base64_decode, base64_encode};

#[derive(Clone, Debug, PartialEq)]
pub struct PatLine {
    /// Degrees.
    pub angle: f64,
    pub origin: [f64; 2],
    /// Offset to the next line of the family: along the line, across it.
    pub delta: [f64; 2],
    /// Dash (> 0) and gap (< 0) lengths; empty for a full line.
    pub dashes: Vec<f64>,
}

/// Whether file bytes are an image rather than a .pat text.
pub fn looks_like_image(b: &[u8]) -> bool {
    b.starts_with(&[0x89, b'P', b'N', b'G'])
        || b.starts_with(&[0xFF, 0xD8])
        || b.starts_with(b"BM")
        || b.starts_with(b"II*\0")
        || b.starts_with(b"MM\0*")
        || b.starts_with(b"8BPS")
}

/// Reads a .pat file: the first pattern's name and its lines.
pub fn parse_pat(text: &str) -> Result<(String, Vec<PatLine>), String> {
    let mut name = String::new();
    let mut lines = Vec::new();
    for raw in text.lines() {
        let l = raw.split(';').next().unwrap_or("").trim();
        if l.is_empty() {
            continue;
        }
        if let Some(h) = l.strip_prefix('*') {
            if !name.is_empty() || !lines.is_empty() {
                break;
            }
            name = h.split(',').next().unwrap_or("").trim().to_string();
            continue;
        }
        let v: Result<Vec<f64>, _> = l.split(',').map(|x| x.trim().parse::<f64>()).collect();
        let v = v.map_err(|_| "Contenu du fichier pat non valide.".to_string())?;
        if v.len() < 5 {
            return Err("Contenu du fichier pat non valide.".into());
        }
        lines.push(PatLine { angle: v[0], origin: [v[1], v[2]], delta: [v[3], v[4]], dashes: v[5..].to_vec() });
    }
    if lines.is_empty() {
        return Err("Contenu du fichier pat non valide.".into());
    }
    Ok((name, lines))
}

/// A .pat text for one pattern.
pub fn pat_text(name: &str, lines: &[PatLine]) -> String {
    let f = |x: f64| {
        let s = format!("{x:.6}");
        let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
        if s == "-0" { "0".into() } else { s }
    };
    let mut out = format!("*{name}\n");
    for l in lines {
        let mut parts = vec![f(l.angle), f(l.origin[0]), f(l.origin[1]), f(l.delta[0]), f(l.delta[1])];
        parts.extend(l.dashes.iter().map(|d| f(*d)));
        out.push_str(&parts.join(", "));
        out.push('\n');
    }
    out
}

/// SketchUp's `patData` (base64 of little-endian floats) for these lines.
pub fn pat_data(lines: &[PatLine]) -> String {
    let mut fl: Vec<f32> = Vec::new();
    for l in lines {
        fl.extend([l.angle, l.origin[0], l.origin[1], l.delta[0], l.delta[1]].map(|x| x as f32));
        let pairs: Vec<(f64, f64)> = if l.dashes.is_empty() {
            vec![(1.0, 0.0)]
        } else {
            l.dashes.chunks(2).map(|c| (c[0].abs(), c.get(1).copied().unwrap_or(0.0).abs())).collect()
        };
        fl.push(pairs.len() as f32);
        for (a, b) in pairs {
            fl.extend([a as f32, b as f32]);
        }
    }
    let bytes: Vec<u8> = fl.iter().flat_map(|x| x.to_le_bytes()).collect();
    base64_encode(&bytes)
}

/// Lines from SketchUp's `patData`.
pub fn parse_pat_data(b64: &str) -> Result<Vec<PatLine>, String> {
    let bytes = base64_decode(b64).ok_or("Contenu de tampon non valide.")?;
    let fl: Vec<f64> = bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c) as f64).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < fl.len() {
        if i + 6 > fl.len() {
            return Err("Contenu de tampon non valide.".into());
        }
        let n = fl[i + 5];
        // Saturating: a count of 3e38 made `2 * n` overflow.
        if n < 0.0 || n.fract() != 0.0 || (n as usize).saturating_mul(2).saturating_add(i + 6) > fl.len() {
            return Err("Nombre de segments non valide.".into());
        }
        let n = n as usize;
        let pairs = &fl[i + 6..i + 6 + 2 * n];
        let full = n == 1 && pairs[1] == 0.0;
        let dashes = if full { Vec::new() } else { pairs.chunks(2).flat_map(|p| [p[0], -p[1]]).collect() };
        // .pat keeps plain decimals: round the float32 noise away.
        let r = |x: f64| (x * 1e6).round() / 1e6;
        out.push(PatLine {
            angle: r(fl[i]),
            origin: [r(fl[i + 1]), r(fl[i + 2])],
            delta: [r(fl[i + 3]), r(fl[i + 4])],
            dashes: dashes.into_iter().map(r).collect(),
        });
        i += 6 + 2 * n;
    }
    if out.is_empty() {
        return Err("Contenu de tampon non valide.".into());
    }
    Ok(out)
}

/// Whether point `p` (pattern units) is on a line of the pattern, lines being `half` wide
/// on each side.
pub fn covers(lines: &[PatLine], p: [f64; 2], half: f64) -> bool {
    lines.iter().any(|l| {
        let a = l.angle.to_radians();
        let (u, n) = ([a.cos(), a.sin()], [-a.sin(), a.cos()]);
        let d = [p[0] - l.origin[0], p[1] - l.origin[1]];
        let (s, t) = (d[0] * u[0] + d[1] * u[1], d[0] * n[0] + d[1] * n[1]);
        if l.delta[1].abs() < 1e-12 {
            return t.abs() <= half && dash_on(&l.dashes, s);
        }
        let k = (t / l.delta[1]).round();
        (t - k * l.delta[1]).abs() <= half && dash_on(&l.dashes, s - k * l.delta[0])
    })
}

fn dash_on(dashes: &[f64], s: f64) -> bool {
    let total: f64 = dashes.iter().map(|d| d.abs()).sum();
    if dashes.is_empty() || total < 1e-12 {
        return true;
    }
    let mut x = s.rem_euclid(total);
    for d in dashes {
        if x <= d.abs() {
            return *d >= 0.0;
        }
        x -= d.abs();
    }
    false
}

/// A square picture (`side` pixels, RGBA) of a pattern: its colour as background, its lines
/// in black, `extent` pattern units across, turned by `rotation` degrees and scaled by
/// `scale`.
pub fn picture(lines: &[PatLine], color: [u8; 3], rotation: f64, scale: [f64; 2], side: u32, extent: f64) -> Vec<u8> {
    let mut out = Vec::with_capacity((side * side * 4) as usize);
    let (c, s) = (rotation.to_radians().cos(), rotation.to_radians().sin());
    let px = extent / side as f64;
    let (sx, sy) = (scale[0].max(1e-9), scale[1].max(1e-9));
    for j in 0..side {
        for i in 0..side {
            let x = (i as f64 + 0.5) * px;
            let y = (side as f64 - j as f64 - 0.5) * px;
            // Back into the pattern's own units: unrotate, unscale.
            let (rx, ry) = (x * c + y * s, -x * s + y * c);
            let p = [rx / sx, ry / sy];
            let on = covers(lines, p, 0.6 * px / sx.min(sy));
            let rgb = if on { [0, 0, 0] } else { color };
            out.extend([rgb[0], rgb[1], rgb[2], 255]);
        }
    }
    out
}

/// A typical size (pattern units) to show a few repeats of a pattern.
pub fn natural_extent(lines: &[PatLine]) -> f64 {
    let mut e: f64 = 0.0;
    for l in lines {
        e = e.max(l.delta[1].abs() * 4.0).max(l.dashes.iter().map(|d| d.abs()).sum::<f64>() * 2.0);
    }
    if e > 1e-9 { e } else { 1.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pat_files_and_sketchup_pat_data() {
        let (name, lines) =
            parse_pat("; motifs\n*CROSS, Croix\n0, 0,0, .25,.25, .125,-.375\n90, .0625,-.0625, .25,.25, .125,-.375\n")
                .unwrap();
        assert_eq!(name, "CROSS");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].origin, [0.0625, -0.0625]);
        // As SketchUp writes it in CROSS.skh.
        assert_eq!(
            pat_data(&lines),
            "AAAAAAAAAAAAAAAAAACAPgAAgD4AAIA/AAAAPgAAwD4AALRCAACAPQAAgL0AAIA+AACAPgAAgD8AAAA+AADAPg=="
        );
        assert_eq!(parse_pat_data(&pat_data(&lines)).unwrap(), lines);
        // A full line: one pair 1, 0 (ANSI31.skh).
        let (_, ansi) = parse_pat("*ANSI31\n45, 0,0, 0,.125\n").unwrap();
        assert_eq!(pat_data(&ansi), "AAA0QgAAAAAAAAAAAAAAAAAAAD4AAIA/AACAPwAAAAA=");
        assert_eq!(parse_pat_data("AAA0QgAAAAAAAAAAAAAAAAAAAD4AAIA/AACAPwAAAAA=").unwrap(), ansi);
        assert!(parse_pat("rien").is_err());
        assert!(parse_pat_data("AAAA").is_err());
        let back = parse_pat(&pat_text("CROSS", &lines)).unwrap();
        assert_eq!(back, ("CROSS".to_string(), lines));
        // A dash count of f32::MAX in the patData of a .skh: `2 * n` overflowed (a panic of
        // the debug builds); refused.
        let mut fl = vec![0f32; 5];
        fl.push(f32::MAX);
        let b64 = crate::photo::base64_encode(&fl.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>());
        assert!(parse_pat_data(&b64).is_err());
    }

    #[test]
    fn lines_and_dashes_are_drawn() {
        let (_, ansi) = parse_pat("*ANSI31\n45, 0,0, 0,.125\n").unwrap();
        assert!(covers(&ansi, [0.3, 0.3], 0.01));
        assert!(!covers(&ansi, [0.3, 0.3 + 0.0884], 0.01), "entre deux lignes");
        let (_, dashed) = parse_pat("*D\n0, 0,0, 0,1, .25,-.75\n").unwrap();
        assert!(covers(&dashed, [0.1, 0.0], 0.01) && !covers(&dashed, [0.5, 0.0], 0.01));
        let img = picture(&ansi, [255, 255, 255], 0.0, [1.0, 1.0], 32, 0.5);
        let dark = img.chunks(4).filter(|p| p[0] == 0).count();
        assert!(dark > 30 && dark < 32 * 32 / 2, "{dark}");
        assert!(looks_like_image(&[0x89, b'P', b'N', b'G']) && !looks_like_image(b"*A\n0,0,0,0,1"));
    }
}
