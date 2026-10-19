//! Wavefront OBJ, written like SketchUp's OBJ exporter: "# Alias OBJ Model File" header,
//! one `g Mesh…` group per collection, `v`/`vt`/`vn` then `usemtl` and faces, a `.mtl`
//! file, and the texture images in a folder named after the file.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;

use anyhow::Result;
use esq_core::glam::{DVec2, DVec3};
use esq_core::meta::side_uv;
use esq_core::model::{MaterialId, Model};

use crate::options::ObjExport;
use crate::scene::{Cache, Opts, WorldFace, placements, sides};
use crate::stl::g;
use crate::tex::TexNames;
use crate::units::LenUnit;

pub fn unit_word(u: LenUnit) -> &'static str {
    match u {
        LenUnit::Model | LenUnit::Inches => "inches",
        LenUnit::Feet => "feet",
        LenUnit::Yards => "yards",
        LenUnit::Miles => "miles",
        LenUnit::Millimeters => "millimeters",
        LenUnit::Centimeters => "centimeters",
        LenUnit::Meters => "meters",
        LenUnit::Kilometers => "kilometers",
    }
}

/// Names that other programs read without trouble (letters, digits, `_`, `-`).
pub fn clean_name(s: &str) -> String {
    let c: String =
        s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    if c.is_empty() { "_".into() } else { c }
}

/// The front and back colours SketchUp uses for faces without a material.
pub const FRONT_COLOR: [u8; 3] = [255, 255, 255];
pub const BACK_COLOR: [u8; 3] = [164, 164, 164];

/// Material slots of an export: one per (material, side) for default materials.
pub struct MatNames {
    pub names: Vec<(Option<MaterialId>, bool, String)>,
}

impl MatNames {
    pub fn new() -> Self {
        MatNames { names: Vec::new() }
    }

    /// The name used for `mat` seen from the `front` side (default material: FrontColor /
    /// BackColor).
    pub fn get(&mut self, model: &Model, mat: Option<MaterialId>, front: bool) -> String {
        let key_front = mat.is_some() || front;
        if let Some(x) = self.names.iter().find(|x| x.0 == mat && x.1 == key_front) {
            return x.2.clone();
        }
        let base = match mat.and_then(|m| model.materials.get(m)) {
            Some(m) => clean_name(&m.name),
            None if front => "FrontColor".into(),
            None => "BackColor".into(),
        };
        let mut name = base.clone();
        let mut i = 1;
        while self.names.iter().any(|x| x.2 == name) {
            name = format!("{base}_{i}");
            i += 1;
        }
        self.names.push((mat, key_front, name.clone()));
        name
    }
}

impl Default for MatNames {
    fn default() -> Self {
        Self::new()
    }
}

/// Texture coordinates of a face side, also for untextured faces (one-inch tiles, as
/// SketchUp writes them).
pub fn face_uvs(model: &Model, f: &WorldFace, mat: Option<MaterialId>, front: bool) -> Vec<DVec2> {
    f.face.uvs(model, mat, front).unwrap_or_else(|| {
        let map = side_uv(f.face.tex.as_ref(), front, f.face.normal, None);
        f.face.points().iter().map(|p| map.at(*p)).collect()
    })
}

struct Pool<K: std::hash::Hash + Eq> {
    map: HashMap<K, usize>,
    lines: String,
    count: usize,
}

impl<K: std::hash::Hash + Eq> Pool<K> {
    fn new() -> Self {
        Pool { map: HashMap::new(), lines: String::new(), count: 0 }
    }
    /// 1-based index of a value, writing it when new.
    fn index(&mut self, key: K, line: impl FnOnce() -> String) -> usize {
        if let Some(&i) = self.map.get(&key) {
            return i;
        }
        self.count += 1;
        self.lines.push_str(&line());
        self.map.insert(key, self.count);
        self.count
    }
}

fn key3(v: DVec3) -> [u64; 3] {
    [v.x.to_bits(), v.y.to_bits(), v.z.to_bits()]
}

/// Writes `path` (.obj), the .mtl beside it and the textures. Returns the results text.
pub fn export(model: &Model, path: &Path, o: &ObjExport) -> Result<String> {
    let unit = o.units.resolve(model.units);
    let k = unit.mm(model.units);
    let conv = |p: DVec3| {
        let p = p / k;
        if o.swap_yz { DVec3::new(p.x, p.z, -p.y) } else { p }
    };
    let convn = |n: DVec3| if o.swap_yz { DVec3::new(n.x, n.z, -n.y) } else { n };
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "model".into());
    let mtl_name = format!("{stem}.mtl");
    let mut out = String::new();
    writeln!(out, "# Alias OBJ Model File")?;
    writeln!(out, "# Exported from SketchUp, (c) 2000-2012 Trimble Navigation Limited")?;
    writeln!(out, "# File units = {}", unit_word(unit))?;
    writeln!(out)?;
    writeln!(out, "mtllib {mtl_name}")?;
    writeln!(out)?;
    let mut cache = Cache::new(model, Opts::default());
    let processed = crate::report::processed(&mut cache, 0);
    let mut mats = MatNames::new();
    let (mut v, mut vt, mut vn) = (Pool::new(), Pool::new(), Pool::new());
    let (mut groups, mut faces_out, mut edges_out) = (0, 0, 0);
    for (mesh_no, p) in placements(&mut cache).iter().enumerate() {
        let coll = cache.get(p.def).clone();
        if coll.faces.is_empty() && (!o.edges || coll.edges.is_empty()) {
            continue;
        }
        groups += 1;
        let mut names = vec![format!("Mesh{}", mesh_no + 1)];
        let mut ents = &model.root;
        let mut chain = Vec::new();
        for id in &p.path {
            if let Some(i) = ents.instances.get(*id) {
                let d = &model.defs[i.def];
                chain.push(clean_name(if i.name.is_empty() { &d.name } else { &i.name }));
                ents = &d.entities;
            }
        }
        chain.reverse();
        names.extend(chain);
        names.push("Model".into());
        writeln!(out, "g {}", names.join(" "))?;
        writeln!(out)?;
        // Faces by material: (name, face lines).
        let mut by_mat: Vec<(String, String)> = Vec::new();
        let (v0, vt0, vn0) = (v.lines.len(), vt.lines.len(), vn.lines.len());
        for face in &coll.faces {
            let wf =
                WorldFace { face, xform: p.xform, front: face.front.or(p.inherited), back: face.back.or(p.inherited) };
            let pts: Vec<DVec3> = wf.points().iter().map(|x| conv(*x)).collect();
            for &front in sides(o.two_sided) {
                let mat = if front { wf.front } else { wf.back };
                let name = mats.get(model, mat, front);
                let uvs = face_uvs(model, &wf, mat, front);
                let n = convn(if front { wf.normal() } else { -wf.normal() }).normalize_or_zero();
                let ni = vn.index(key3(n), || format!("vn {} {} {}\n", g(n.x), g(n.y), g(n.z)));
                let vi: Vec<usize> =
                    pts.iter().map(|q| v.index(key3(*q), || format!("v {} {} {}\n", g(q.x), g(q.y), g(q.z)))).collect();
                let ti: Vec<usize> = uvs
                    .iter()
                    .map(|t| vt.index([t.x.to_bits(), t.y.to_bits(), 0], || format!("vt {} {}\n", g(t.x), g(t.y))))
                    .collect();
                let corner = |i: usize| format!("{}/{}/{}", vi[i], ti[i], ni);
                let mut lines = String::new();
                let polys: Vec<Vec<usize>> = if !o.triangulate && face.is_simple() {
                    let mut l = wf.loops().remove(0);
                    if !front {
                        l.reverse();
                    }
                    vec![l]
                } else {
                    wf.tris().into_iter().map(|t| if front { t.to_vec() } else { vec![t[0], t[2], t[1]] }).collect()
                };
                for poly in polys {
                    faces_out += 1;
                    if poly.len() == 3 {
                        writeln!(lines, "f {} {} {}", corner(poly[0]), corner(poly[1]), corner(poly[2]))?;
                    } else {
                        lines.push_str("f ");
                        for i in poly {
                            lines.push_str(&corner(i));
                            lines.push(' ');
                        }
                        lines.push('\n');
                    }
                }
                match by_mat.iter_mut().find(|x| x.0 == name) {
                    Some(x) => x.1.push_str(&lines),
                    None => by_mat.push((name, lines)),
                }
            }
        }
        let mut line_list = String::new();
        if o.edges {
            for e in &coll.edges {
                let (a, b) = (conv(p.xform.transform_point3(e.a)), conv(p.xform.transform_point3(e.b)));
                let ia = v.index(key3(a), || format!("v {} {} {}\n", g(a.x), g(a.y), g(a.z)));
                let ib = v.index(key3(b), || format!("v {} {} {}\n", g(b.x), g(b.y), g(b.z)));
                writeln!(line_list, "l {ia} {ib}")?;
                edges_out += 1;
            }
        }
        out.push_str(&v.lines[v0..]);
        out.push_str(&vt.lines[vt0..]);
        out.push_str(&vn.lines[vn0..]);
        if !v.lines[v0..].is_empty() {
            writeln!(out)?;
        }
        for (name, lines) in by_mat {
            writeln!(out, "usemtl {name}")?;
            out.push_str(&lines);
            writeln!(out)?;
        }
        if !line_list.is_empty() {
            out.push_str(&line_list);
            writeln!(out)?;
        }
    }
    // Materials and textures.
    let mut mtl = String::new();
    writeln!(mtl, "## Alias OBJ Material File")?;
    writeln!(mtl, "# Exported from SketchUp, (c) 2000-2012 Trimble Navigation Limited")?;
    writeln!(mtl)?;
    let mut texnames = TexNames::new();
    let tex_dir = path.with_file_name(&stem);
    let mut textures = 0;
    for (mat, front, name) in &mats.names {
        let m = mat.and_then(|m| model.materials.get(m));
        let (c, d) = match m {
            Some(m) => (m.color, m.opacity),
            None if *front => (FRONT_COLOR, 1.0),
            None => (BACK_COLOR, 1.0),
        };
        writeln!(mtl, "newmtl {name}")?;
        writeln!(mtl, "Ka 0.000000 0.000000 0.000000")?;
        writeln!(mtl, "Kd {:.6} {:.6} {:.6}", c[0] as f64 / 255.0, c[1] as f64 / 255.0, c[2] as f64 / 255.0)?;
        writeln!(mtl, "Ks 0.330000 0.330000 0.330000")?;
        if d < 1.0 {
            writeln!(mtl, "d {:.6}", d)?;
        }
        if o.texture_maps
            && let Some(t) = m.and_then(|m| m.ext.texture.as_ref())
        {
            let file = texnames.name_for(t);
            std::fs::create_dir_all(&tex_dir)?;
            std::fs::write(tex_dir.join(&file), &t.image.0[..])?;
            writeln!(mtl, "map_Kd {stem}/{file}")?;
            textures += 1;
        }
        writeln!(mtl)?;
    }
    std::fs::write(path, out)?;
    std::fs::write(path.with_extension("mtl"), mtl)?;
    Ok(crate::report::text(
        "OBJ",
        &processed,
        &[
            ("Groupes", groups),
            ("Faces", faces_out),
            ("arêtes", edges_out),
            ("Matières", mats.names.len()),
            ("Textures", textures),
        ],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use esq_core::model::Units;

    fn square(m: &mut Model) {
        let e = &mut m.root;
        let v: Vec<_> = [(0.0, 0.0), (1000.0, 0.0), (1000.0, 1000.0), (0.0, 1000.0)]
            .iter()
            .map(|(x, y)| e.add_vertex_raw(DVec3::new(*x, *y, 0.0)))
            .collect();
        e.add_face_raw(v, vec![]);
    }

    #[test]
    fn writes_sketchups_layout_and_options() {
        let dir = std::env::temp_dir().join(format!("esq_obj_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut m = Model::new();
        m.units = Units::Meters;
        square(&mut m);
        let mat = m.add_material("Brique rouge", [200, 40, 30]);
        let f = m.root.faces.keys().next().unwrap();
        m.root.faces[f].front = Some(mat);
        let p = dir.join("carre.obj");
        let report = export(&m, &p, &ObjExport { two_sided: true, triangulate: true, ..Default::default() }).unwrap();
        let obj = std::fs::read_to_string(&p).unwrap();
        assert!(obj.starts_with("# Alias OBJ Model File\n# Exported from SketchUp"));
        assert!(obj.contains("# File units = meters"));
        assert!(obj.contains("mtllib carre.mtl"));
        assert!(obj.contains("g Mesh1 Model"));
        // Two-sided: front triangles with the brick, back ones with BackColor.
        assert_eq!(obj.lines().filter(|l| l.starts_with("f ")).count(), 4);
        assert!(obj.contains("usemtl Brique_rouge") && obj.contains("usemtl BackColor"));
        // Swap YZ (Y up) is on: the square lies in x/z, 1 m wide.
        assert!(obj.contains("v 1 0 -1"), "{obj}");
        let mtl = std::fs::read_to_string(p.with_extension("mtl")).unwrap();
        assert!(mtl.contains("newmtl Brique_rouge\nKa 0.000000 0.000000 0.000000\nKd 0.784314 0.156863 0.117647"));
        assert!(report.contains("Entités OBJ exportées"));
        // One-sided, untriangulated, with edges, in millimetres, Z up.
        let o = ObjExport {
            two_sided: false,
            triangulate: false,
            edges: true,
            swap_yz: false,
            units: LenUnit::Millimeters,
            ..Default::default()
        };
        export(&m, &p, &o).unwrap();
        let obj = std::fs::read_to_string(&p).unwrap();
        assert_eq!(obj.lines().filter(|l| l.starts_with("f ")).count(), 1);
        assert_eq!(obj.lines().filter(|l| l.starts_with("l ")).count(), 4);
        assert!(obj.contains("v 1000 1000 0"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
