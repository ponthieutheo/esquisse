//! Softimage dotXSI 3.0 text files (.xsi), as SketchUp's XSI exporter writes them: a model per
//! group or component with its SRT transform, meshes as `SI_Shape` arrays and
//! `SI_PolygonList`s per material, edges as degree-1 `SI_NurbsCurve`s, a material library.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;

use anyhow::Result;
use esq_core::glam::{DMat4, DVec2, DVec3, EulerRot};
use esq_core::model::{DefId, MaterialId, Model};

use crate::obj::{BACK_COLOR, FRONT_COLOR, MatNames, clean_name, face_uvs};
use crate::options::XsiExport;
use crate::scene::{Cache, Opts, WorldFace, sides};
use crate::stl::g;
use crate::tex::TexNames;

/// A polygon corner: position, normal and texture coordinate indices.
type Corner = (usize, usize, usize);

struct W<'a> {
    model: &'a Model,
    o: &'a XsiExport,
    out: String,
    depth: usize,
    /// Model units → file units (scale) and the optional Y-up swap, as one matrix.
    conv: DMat4,
    mats: MatNames,
    names: Vec<String>,
    meshes: usize,
    faces: usize,
    edges: usize,
}

impl W<'_> {
    fn line(&mut self, s: &str) {
        for _ in 0..self.depth {
            self.out.push('\t');
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn open(&mut self, s: &str) {
        self.line(s);
        self.depth += 1;
    }

    fn close(&mut self) {
        self.depth = self.depth.saturating_sub(1);
        self.line("}");
        if self.depth == 0 {
            self.out.push('\n');
        }
    }

    fn unique(&mut self, base: &str) -> String {
        let b = clean_name(base);
        let mut n = b.clone();
        let mut i = 1;
        while self.names.contains(&n) {
            n = format!("{b}_{i}");
            i += 1;
        }
        self.names.push(n.clone());
        n
    }

    fn srt(&mut self, name: &str, m: &DMat4) {
        let c = self.conv * *m * self.conv.inverse();
        let (s, r, t) = c.to_scale_rotation_translation();
        let (z, y, x) = r.to_euler(EulerRot::ZYX);
        self.open(&format!("SI_Transform SRT-{name} {{"));
        self.line(&format!("{},", g(s.x)));
        self.line(&format!("{},", g(s.y)));
        self.line(&format!("{},", g(s.z)));
        self.line(&format!("{},", g(x.to_degrees())));
        self.line(&format!("{},", g(y.to_degrees())));
        self.line(&format!("{},", g(z.to_degrees())));
        self.line(&format!("{},", g(t.x)));
        self.line(&format!("{},", g(t.y)));
        self.line(&format!("{},", g(t.z)));
        self.close();
    }

    /// One model: its transform, its mesh and edges, its child models.
    fn model_block(
        &mut self,
        cache: &mut Cache,
        def: Option<DefId>,
        xform: &DMat4,
        inherited: Option<MaterialId>,
        name: &str,
        depth: usize,
    ) {
        let name = self.unique(name);
        self.open(&format!("SI_Model MDL-{name} {{"));
        self.srt(&name, xform);
        self.open("SI_Visibility {");
        self.line("1,");
        self.close();
        let coll = cache.get(def).clone();
        if !coll.faces.is_empty() {
            self.mesh(&coll, inherited, &name);
        }
        if self.o.0.edges && !coll.edges.is_empty() {
            let cname = self.unique(&format!("{name}-Edges"));
            self.open(&format!("XSI_CurveList {cname} {{"));
            self.line(&format!("{},", coll.edges.len()));
            for (k, e) in coll.edges.iter().enumerate() {
                let (a, b) = (self.conv.transform_point3(e.a), self.conv.transform_point3(e.b));
                self.open(&format!("SI_NurbsCurve Edge{} {{", k + 1));
                self.line("1,");
                self.line("0,");
                self.line("2,");
                self.line("0,1,");
                self.line("2,");
                self.line(&format!("{},{},{},1,", g(a.x), g(a.y), g(a.z)));
                self.line(&format!("{},{},{},1", g(b.x), g(b.y), g(b.z)));
                self.close();
                self.edges += 1;
            }
            self.close();
        }
        if depth < 64 {
            for inst in &coll.instances {
                let inh = inst.material.or(inherited);
                let d = &self.model.defs[inst.def];
                let child = if inst.name.is_empty() { d.name.clone() } else { inst.name.clone() };
                let child = if d.is_group { format!("G_{child}") } else { child };
                self.model_block(cache, Some(inst.def), &inst.xform, inh, &child, depth + 1);
            }
        }
        self.close();
    }

    fn mesh(&mut self, coll: &crate::scene::Collection, inherited: Option<MaterialId>, name: &str) {
        self.meshes += 1;
        let mut pos: Vec<DVec3> = Vec::new();
        let mut nor: Vec<DVec3> = Vec::new();
        let mut uv: Vec<DVec2> = Vec::new();
        let mut pi: HashMap<[u64; 3], usize> = HashMap::new();
        let mut ni: HashMap<[u64; 3], usize> = HashMap::new();
        let mut ti: HashMap<[u64; 2], usize> = HashMap::new();
        // material → polygons as (pos, normal, uv) index lists
        let mut lists: Vec<(String, Vec<Vec<Corner>>)> = Vec::new();
        for face in &coll.faces {
            let wf = WorldFace {
                face,
                xform: DMat4::IDENTITY,
                front: face.front.or(inherited),
                back: face.back.or(inherited),
            };
            for &front in sides(self.o.0.two_sided) {
                let mat = if front { wf.front } else { wf.back };
                let mname = self.mats.get(self.model, mat, front);
                let uvs = face_uvs(self.model, &wf, mat, front);
                let n = self.conv.transform_vector3(if front { face.normal } else { -face.normal }).normalize_or_zero();
                let nk = n.to_array().map(f64::to_bits);
                let nidx = *ni.entry(nk).or_insert_with(|| {
                    nor.push(n);
                    nor.len() - 1
                });
                let pts: Vec<DVec3> = face.points().iter().map(|p| self.conv.transform_point3(*p)).collect();
                let corner = |i: usize,
                              pos: &mut Vec<DVec3>,
                              uv: &mut Vec<DVec2>,
                              pi: &mut HashMap<[u64; 3], usize>,
                              ti: &mut HashMap<[u64; 2], usize>| {
                    let p = pts[i];
                    let a = *pi.entry(p.to_array().map(f64::to_bits)).or_insert_with(|| {
                        pos.push(p);
                        pos.len() - 1
                    });
                    let t = uvs[i];
                    let b = *ti.entry(t.to_array().map(f64::to_bits)).or_insert_with(|| {
                        uv.push(t);
                        uv.len() - 1
                    });
                    (a, nidx, b)
                };
                let polys: Vec<Vec<usize>> = if !self.o.0.triangulate && face.is_simple() {
                    let mut l = wf.loops().remove(0);
                    if !front {
                        l.reverse();
                    }
                    vec![l]
                } else {
                    wf.tris().into_iter().map(|t| if front { t.to_vec() } else { vec![t[0], t[2], t[1]] }).collect()
                };
                for p in polys {
                    let c: Vec<(usize, usize, usize)> =
                        p.iter().map(|&i| corner(i, &mut pos, &mut uv, &mut pi, &mut ti)).collect();
                    self.faces += 1;
                    match lists.iter_mut().find(|l| l.0 == mname) {
                        Some(l) => l.1.push(c),
                        None => lists.push((mname.clone(), vec![c])),
                    }
                }
            }
        }
        self.open(&format!("SI_Mesh MSH-{name} {{"));
        self.open(&format!("SI_Shape SHP-{name}-ORG {{"));
        self.line("3,");
        self.line("\"ORDERED\",");
        self.line(&format!("{},", pos.len()));
        self.line("\"POSITION\",");
        for p in &pos {
            self.line(&format!("{},{},{},", g(p.x), g(p.y), g(p.z)));
        }
        self.line(&format!("{},", nor.len()));
        self.line("\"NORMAL\",");
        for n in &nor {
            self.line(&format!("{},{},{},", g(n.x), g(n.y), g(n.z)));
        }
        self.line(&format!("{},", uv.len()));
        self.line("\"TEX_COORD_UV\",");
        for t in &uv {
            self.line(&format!("{},{},", g(t.x), g(t.y)));
        }
        self.close();
        for (mname, polys) in lists {
            let lname = self.unique(&format!("{name}-{mname}"));
            self.open(&format!("SI_PolygonList {lname} {{"));
            self.line(&format!("{},", polys.len()));
            self.line("\"NORMAL|TEX_COORD_UV\",");
            self.line(&format!("\"{mname}\","));
            self.line(&format!("{},", polys.iter().map(Vec::len).sum::<usize>()));
            for p in &polys {
                self.line(&format!("{},", p.len()));
            }
            for k in 0..3 {
                for p in &polys {
                    let s: Vec<String> = p.iter().map(|c| [c.0, c.1, c.2][k].to_string()).collect();
                    self.line(&format!("{},", s.join(",")));
                }
            }
            self.close();
        }
        self.close();
    }
}

/// File › Export › 3D Model › XSI.
pub fn export(model: &Model, path: &Path, o: &XsiExport) -> Result<String> {
    let unit = o.0.units.mm(model.units);
    let swap = if o.0.swap_yz { crate::build::y_up_to_z_up().inverse() } else { DMat4::IDENTITY };
    let conv = swap * DMat4::from_scale(DVec3::splat(1.0 / unit));
    let mut cache = Cache::new(model, Opts::default());
    let processed = crate::report::processed(&mut cache, 0);
    let mut w = W {
        model,
        o,
        out: String::new(),
        depth: 0,
        conv,
        mats: MatNames::new(),
        names: Vec::new(),
        meshes: 0,
        faces: 0,
        edges: 0,
    };
    w.out.push_str("xsi 0300txt 0032\n\n");
    w.open("SI_FileInfo {");
    w.line("\"SoftImage dotXSI file, Exported from SketchUp, (c) 2000-2012 Trimble Navigation Limited\",");
    w.line("\"\",");
    w.line("\"\",");
    w.line("\"SketchUp\",");
    w.close();
    w.open("SI_Scene {");
    w.line("\"FRAMES\",");
    w.line("0,");
    w.line("100,");
    w.line("30,");
    w.close();
    w.open("SI_CoordinateSystem {");
    // Right-handed; U right, V up; X right, then Y up and Z out (Y up) or Y in and Z up.
    for v in if o.0.swap_yz { [1, 0, 1, 0, 2, 5] } else { [1, 0, 1, 0, 4, 2] } {
        w.line(&format!("{v},"));
    }
    w.close();
    w.open("SI_Angle {");
    w.line("0,");
    w.close();
    w.open("SI_Ambience {");
    w.line("0,");
    w.line("0,");
    w.line("0,");
    w.close();
    // Geometry first (it names the materials), then the library is put before it.
    let head = std::mem::take(&mut w.out);
    w.model_block(&mut cache, None, &DMat4::IDENTITY, None, "Model", 0);
    let body = std::mem::take(&mut w.out);
    let mut lib = String::new();
    let mut texnames = TexNames::new();
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut textures = 0;
    writeln!(lib, "SI_MaterialLibrary {{")?;
    writeln!(lib, "\t{},", w.mats.names.len())?;
    for (mat, front, name) in &w.mats.names {
        let m = mat.and_then(|m| model.materials.get(m));
        let (c, a) = match m {
            Some(m) => (m.color, m.opacity as f64),
            None if *front => (FRONT_COLOR, 1.0),
            None => (BACK_COLOR, 1.0),
        };
        let c = c.map(|x| x as f64 / 255.0);
        writeln!(lib, "\tSI_Material {name} {{")?;
        writeln!(lib, "\t\t{},{},{},{},", g(c[0]), g(c[1]), g(c[2]), g(a))?;
        writeln!(lib, "\t\t0,")?;
        writeln!(lib, "\t\t0,0,0,")?;
        writeln!(lib, "\t\t0,0,0,")?;
        writeln!(lib, "\t\t1,")?;
        writeln!(lib, "\t\t{},{},{},", g(c[0]), g(c[1]), g(c[2]))?;
        if o.0.texture_maps
            && let Some(t) = m.and_then(|m| m.ext.texture.as_ref())
        {
            let file = texnames.name_for(t);
            std::fs::write(dir.join(&file), &t.image.0[..])?;
            textures += 1;
            writeln!(lib, "\t\tSI_Texture2D {{")?;
            writeln!(lib, "\t\t\t\"{file}\",")?;
            writeln!(lib, "\t\t\t3,")?;
            writeln!(lib, "\t\t\t{},{},", t.px[0], t.px[1])?;
            writeln!(lib, "\t\t\t0,{},0,{},", t.px[0].saturating_sub(1), t.px[1].saturating_sub(1))?;
            writeln!(lib, "\t\t\t0,")?;
            writeln!(lib, "\t\t\t1,1,")?;
            writeln!(lib, "\t\t\t0,0,")?;
            writeln!(lib, "\t\t\t1,1,")?;
            writeln!(lib, "\t\t\t0,0,")?;
            writeln!(lib, "\t\t\t1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1,")?;
            writeln!(lib, "\t\t\t3,")?;
            writeln!(lib, "\t\t\t1,")?;
            writeln!(lib, "\t\t\t0.75,")?;
            writeln!(lib, "\t\t\t1,")?;
            writeln!(lib, "\t\t\t0,")?;
            writeln!(lib, "\t\t\t0,")?;
            writeln!(lib, "\t\t\t0,")?;
            writeln!(lib, "\t\t\t0,")?;
            writeln!(lib, "\t\t}}")?;
        }
        writeln!(lib, "\t}}")?;
    }
    writeln!(lib, "}}\n")?;
    std::fs::write(path, format!("{head}{lib}{body}"))?;
    Ok(crate::report::text(
        "XSI",
        &processed,
        &[
            ("Groupes", w.meshes),
            ("Faces", w.faces),
            ("arêtes", w.edges),
            ("Matières", w.mats.names.len()),
            ("Textures", textures),
        ],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::ObjExport;
    use esq_core::model::{Definition, Instance};

    #[test]
    fn writes_models_meshes_and_materials() {
        let mut m = Model::new();
        let mut sq = esq_core::model::Entities::default();
        let v: Vec<_> = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]
            .iter()
            .map(|(x, y)| sq.add_vertex_raw(DVec3::new(*x, *y, 0.0)))
            .collect();
        sq.add_face_raw(v, vec![]);
        let d = m.defs.insert(Definition {
            name: "Carreau".into(),
            is_group: false,
            description: String::new(),
            entities: sq,
            attributes: Default::default(),
            ext: Default::default(),
        });
        let red = m.add_material("Rouge", [255, 0, 0]);
        m.root.instances.insert(Instance {
            def: d,
            xform: DMat4::from_translation(DVec3::new(100.0, 0.0, 0.0)) * DMat4::from_rotation_z(90f64.to_radians()),
            name: String::new(),
            material: Some(red),
            layer: 0,
            hidden: false,
            locked: false,
            attributes: Default::default(),
            ext: Default::default(),
        });
        let dir = std::env::temp_dir().join(format!("esq_xsi_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("c.xsi");
        let rep = export(&m, &p, &XsiExport::default()).unwrap();
        // Compared without the indentation.
        let t = std::fs::read_to_string(&p).unwrap().replace('\t', "");
        assert!(t.starts_with("xsi 0300txt 0032\n"));
        assert!(t.contains("SI_Model MDL-Model {") && t.contains("SI_Model MDL-Carreau {"));
        assert!(t.contains("SI_Transform SRT-Carreau {\n1,\n1,\n1,\n0,\n0,\n90,\n100,"), "{t}");
        assert!(t.contains("SI_Material Rouge {\n1,0,0,1,"));
        // One quad, not triangulated by default.
        assert!(t.contains("\"Rouge\",\n4,\n4,"), "{t}");
        assert!(rep.contains("Entités XSI exportées"));
        let o = XsiExport(ObjExport { triangulate: true, swap_yz: true, ..Default::default() });
        export(&m, &p, &o).unwrap();
        let t = std::fs::read_to_string(&p).unwrap().replace('\t', "");
        assert!(t.contains("SI_CoordinateSystem {\n1,\n0,\n1,\n0,\n2,\n5,"));
        assert!(t.contains("\"Rouge\",\n6,"), "{t}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
