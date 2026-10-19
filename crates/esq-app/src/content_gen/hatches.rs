//! The hatch patterns of section fills: as many as SketchUp 2026 ships (12, `Default
//! Hatching`), under the usual names of drafting hatches (ANSI31 to ANSI38, CLAY, CROSS,
//! EARTH, STEEL). The line families below are written here (angle, origin, offset, dashes, in
//! inches as .pat files count), after what each name stands for in drafting.

use esq_core::meta::HatchPattern;

pub const COUNT: usize = 12;

const PATTERNS: [(&str, &str); 12] = [
    // Iron, brick, stone masonry: thin lines at 45°.
    ("ANSI31", "45, 0,0, 0,0.125"),
    // Steel: pairs of lines at 45°.
    ("ANSI32", "45, 0,0, 0,0.375\n45, 0.176777,0, 0,0.375"),
    // Bronze, brass: lines at 45°, every other one dashed.
    ("ANSI33", "45, 0,0, 0,0.25\n45, 0.176777,0, 0,0.25, 0.125,-0.0625"),
    // Plastic, rubber: groups of four lines at 45°.
    ("ANSI34", "45, 0,0, 0,0.75\n45, 0.176777,0, 0,0.75\n45, 0.353553,0, 0,0.75\n45, 0.53033,0, 0,0.75"),
    // Refractory brick: a line and a dash-dot line at 45°.
    ("ANSI35", "45, 0,0, 0,0.25\n45, 0.176777,0, 0,0.25, 0.3125,-0.0625,0,-0.0625"),
    // Marble, slate, glass: dash-dot lines at 45°.
    ("ANSI36", "45, 0,0, 0.21875,0.125, 0.3125,-0.0625,0,-0.0625"),
    // Lead, zinc: crossed lines at 45° and 135°.
    ("ANSI37", "45, 0,0, 0,0.125\n135, 0,0, 0,0.125"),
    // Aluminium: lines at 45° crossed by dashes at 135°.
    ("ANSI38", "45, 0,0, 0,0.125\n135, 0,0, 0.25,0.125, 0.3125,-0.1875"),
    // Clay: groups of three lines and a dashed line.
    ("CLAY", "0, 0,0, 0,0.1875\n0, 0,0.03125, 0,0.1875\n0, 0,0.0625, 0,0.1875\n0, 0,0.125, 0,0.1875, 0.1875,-0.125"),
    // Little crosses.
    ("CROSS", "0, 0,0, 0.25,0.25, 0.125,-0.375\n90, 0.0625,-0.0625, 0.25,0.25, 0.125,-0.375"),
    // Earth: short horizontal and vertical strokes in turn.
    (
        "EARTH",
        "0, 0,0, 0.25,0.25, 0.25,-0.25\n0, 0,0.09375, 0.25,0.25, 0.25,-0.25\n0, 0,0.1875, 0.25,0.25, 0.25,-0.25\n90, 0.03125,0.21875, 0.25,0.25, 0.25,-0.25\n90, 0.125,0.21875, 0.25,0.25, 0.25,-0.25\n90, 0.21875,0.21875, 0.25,0.25, 0.25,-0.25",
    ),
    // Steel (sections): lines at 45° with a dashed line between.
    ("STEEL", "45, 0,0, 0,0.125\n45, 0,0.0625, 0,0.125, 0.0625,-0.0625"),
];

/// (file stem, bytes) of each pattern.
pub fn all() -> Vec<(String, Vec<u8>)> {
    PATTERNS
        .iter()
        .enumerate()
        .map(|(i, (name, lines))| {
            let pat = format!("*{name}\n{lines}\n");
            let mut h = HatchPattern::new(i as u32 + 1, name, &format!("{name}.pat"), pat.into_bytes());
            h.size = [14.0, 14.0];
            let thumb = crate::skh::png(crate::hatch_panel::thumbnail(&h, 64), 64);
            (name.to_string(), crate::skh::write(&h, &thumb).expect("motif de hachures"))
        })
        .collect()
}
