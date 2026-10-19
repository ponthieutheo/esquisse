//! VRML 2.0 (.wrl), written like SketchUp's VRML exporter: inches scaled to metres by a root
//! `Transform` (turned to VRML's Y-up orientation on request), one `Group` per collection, a
//! `Shape` per material with `DEF`/`USE` appearances, components `DEF`'d once and `USE`'d,
//! edges as `IndexedLineSet`s and the scenes as `Viewpoint`s.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;

use anyhow::Result;
use esq_core::glam::{DMat3, DMat4, DQuat, DVec2, DVec3};
use esq_core::model::{DefId, MaterialId, Model, SceneCamera};

use crate::obj::{BACK_COLOR, FRONT_COLOR, clean_name};
use crate::options::VrmlExport;
use crate::scene::{Cache, Collection, Opts, WorldFace};
use crate::tex::TexNames;

const IN: f64 = 25.4;

/// A material seen from one side.
type Look = (Option<MaterialId>, bool);

fn f(x: f64) -> String {
    let s = format!("{x:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() { "0".into() } else { s.to_string() }
}

fn v3(v: DVec3) -> String {
    format!("{} {} {}", f(v.x), f(v.y), f(v.z))
}

struct W<'a> {
    model: &'a Model,
    o: &'a VrmlExport,
    out: String,
    depth: usize,
    /// Appearance names already DEF'd: (material, side) → name.
    looks: HashMap<(Option<MaterialId>, bool), String>,
    /// Component bodies already DEF'd: (definition, override material) → name.
    comps: HashMap<(DefId, Option<MaterialId>), String>,
    names: Vec<String>,
    texnames: TexNames,
    textures: Vec<(String, Vec<u8>)>,
    groups: usize,
    shapes: usize,
}

impl W<'_> {
    fn line(&mut self, s: &str) {
        for _ in 0..self.depth {
            self.out.push_str("  ");
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn open(&mut self, s: &str) {
        self.line(s);
        self.depth += 1;
    }

    fn close(&mut self, s: &str) {
        self.depth = self.depth.saturating_sub(1);
        self.line(s);
    }

    /// A VRML name, unique in the file.
    fn name(&mut self, prefix: &str, base: &str) -> String {
        let b = format!("{prefix}{}", clean_name(base));
        let mut n = b.clone();
        let mut i = 1;
        while self.names.contains(&n) {
            n = format!("{b}_{i}");
            i += 1;
        }
        self.names.push(n.clone());
        n
    }

    fn appearance(&mut self, mat: Option<MaterialId>, front: bool) {
        let key = (mat, mat.is_some() || front);
        if let Some(n) = self.looks.get(&key).cloned() {
            self.line(&format!("appearance USE {n}"));
            return;
        }
        let m = mat.and_then(|m| self.model.materials.get(m));
        let tex = m.and_then(|m| m.ext.texture.as_ref()).filter(|_| self.o.texture_maps);
        let base = m.map_or(if front { "FrontColor" } else { "BackColor" }, |m| m.name.as_str()).to_string();
        let n = self.name(if tex.is_some() { "TEX_" } else { "COL_" }, &base);
        self.looks.insert(key, n.clone());
        let c = m.map_or(if front { FRONT_COLOR } else { BACK_COLOR }, |m| m.color);
        let c = format!("{} {} {}", f(c[0] as f64 / 255.0), f(c[1] as f64 / 255.0), f(c[2] as f64 / 255.0));
        self.open(&format!("appearance DEF {n} Appearance {{"));
        match m.map(|m| m.opacity).filter(|o| *o < 1.0) {
            Some(op) => {
                self.line(&format!("material Material {{ diffuseColor {c} transparency {} }}", f(1.0 - op as f64)))
            }
            None => self.line(&format!("material Material {{ diffuseColor {c} }}")),
        }
        if let Some(t) = tex {
            let file = self.texnames.name_for(t);
            self.textures.push((file.clone(), t.image.0.to_vec()));
            self.open("texture ImageTexture {");
            self.line(&format!("url [\"{file}\"]"));
            self.close("}");
        }
        self.close("}");
    }

    /// Shapes of one collection's faces, grouped by material and side, in local coordinates
    /// (or through `xform` when the collection is written in place).
    fn faces(&mut self, coll: &Collection, xform: DMat4, inherited: Option<MaterialId>) {
        // (material, front?) → faces
        let mut by: Vec<(Look, Vec<&crate::scene::XFace>)> = Vec::new();
        for face in &coll.faces {
            let front_mat = face.front.or(inherited);
            let back_mat = face.back.or(inherited);
            let mut push = |k: (Option<MaterialId>, bool)| match by.iter_mut().find(|x| x.0 == k) {
                Some(x) => x.1.push(face),
                None => by.push((k, vec![face])),
            };
            push((front_mat, true));
            if !self.o.ignore_back_material {
                push((back_mat, false));
            }
        }
        for ((mat, front), faces) in by {
            self.shapes += 1;
            self.open("Shape {");
            self.appearance(mat, front);
            self.open("geometry IndexedFaceSet {");
            self.line("convex FALSE");
            // Ignoring the back material, the faces show from both sides.
            self.line(&format!("solid {}", if self.o.ignore_back_material { "FALSE" } else { "TRUE" }));
            let mut pts: Vec<DVec3> = Vec::new();
            let mut uvs: Vec<DVec2> = Vec::new();
            let mut idx: Vec<Vec<usize>> = Vec::new();
            let textured = self.o.texture_maps
                && mat.and_then(|m| self.model.materials.get(m)).is_some_and(|m| m.ext.texture.is_some());
            for face in faces {
                let wf = WorldFace { face, xform, front: mat, back: mat };
                let p = wf.points();
                let uv = if textured { face.uvs(self.model, mat, front) } else { None };
                let base = pts.len();
                pts.extend(p.iter().map(|x| *x / IN));
                if let Some(u) = uv {
                    uvs.extend(u);
                } else if textured {
                    uvs.extend(std::iter::repeat_n(DVec2::ZERO, p.len()));
                }
                let loops = wf.loops();
                if face.is_simple() {
                    let mut l: Vec<usize> = loops[0].iter().map(|i| base + i).collect();
                    if !front {
                        l.reverse();
                    }
                    idx.push(l);
                } else {
                    for t in wf.tris() {
                        let t = if front { t } else { [t[0], t[2], t[1]] };
                        idx.push(t.iter().map(|i| base + i).collect());
                    }
                }
            }
            self.open("coord Coordinate {");
            self.open("point [");
            for p in &pts {
                self.line(&format!("{},", v3(*p)));
            }
            self.close("]");
            self.close("}");
            self.open("coordIndex [");
            for l in &idx {
                let s: Vec<String> = l.iter().map(|i| i.to_string()).collect();
                self.line(&format!("{} -1", s.join(" ")));
            }
            self.close("]");
            if textured {
                self.open("texCoord TextureCoordinate {");
                self.open("point [");
                for t in &uvs {
                    self.line(&format!("{} {},", f(t.x), f(t.y)));
                }
                self.close("]");
                self.close("}");
            }
            self.close("}");
            self.close("}");
        }
        if self.o.edges {
            for e in &coll.edges {
                self.open("Shape {");
                self.line("appearance Appearance { material Material { diffuseColor 0 0 0 } }");
                self.open("geometry IndexedLineSet {");
                let (a, b) = (xform.transform_point3(e.a) / IN, xform.transform_point3(e.b) / IN);
                self.line(&format!("coord Coordinate {{ point [ {}, {} ] }}", v3(a), v3(b)));
                self.line("coordIndex [ 0 1 -1]");
                self.close("}");
                self.close("}");
            }
        }
    }

    /// A collection: its shapes and its instances.
    fn body(
        &mut self,
        cache: &mut Cache,
        def: Option<DefId>,
        xform: DMat4,
        inherited: Option<MaterialId>,
        depth: usize,
    ) {
        let coll = cache.get(def).clone();
        self.faces(&coll, xform, inherited);
        if depth > 64 {
            return;
        }
        for inst in &coll.instances {
            let inh = if self.o.material_overrides { inst.material.or(inherited) } else { inherited };
            let mirrored = inst.xform.determinant() < 0.0;
            if mirrored && !self.o.allow_mirrored {
                // Written in place, already mirrored.
                self.body(cache, Some(inst.def), xform * inst.xform, inh, depth + 1);
                continue;
            }
            let (scale, rot, trans) = decompose(&inst.xform);
            self.open("Transform {");
            self.line(&format!("translation {}", v3(trans / IN)));
            let (axis, angle) = rot.to_axis_angle();
            self.line(&format!("rotation {} {}", v3(axis), f(angle)));
            self.line(&format!("scale {}", v3(scale)));
            let key = (inst.def, inh);
            if let Some(n) = self.comps.get(&key).cloned() {
                self.line(&format!("children [USE {n}]"));
            } else {
                let n = self.name("CMP_", &self.model.defs[inst.def].name.clone());
                self.comps.insert(key, n.clone());
                self.open(&format!("children [DEF {n} Group {{"));
                self.groups += 1;
                self.open("children [");
                self.body(cache, Some(inst.def), DMat4::IDENTITY, inh, depth + 1);
                self.close("]");
                self.close("}]");
            }
            self.close("}");
        }
    }

    fn viewpoint(&mut self, c: &SceneCamera, name: &str) {
        let dir = (c.target - c.eye).normalize_or(DVec3::NEG_Y);
        let right = dir.cross(c.up).normalize_or(DVec3::X);
        let up = right.cross(dir);
        let q = DQuat::from_mat3(&DMat3::from_cols(right, up, -dir));
        let (axis, angle) = q.to_axis_angle();
        self.open("Viewpoint {");
        self.line(&format!("position {}", v3(c.eye / IN)));
        self.line(&format!("orientation {} {}", v3(axis), f(angle)));
        self.line(&format!("fieldOfView {}", f(c.fov_deg.to_radians())));
        self.line(&format!("description \"{}\"", name.replace('"', "'")));
        self.line("jump FALSE");
        self.close("}");
    }
}

/// Scale, rotation and translation of an instance (scale may be negative on one axis).
fn decompose(m: &DMat4) -> (DVec3, DQuat, DVec3) {
    let (s, r, t) = m.to_scale_rotation_translation();
    (s, r, t)
}

/// File › Export › 3D Model › VRML.
pub fn export(model: &Model, path: &Path, o: &VrmlExport, cx: &crate::ExportCx) -> Result<String> {
    let mut cache = Cache::new(model, Opts::default());
    let cameras = if o.cameras { model.scenes.len() + cx.view.is_some() as usize } else { 0 };
    let processed = crate::report::processed(&mut cache, cameras);
    let mut w = W {
        model,
        o,
        out: String::from("#VRML V2.0 utf8\n\n"),
        depth: 0,
        looks: HashMap::new(),
        comps: HashMap::new(),
        names: Vec::new(),
        texnames: TexNames::new(),
        textures: Vec::new(),
        groups: 0,
        shapes: 0,
    };
    w.open("Transform {");
    if o.vrml_orientation {
        w.line("rotation 1 0 0 -1.570796");
    }
    w.line(&format!("scale {:.6} {:.6} {:.6}", IN / 1000.0, IN / 1000.0, IN / 1000.0));
    w.open("children [");
    let root = w.name("GRP_", "SketchUp");
    w.open(&format!("DEF {root} Group {{"));
    w.groups += 1;
    w.open("children [");
    w.body(&mut cache, None, DMat4::IDENTITY, None, 0);
    w.close("]");
    w.close("}");
    if o.cameras {
        if let Some(v) = &cx.view {
            w.viewpoint(v, "Last Saved SketchUp View");
        }
        for s in &model.scenes {
            w.viewpoint(&s.camera, &s.name);
        }
    }
    w.close("]");
    w.close("}");
    let mut text = std::mem::take(&mut w.out);
    let _ = writeln!(text);
    std::fs::write(path, text)?;
    let dir = path.parent().unwrap_or(Path::new("."));
    for (name, data) in &w.textures {
        std::fs::write(dir.join(name), data)?;
    }
    Ok(crate::report::text(
        "VRML",
        &processed,
        &[
            ("Groupes", w.groups),
            ("Faces", w.shapes),
            ("arêtes", if o.edges { processed.edges } else { 0 }),
            ("Matières", w.looks.len()),
            ("Textures", w.textures.len()),
            ("Caméras", cameras),
        ],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use esq_core::model::{Definition, Instance};

    #[test]
    fn writes_groups_def_use_and_viewpoints() {
        let mut m = Model::new();
        let mut sq = esq_core::model::Entities::default();
        let v: Vec<_> = [(0.0, 0.0), (254.0, 0.0), (254.0, 254.0), (0.0, 254.0)]
            .iter()
            .map(|(x, y)| sq.add_vertex_raw(DVec3::new(*x, *y, 0.0)))
            .collect();
        sq.add_face_raw(v, vec![]);
        let d = m.defs.insert(Definition {
            name: "Dalle".into(),
            is_group: false,
            description: String::new(),
            entities: sq,
            attributes: Default::default(),
            ext: Default::default(),
        });
        for (x, sx) in [(0.0, 1.0), (1000.0, 1.0), (2000.0, -1.0)] {
            m.root.instances.insert(Instance {
                def: d,
                xform: DMat4::from_translation(DVec3::new(x, 0.0, 0.0)) * DMat4::from_scale(DVec3::new(sx, 1.0, 1.0)),
                name: String::new(),
                material: None,
                layer: 0,
                hidden: false,
                locked: false,
                attributes: Default::default(),
                ext: Default::default(),
            });
        }
        let dir = std::env::temp_dir().join(format!("esq_vrml_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("dalles.wrl");
        let view = SceneCamera {
            eye: DVec3::new(0.0, -5000.0, 1000.0),
            target: DVec3::ZERO,
            up: DVec3::Z,
            fov_deg: 35.0,
            perspective: true,
        };
        let rep = export(&m, &p, &VrmlExport::default(), &crate::ExportCx { view: Some(view), name: "dalles".into() })
            .unwrap();
        let t = std::fs::read_to_string(&p).unwrap();
        assert!(t.starts_with("#VRML V2.0 utf8\n"));
        assert!(t.contains("scale 0.025400 0.025400 0.025400"));
        assert!(!t.contains("rotation 1 0 0 -1.570796"), "orientation VRML désactivée par défaut");
        // Defined once, used again; the mirrored copy is written in place.
        assert_eq!(t.matches("children [DEF CMP_Dalle Group {").count(), 1, "{t}");
        assert_eq!(t.matches("children [USE CMP_Dalle]").count(), 1);
        assert!(t.contains("Viewpoint {") && t.contains("description \"Last Saved SketchUp View\""));
        assert!(t.contains("point [\n") && t.contains("10 10 0,"));
        assert!(rep.contains("Entités VRML exportées"));
        let o = VrmlExport { vrml_orientation: true, allow_mirrored: true, ..Default::default() };
        export(&m, &p, &o, &crate::ExportCx::default()).unwrap();
        let t = std::fs::read_to_string(&p).unwrap();
        assert!(t.contains("rotation 1 0 0 -1.570796"));
        assert_eq!(t.matches("children [USE CMP_Dalle]").count(), 2);
        std::fs::remove_dir_all(&dir).ok();
    }
}
