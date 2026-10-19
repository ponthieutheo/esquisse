//! The MFC archive of SketchUp 3 to 2020, as SketchUp 2026 writes each of these versions
//! (`docs/skp-format.md`, §3 and §8.2): the storing branch of each class's `Serialize`, read
//! in SketchUp.exe (for the model), field by field in the schema the version's
//! map gives the class. What a version cannot hold is left out as SketchUp leaves it out (tag
//! folders before 2020, line styles before 2019, dictionaries and texture positions in
//! SketchUp 3…); the values SketchUp writes for what Esquisse does not have are those of a new
//! model.

use super::mfc::Ar;
use super::plan::*;
use super::versions;
use crate::SkpVersion;

/// Persistent ids of the objects that have none in the plan (style, fonts…).
const STYLE_ID: u64 = 1;

pub fn file(p: &Plan, v: SkpVersion) -> Vec<u8> {
    traced(p, v, false).bytes
}

/// A file and where its objects are (format study).
pub struct Traced {
    pub bytes: Vec<u8>,
    /// Offsets in the file and what starts there.
    pub trace: Vec<(usize, String)>,
    /// Ranges of the file that differ from one writing to the next (GUIDs, times).
    pub variable: Vec<(usize, usize)>,
    /// The classes of the objects written (the version map aside), sorted.
    pub classes: Vec<&'static str>,
}

pub fn traced(p: &Plan, v: SkpVersion, trace: bool) -> Traced {
    let map = versions::map(v);
    let turned;
    let p = if versions::schema(map, "CCurve") == 3 {
        turned = head_to_tail(p);
        &turned
    } else {
        p
    };
    let mut a = Ar::new(map);
    if trace {
        a.trace = Some(Vec::new());
    }
    let mut head = Vec::new();
    super::vff::cstring(&mut head, "SketchUp Model");
    super::vff::cstring(&mut head, &format!("{{{}.0.1}}", v.major()));
    head.extend(p.guid);
    let w = W { p, code: versions::code(v), v };
    // Before the archive's objects: a string (empty) and the time of the save.
    a.str("");
    a.time(p.time);
    a.obj(("CVersionMap", 0), |a| {
        for (class, schema) in map {
            a.str(class);
            a.u32(*schema);
        }
        a.str("End-Of-Version-Map");
        a.u32(0);
    });
    w.model(&mut a);
    let classes: Vec<&'static str> = a.declared().into_iter().filter(|c| *c != "CVersionMap").collect();
    let base = head.len();
    head.extend(a.out);
    Traced {
        classes,
        bytes: head,
        trace: a.trace.unwrap_or_default().into_iter().map(|(o, s)| (o + base, s)).collect(),
        variable: a.variable.into_iter().map(|(o, n)| (o + base, n)).chain([(base - 16, 16)]).collect(),
    }
}

/// SketchUp 3 keeps a curve as its first and last edges and finds the others going on from
/// the first edge's end: a curve whose first edge points away from the second is lost (by
/// the SDK's writer too: measured with the SDK, a polyline whose first edge is turned back
/// reads back without its curve). The curve is then listed from its other end when its last
/// edge leads the way, else its first edge is turned (the faces' uses of it with it).
fn head_to_tail(p: &Plan) -> Plan {
    use std::collections::{HashMap, HashSet};
    fn fix(e: &mut Ents) {
        let ends: HashMap<Id, (Id, Id)> = e.edges.iter().map(|x| (x.el.id, (x.start, x.end))).collect();
        // Whether `a`, going on from its end, reaches `b`.
        let leads = |a: Id, b: Id| match (ends.get(&a), ends.get(&b)) {
            (Some(&(_, t)), Some(&(s2, t2))) => t == s2 || t == t2,
            _ => true,
        };
        let mut turn: HashSet<Id> = HashSet::new();
        let arcs = e.arcs.len();
        for (k, c) in e.arcs.iter_mut().chain(e.curves.iter_mut()).enumerate() {
            let n = c.edges.len();
            if n < 2 || leads(c.edges[0], c.edges[1]) {
                continue;
            }
            // An arc goes counter-clockwise from its start: its order stays.
            if k >= arcs && leads(c.edges[n - 1], c.edges[n - 2]) {
                c.edges.reverse();
            } else {
                turn.insert(c.edges[0]);
            }
        }
        if turn.is_empty() {
            return;
        }
        for x in e.edges.iter_mut().filter(|x| turn.contains(&x.el.id)) {
            std::mem::swap(&mut x.start, &mut x.end);
        }
        for f in &mut e.faces {
            for (_, uses) in &mut f.loops {
                for (id, rev) in uses.iter_mut() {
                    if turn.contains(id) {
                        *rev = !*rev;
                    }
                }
            }
        }
    }
    let mut p = p.clone();
    fix(&mut p.root);
    for d in &mut p.defs {
        fix(&mut d.ents);
    }
    p
}

struct W<'p> {
    p: &'p Plan,
    /// The version code SketchUp's writers test (3000 … 20000).
    code: u32,
    v: SkpVersion,
}

/// A key for an object of the plan.
fn k(class: &'static str, id: u64) -> super::mfc::Key {
    (class, id)
}

/// Keys of objects that have no persistent id: above every id of the plan.
const OWN: u64 = 1 << 40;

impl W<'_> {
    fn has(&self, a: &Ar, class: &str) -> bool {
        versions::has(a.map, class)
    }

    // --- Bases --------------------------------------------------------------------------

    /// `CEntity`: its dictionaries (an attribute container) and its persistent id.
    fn entity(&self, a: &mut Ar, id: u64, dicts: &[Dict], tex: Option<&[Tc; 2]>) {
        a.wrote("CEntity");
        let s = a.schema("CEntity");
        match s {
            0 | 2 => {}
            1 => a.u32(0),
            _ => {
                let has_container = !dicts.is_empty() || (tex.is_some() && self.has(a, "CFaceTextureCoords"));
                if has_container && self.has(a, "CAttributeContainer") {
                    a.obj(k("CAttributeContainer", OWN + a.out.len() as u64), |a| self.container(a, dicts, tex));
                } else {
                    a.null();
                }
                if s == 4 {
                    a.u64(id);
                } else if s >= 5 {
                    a.compact_id(id);
                }
            }
        }
    }

    fn container(&self, a: &mut Ar, dicts: &[Dict], tex: Option<&[Tc; 2]>) {
        self.entity(a, 0, &[], None);
        for d in dicts.iter().filter(|d| d.name != EMPTY_CONTAINER) {
            a.obj(k("CAttributeNamed", OWN + a.out.len() as u64), |a| self.dict(a, d));
        }
        if let Some(t) = tex.filter(|_| self.has(a, "CFaceTextureCoords")) {
            a.obj(k("CFaceTextureCoords", OWN + a.out.len() as u64), |a| self.tex_coords(a, t));
        }
        a.null();
    }

    fn dict(&self, a: &mut Ar, d: &Dict) {
        a.wrote("CAttribute");
        // Its own dictionaries: those nested in it.
        self.entity(a, 0, &d.children, None);
        a.u32(0);
        a.str(&d.name);
        let mut kv: Vec<&(String, Tv)> = d.values.iter().collect();
        kv.sort_by(|x, y| x.0.as_bytes().cmp(y.0.as_bytes()));
        for (key, val) in kv {
            if key.is_empty() {
                continue;
            }
            a.str(key);
            variant(a, val, 0);
        }
        a.str("");
        if a.schema("CAttributeNamed") > 0 {
            a.u32(0);
        }
    }

    fn tex_coords(&self, a: &mut Ar, t: &[Tc; 2]) {
        self.entity(a, 0, &[], None);
        a.u32(0);
        let s = a.schema("CFaceTextureCoords");
        a.f64s_derived(&t[0].matrix);
        if s != 0 {
            a.f64s(&t[0].projection);
        }
        if s > 1 {
            a.f64s_derived(&t[1].matrix);
            a.f64s(&t[1].projection);
        }
        if s > 2 {
            a.i32(0);
            a.i32(0);
        }
        if s >= 4 {
            a.u32(t[0].flags);
            a.u32(t[1].flags);
        }
    }

    /// `CDrawingElement`: entity, material, flags, tag.
    fn element(&self, a: &mut Ar, el: &El) {
        a.wrote("CDrawingElement");
        self.entity(a, el.id, &el.dicts, el.tex.as_deref());
        let s = a.schema("CDrawingElement");
        if s > 4 {
            match el.material {
                Some(m) => self.material_ref(a, m),
                None => a.null(),
            }
        } else if s != 2 {
            a.color([0, 0, 0, 0]);
        }
        let f = |b: u8| el.flags & b != 0;
        if s != 0 {
            a.bool(f(flags::HIDDEN));
        }
        if s >= 6 {
            a.bool(f(flags::CASTS));
            a.bool(f(flags::RECEIVES));
        }
        if s >= 7 {
            a.bool(f(flags::SOFT));
        }
        if s >= 8 {
            a.bool(f(flags::SMOOTH));
            if s > 8 {
                a.bool(f(flags::LOCKED));
            }
        }
        if s >= 4 {
            match el.layer {
                Some(l) => self.layer_ref(a, l),
                None => a.null(),
            }
        }
    }

    // --- Materials and tags -----------------------------------------------------------

    fn material_ref(&self, a: &mut Ar, id: u64) {
        let Some(m) = self.p.materials.iter().find(|m| m.id == id) else {
            a.null();
            return;
        };
        a.obj(k("CMaterial", id), |a| self.material(a, m));
    }

    fn material(&self, a: &mut Ar, m: &Mat) {
        let s = a.schema("CMaterial");
        self.entity(a, m.id, &m.dicts, None);
        a.str(&m.name);
        if s < 3 {
            a.bool(false);
        }
        a.bool(m.texture.is_some());
        if let Some(t) = &m.texture {
            if s < 4 {
                a.obj(k("CTexture", OWN + a.out.len() as u64), |a| self.texture(a, t, m.kind, m.color));
            } else {
                self.texture(a, t, m.kind, m.color);
            }
        }
        if (2..=4).contains(&s) {
            a.u32(0);
        }
        if s > 5 {
            a.bool(m.owned);
        }
        if s > 6 {
            a.color([m.color[0], m.color[1], m.color[2], 255]);
        }
        if s > 7 {
            a.str("");
        }
        if s >= 9 {
            a.i32(m.kind);
            if s >= 10 {
                a.i32(m.colorize_type);
                if s >= 11 {
                    a.f64(m.trans);
                    if s >= 12 {
                        a.bool(m.use_trans);
                    }
                }
            }
        }
    }

    fn texture(&self, a: &mut Ar, t: &Tex, kind: i32, color: [u8; 3]) {
        a.wrote("CTexture");
        let s = a.schema("CTexture");
        if s > 4 {
            self.entity(a, 0, &[], None);
        }
        if s < 6 {
            if s < 2 {
                a.obj(k("CDib", OWN + a.out.len() as u64), |a| self.dib(a, &t.image));
            } else {
                a.bool(true);
                self.dib(a, &t.image);
            }
        } else {
            a.obj(k("CDib", OWN + a.out.len() as u64), |a| self.dib(a, &t.image));
        }
        a.f64(t.size[0]);
        a.f64(t.size[1]);
        a.str(&t.file_name);
        if s != 0 {
            // A colorized texture keeps the colour it is tinted with.
            let c = if kind == 2 { color } else { t.avg };
            a.color([c[0], c[1], c[2], 255]);
        }
        if s == 3 {
            a.u64(0);
        }
    }

    /// `CDib`: an image, as the bytes of its file: JPEG (format 1, then its
    /// quality) or PNG (format 4; any other image is turned into a PNG, as SketchUp does for
    /// the oldest versions).
    fn dib(&self, a: &mut Ar, bytes: &[u8]) {
        a.wrote("CDib");
        let s = a.schema("CDib");
        if s == 0 {
            return;
        }
        let jpeg = bytes.starts_with(&[0xFF, 0xD8]);
        let png;
        let data: &[u8] = if jpeg || bytes.starts_with(b"\x89PNG") {
            bytes
        } else {
            png = to_png(bytes);
            &png
        };
        let format = if jpeg { 1 } else { 4 };
        if s != 1 {
            a.i32(format);
        }
        a.u32(data.len() as u32);
        a.out.extend(data);
        if s >= 3 && format == 1 {
            a.u32(90);
        }
    }

    fn layer_ref(&self, a: &mut Ar, id: u64) {
        let Some(l) = self.p.layers.iter().find(|l| l.id == id) else {
            a.null();
            return;
        };
        a.obj(k("CLayer", id), |a| self.layer(a, l));
    }

    fn layer(&self, a: &mut Ar, l: &Layer) {
        let s = a.schema("CLayer");
        self.entity(a, l.id, &l.dicts, None);
        a.str(&l.name);
        a.bool(l.hidden);
        let mut m = l.material.clone();
        if l.colorless {
            m.color = [64, 64, 64];
        }
        if s == 0 {
            a.color([m.color[0], m.color[1], m.color[2], 255]);
            a.null();
        } else {
            self.material(a, &m);
            if s >= 2 {
                a.u32(l.page_behavior);
                if s > 2 {
                    match l.line_style.filter(|_| self.has(a, "CCustomLineStyle")) {
                        Some(ls) => self.line_style_ref(a, ls),
                        None => a.null(),
                    }
                }
            }
        }
    }

    fn line_style_ref(&self, a: &mut Ar, id: u64) {
        let Some(st) = self.p.line_styles.iter().find(|s| s.id == id) else {
            a.null();
            return;
        };
        a.obj(k("CCustomLineStyle", id), |a| {
            let s = a.schema("CCustomLineStyle");
            self.entity(a, st.id, &[], None);
            if s < 3 {
                self.entity(a, st.id, &[], None);
            }
            a.str(st.name);
            if s == 1 {
                a.u16(0);
            } else {
                a.str(st.pattern);
            }
            a.f64(1.0);
            if s > 1 {
                a.f64(1.0);
                if s < 4 {
                    a.f64(1.0);
                } else {
                    a.color([0, 0, 0, 255]);
                    a.u8(0);
                }
            }
        });
    }

    fn material_manager(&self, a: &mut Ar, root: bool) {
        a.wrote("CMaterialManager");
        let s = a.schema("CMaterialManager");
        if s != 0 {
            self.entity(a, 0, &[], None);
        }
        if root {
            let list: Vec<&Mat> = self.p.materials.iter().collect();
            a.u32(list.len() as u32);
            for m in list {
                a.obj(k("CMaterial", m.id), |a| self.material(a, m));
            }
        } else {
            a.u32(0);
        }
        if s > 1 {
            a.null();
            if s == 3 {
                a.null();
            }
        }
    }

    /// A tag manager: the model's tags, or (`root` false) that of a definition, which SketchUp
    /// writes with a default tag of its own.
    fn layer_manager(&self, a: &mut Ar, root: bool) {
        a.wrote("CLayerManager");
        let s = a.schema("CLayerManager");
        if s != 0 {
            self.entity(a, 0, &[], None);
        }
        let own;
        let layers: Vec<&Layer> = if root {
            self.p.layers.iter().collect()
        } else {
            let mut l = self.p.layers[0].clone();
            l.id = 0;
            l.material.id = 0;
            l.dicts.clear();
            own = l;
            vec![&own]
        };
        let mut own_key = None;
        a.i32(layers.len() as i32);
        for l in &layers {
            // The definition's own default tag: a key of its own.
            let key = if root { k("CLayer", l.id) } else { k("CLayer", OWN + a.out.len() as u64) };
            own_key = Some(key);
            a.obj(key, |a| self.layer(a, l));
        }
        if s < 3 {
            let name = self.p.layers.iter().find(|l| l.id == self.p.active_layer).map_or("Layer0", |l| l.name.as_str());
            a.str(if root { name } else { "Layer0" });
        } else {
            if s == 3 || !layers.is_empty() {
                if root {
                    self.layer_ref(a, self.p.active_layer);
                } else if let Some(key) = own_key {
                    a.obj(key, |_| {});
                }
            }
            if s == 6 {
                a.u32(0);
            } else if s >= 7 {
                let empty = Folder::default();
                self.folder(a, if root { &self.p.folders } else { &empty }, 0);
            }
        }
    }

    fn folder(&self, a: &mut Ar, f: &Folder, depth: usize) {
        let s = a.schema("CLayerGroup");
        self.entity(a, f.id, &[], None);
        a.str(&f.name);
        let subs: &[Folder] = if depth < 64 { &f.folders } else { &[] };
        a.i32(subs.len() as i32);
        for sub in subs {
            a.obj(k("CLayerGroup", sub.id), |a| self.folder(a, sub, depth + 1));
        }
        a.i32(f.layers.len() as i32);
        for &l in &f.layers {
            self.layer_ref(a, l);
        }
        if s > 1 {
            a.bool(f.hidden);
        }
        if s >= 3 {
            a.bool(f.hidden_on_new_pages);
        }
    }

    // --- Geometry ------------------------------------------------------------------------

    fn vertex_ref(&self, a: &mut Ar, e: &Ents, id: u64) {
        let pos = e.verts.iter().find(|v| v.id == id).map_or([0.0; 3], |v| v.pos);
        a.obj(k("CVertex", id), |a| {
            self.entity(a, id, &[], None);
            a.f64s(&pos);
        });
    }

    fn edge_ref(&self, a: &mut Ar, e: &Ents, id: u64) {
        let Some(x) = e.edges.iter().find(|x| x.el.id == id) else {
            a.null();
            return;
        };
        a.obj(k("CEdge", id), |a| {
            self.element(a, &x.el);
            self.vertex_ref(a, e, x.start);
            self.vertex_ref(a, e, x.end);
            if matches!(a.schema("CEdge"), 1 | 2) {
                match x.curve {
                    Some(c) => self.curve_ref(a, e, c),
                    None => a.null(),
                }
            }
        });
    }

    fn curve_ref(&self, a: &mut Ar, e: &Ents, id: u64) {
        let Some(c) = e.curves.iter().chain(&e.arcs).find(|c| c.id == id) else {
            a.null();
            return;
        };
        let class = if c.arc.is_some() { "CArcCurve" } else { "CCurve" };
        a.obj(k(class, id), |a| {
            self.entity(a, id, &[], None);
            match a.schema("CCurve") {
                4 => {
                    a.bool(c.polygon);
                    a.u32(c.segments);
                }
                3 => {
                    // Its first and last edges.
                    match c.edges.first() {
                        Some(&x) => self.edge_ref(a, e, x),
                        None => a.null(),
                    }
                    match c.edges.last() {
                        Some(&x) => self.edge_ref(a, e, x),
                        None => a.null(),
                    }
                    a.bool(c.polygon);
                    a.u32(c.segments);
                }
                2 => a.bool(c.polygon),
                0 => {
                    a.null();
                    a.null();
                }
                _ => {}
            }
            if let Some(arc) = &c.arc {
                a.f64s(&arc.center);
                a.f64s(&arc.normal);
                a.f64s(&arc.xaxis);
                a.f64(arc.start);
                a.f64(arc.end);
                if a.schema("CArcCurve") != 0 {
                    a.f64s(&arc.yaxis);
                }
            }
        });
    }

    fn face(&self, a: &mut Ar, e: &Ents, f: &Face) {
        a.obj(k("CFace", f.el.id), |a| {
            self.element(a, &f.el);
            a.f64s(&f.plane);
            a.u32(f.loops.len() as u32);
            for (li, (lid, uses)) in f.loops.iter().enumerate() {
                let lkey = k("CLoop", *lid);
                a.obj(lkey, |a| {
                    // Loops have no persistent id.
                    self.entity(a, 0, &[], None);
                    a.bool(li == 0);
                    a.bool(true);
                    let s = a.schema("CLoop");
                    for (i, (edge, rev)) in uses.iter().enumerate() {
                        if i > 0 && s == 0 {
                            break;
                        }
                        a.obj(k("CEdgeUse", OWN + a.out.len() as u64), |a| {
                            self.entity(a, 0, &[], None);
                            self.edge_ref(a, e, *edge);
                            a.bool(*rev);
                            // The loop it belongs to.
                            a.obj(lkey, |_| {});
                            if a.schema("CEdgeUse") == 0 {
                                a.null();
                                a.null();
                            }
                        });
                    }
                    if s != 0 && !uses.is_empty() {
                        a.null();
                    }
                });
            }
            let s = a.schema("CFace");
            if s == 1 {
                match f.el.material {
                    Some(m) => {
                        a.u32(1);
                        self.material_ref(a, m);
                    }
                    None => a.u32(0),
                }
            } else if s > 2 {
                match f.back {
                    Some(m) => self.material_ref(a, m),
                    None => a.null(),
                }
            }
        });
    }

    // --- Components ------------------------------------------------------------------------

    /// `CComponent`: the entities of the model (`root`) or of a definition, with (for the
    /// model) its materials, tags and definitions.
    fn component(&self, a: &mut Ar, e: &Ents, root: bool) {
        a.wrote("CComponent");
        self.element(a, &e.el);
        let s = a.schema("CComponent");
        if s >= 9 {
            self.material_manager(a, root);
            self.layer_manager(a, root);
            self.definition_list(a, root);
            self.items(a, e);
            self.relationships(a, e);
            match e.active_section {
                Some(id) => self.section_ref(a, e, id),
                None => a.null(),
            }
            if s == 10 {
                self.font_manager(a);
            }
        }
    }

    /// `CRelationshipMap` in place: u32 n + n obj `CRelationship` (CEntity,
    /// what the component is glued to, the component; both written with the items before).
    fn relationships(&self, a: &mut Ar, e: &Ents) {
        let key = |id: Id| {
            if e.faces.iter().any(|f| f.el.id == id) {
                Some(k("CFace", id))
            } else if e.instances.iter().any(|i| i.el.id == id) {
                Some(k("CComponentInstance", id))
            } else if e.groups.iter().any(|g| g.el.id == id) {
                Some(k("CGroup", id))
            } else if e.images.iter().any(|i| i.inst.el.id == id) {
                Some(k("CImage", id))
            } else {
                None
            }
        };
        let list: Vec<_> = e.glues.iter().filter_map(|[to, what]| Some((key(*to)?, key(*what)?))).collect();
        a.u32(list.len() as u32);
        for (n, (to, what)) in list.into_iter().enumerate() {
            a.obj(k("CRelationship", OWN + (e.el.id << 20) + n as u64), |a| {
                self.entity(a, 0, &[], None);
                a.obj(to, |_| {});
                a.obj(what, |_| {});
            });
        }
    }

    fn definition_list(&self, a: &mut Ar, root: bool) {
        a.wrote("CDefinitionList");
        if !root {
            a.u32(0);
            return;
        }
        a.u32(self.p.defs.len() as u32);
        for d in &self.p.defs {
            self.definition_ref(a, d.id);
        }
    }

    fn definition_ref(&self, a: &mut Ar, id: u64) {
        let Some(d) = self.p.defs.iter().find(|d| d.id == id) else {
            a.null();
            return;
        };
        a.obj(k("CComponentDefinition", id), |a| {
            let s = a.schema("CComponentDefinition");
            if self.code < 18000 {
                let mut ents = d.ents.clone();
                strip_advanced(&mut ents.el.dicts, "SU_DefinitionSet");
                self.component(a, &ents, false);
            } else {
                self.component(a, &d.ents, false);
            }
            a.guid(&d.guid);
            if s > 2 {
                a.str(&d.name);
                a.str(&d.description);
            }
            // The file it was loaded from and when (`DefExt::path`, `loaded_at`).
            if s < 4 {
                if d.path.is_empty() {
                    a.u32(0);
                } else {
                    a.u32(1);
                    a.str(&d.path);
                }
            } else {
                a.str(&d.path);
            }
            a.time(d.time.unwrap_or(self.p.time));
            if s > 8 {
                a.bool(false);
            }
            if s > 4 {
                a.f64s(&[0.0; 3]);
            }
            if s < 6 {
                a.u32(0);
            }
            if s > 1 {
                self.behavior(a, &d.behavior);
            }
            if s > 7 {
                a.u32(d.kind);
            }
            if s > 9 {
                // Its thumbnail (`CThumbnail`): its camera (the SDK's when it has none) and its
                // image as read.
                a.obj(k("CThumbnail", OWN + a.out.len() as u64), |a| {
                    self.entity(a, 0, &[], None);
                    let (eye, target, up) = match d.thumb_camera {
                        Some([e, t, u]) => (e, t, u),
                        None => ([0.0, 0.0, 1.0], [0.0, 0.0, 2.0], [0.0, 1.0, 0.0]),
                    };
                    let cam =
                        Camera { eye, target, up, perspective: true, fov: 30.0, near: 0.0, far: 0.0, height: 1.0 };
                    if a.schema("CThumbnail") == 0 {
                        self.camera_old(a, &cam);
                    } else {
                        self.camera(a, &cam);
                    }
                    match &d.thumb {
                        Some(img) if self.has(a, "CDib") => {
                            a.obj(k("CDib", OWN + a.out.len() as u64), |a| self.dib(a, img))
                        }
                        _ => a.null(),
                    }
                });
            }
        });
    }

    fn behavior(&self, a: &mut Ar, b: &Behavior) {
        a.wrote("CComponentBehavior");
        self.entity(a, 0, &[], None);
        let s = a.schema("CComponentBehavior");
        if s < 3 {
            a.bool(b.cuts_opening);
            if s < 2 {
                return;
            }
        } else {
            a.bool(b.glues);
            a.bool(b.cuts_opening);
        }
        a.i32(b.snap_to);
        if s > 3 {
            a.u8(b.always_face_camera as u8 | (b.shadows_face_sun as u8) << 1);
            if s >= 5 {
                a.i32(b.no_scale_mask);
            }
        }
    }

    fn instance(&self, a: &mut Ar, class: &'static str, i: &Inst) {
        a.obj(k(class, i.el.id), |a| {
            if self.code < 18000 {
                let mut el = i.el.clone();
                strip_advanced(&mut el.dicts, "SU_InstanceSet");
                self.element(a, &el);
            } else {
                self.element(a, &i.el);
            }
            self.definition_ref(a, i.def);
            a.f64s(&i.xform);
            let s = a.schema("CComponentInstance");
            if s < 2 {
                a.u32(0);
            }
            if (1..=2).contains(&s) {
                a.u32(0);
            }
            if s > 3 {
                a.str(&i.name);
            }
            if s > 4 {
                a.guid(&i.guid);
            }
        });
    }

    /// The entities of a collection, in SketchUp's order.
    fn items(&self, a: &mut Ar, e: &Ents) {
        let n = e.edges.len()
            + e.faces.len()
            + e.instances.len()
            + e.groups.len()
            + e.images.len()
            + e.guide_lines.len()
            + e.guide_points.len()
            + e.sections.len()
            + e.texts.len()
            + e.dims.len()
            + if self.has(a, "CDimensionRadial") { e.radials.len() } else { 0 }
            + if self.has(a, "CPolyline3d") { e.polylines.len() } else { 0 };
        a.u32(n as u32);
        for x in &e.edges {
            self.edge_ref(a, e, x.el.id);
        }
        for f in &e.faces {
            self.face(a, e, f);
        }
        for i in &e.instances {
            self.instance(a, "CComponentInstance", i);
        }
        for g in &e.groups {
            self.instance(a, "CGroup", g);
        }
        for i in &e.images {
            self.instance(a, "CImage", &i.inst);
        }
        for g in &e.guide_lines {
            a.obj(k("CConstructionLine", g.el.id), |a| {
                a.wrote("CConstructionGeometry");
                self.element(a, &g.el);
                a.f64s(&g.point);
                a.f64s(&g.dir);
                if a.schema("CConstructionLine") >= 1 {
                    a.f64(g.t[0]);
                    a.f64(g.t[1]);
                    // Its dash pattern: none (`FFFF`) since the line styles of 2019, 0 before.
                    a.u32(if self.code >= 19000 { 0xffff } else { 0 });
                }
            });
        }
        for g in &e.guide_points {
            a.obj(k("CConstructionPoint", g.el.id), |a| {
                a.wrote("CConstructionGeometry");
                self.element(a, &g.el);
                a.f64s(&g.pos);
                a.f64s(&[0.0; 3]);
                a.bool(false);
            });
        }
        for s in &e.sections {
            self.section_ref(a, e, s.el.id);
        }
        for t in &e.texts {
            self.text(a, e, t);
        }
        for d in &e.dims {
            self.dim(a, e, d);
        }
        if self.has(a, "CDimensionRadial") {
            for d in &e.radials {
                self.radial(a, e, d);
            }
        }
        if self.has(a, "CPolyline3d") {
            for pl in &e.polylines {
                // `CPolyline3d`: the drawing element, u32 n, n points.
                a.obj(k("CPolyline3d", pl.el.id), |a| {
                    self.element(a, &pl.el);
                    a.u32(pl.points.len() as u32);
                    for p in &pl.points {
                        a.f64s(p);
                    }
                });
            }
        }
    }

    /// `CDimensionRadial`: the dimension, the edge of its arc (or none), the
    /// angle, the leader's offset, diameter, and without an edge the arc itself.
    fn radial(&self, a: &mut Ar, e: &Ents, d: &super::plan::Radial) {
        a.wrote("CDimension");
        a.obj(k("CDimensionRadial", d.el.id), |a| {
            self.element(a, &d.el);
            a.str(&d.text);
            if a.schema("CDimension") > 0 {
                if d.font != 0 {
                    self.font_ref(a, d.font);
                } else {
                    a.null();
                }
                a.bool(false);
                a.i32(d.arrow_type);
            }
            match d.edge {
                Some(x) => self.edge_ref(a, e, x),
                None => a.null(),
            }
            a.f64(d.angle);
            a.f64(d.leader);
            a.bool(d.diameter);
            if d.edge.is_none() {
                let arc = d.arc.clone().unwrap_or(super::plan::Arc3 {
                    center: [0.0; 3],
                    normal: [0.0, 0.0, 1.0],
                    xaxis: [1.0, 0.0, 0.0],
                    yaxis: [0.0, 1.0, 0.0],
                    radius: 1.0,
                    start: 0.0,
                    end: std::f64::consts::TAU,
                });
                a.f64s(&arc.center);
                a.f64s(&arc.normal);
                a.f64s(&arc.xaxis);
                a.f64(arc.start);
                a.f64(arc.end);
                if a.schema("CDimensionRadial") > 1 {
                    a.f64s(&arc.yaxis);
                }
            }
        });
    }

    fn section_ref(&self, a: &mut Ar, e: &Ents, id: u64) {
        let Some(s) = e.sections.iter().find(|s| s.el.id == id) else {
            a.null();
            return;
        };
        a.obj(k("CSectionPlane", id), |a| {
            self.element(a, &s.el);
            a.f64s(&s.plane);
            if a.schema("CSectionPlane") >= 3 {
                a.str(&s.name);
                a.str(&s.symbol);
            }
        });
    }

    /// A point an annotation is attached to: type, version 4, point, entity,
    /// second entity, the instances that lead to the entity, those of the second. Attached as
    /// `c` says when the entity and the instances are already in the archive (they are: the
    /// collection's geometry and instances, with the definitions they place, come before its
    /// texts and dimensions, `items`); otherwise a free point at `p`.
    fn connection(&self, a: &mut Ar, p: Option<[f64; 3]>, c: Option<&super::plan::Conn>) {
        let written = |x: &(u64, &'static str)| a.written(k(x.1, x.0));
        let c = c.filter(|c| c.leaf.iter().chain(&c.path).all(written));
        let Some(c) = c else {
            a.i32(if p.is_some() { 1 } else { 0 });
            a.i32(4);
            a.f64s(&p.unwrap_or_default());
            a.null();
            a.null();
            a.u32(0);
            a.u32(0);
            return;
        };
        a.i32(c.kind);
        a.i32(4);
        a.f64s(&c.point);
        match c.leaf {
            // Already written: its index.
            Some((id, class)) => a.obj(k(class, id), |_| {}),
            None => a.null(),
        }
        a.null();
        a.u32(c.path.len() as u32);
        for &(id, class) in &c.path {
            a.obj(k(class, id), |_| {});
        }
        a.u32(0);
    }

    fn text(&self, a: &mut Ar, _e: &Ents, t: &Text) {
        a.obj(k("CText", t.el.id), |a| {
            self.element(a, &t.el);
            let s = a.schema("CText");
            if s == 0 {
                return;
            }
            // Its font (`CSkFont`), twice in the oldest schemas.
            if s > 3 {
                self.font_ref(a, t.font);
            }
            if s < 5 {
                self.font_ref(a, t.font);
            }
            a.f64(t.screen[0]);
            a.f64(t.screen[1]);
            if s < 5 {
                a.f64s(&t.point.unwrap_or_default());
            } else {
                self.connection(a, t.point, t.conn.as_ref());
            }
            if s < 6 {
                a.f64s(&t.leader);
                a.f64s(&t.leader);
            } else {
                a.f64s(&t.leader);
            }
            a.f64s(&[0.0, 0.0, 1.0]);
            if (3..=7).contains(&s) {
                a.f64(0.0);
            }
            a.i32(t.leader_type);
            a.i32(1);
            a.bool(true);
            a.bool(false);
            a.i32(t.arrow_type);
            if s > 1 {
                a.bool(true);
            }
            a.str(&t.text);
            if s > 7 {
                a.bool(false);
            }
            if s > 8 {
                a.i32(0);
            }
        });
    }

    fn dim(&self, a: &mut Ar, _e: &Ents, d: &Dim) {
        a.wrote("CDimension");
        a.obj(k("CDimensionLinear", d.el.id), |a| {
            let s = a.schema("CDimensionLinear");
            if s == 0 {
                a.null();
                a.null();
            } else {
                if s >= 2 {
                    self.element(a, &d.el);
                    a.str(&d.text);
                    if a.schema("CDimension") > 0 {
                        // Its font (none of its own: the model's style's).
                        if d.font != 0 {
                            self.font_ref(a, d.font);
                        } else {
                            a.null();
                        }
                        a.bool(false);
                        a.i32(d.arrow_type);
                    }
                }
                self.connection(a, Some(d.start), d.conns[0].as_ref());
                self.connection(a, Some(d.end), d.conns[1].as_ref());
            }
            a.f64s(&d.normal);
            if s < 4 {
                a.f64(0.0);
            } else {
                a.f64s(&d.xaxis);
            }
            a.u32(0);
            a.f64(d.position[1]);
            if s >= 3 {
                a.f64(d.position[0]);
                if s > 5 {
                    a.u32(1);
                }
            }
        });
    }

    // --- Settings ----------------------------------------------------------------------------

    /// A camera in the format of the first versions: the eye, the direction of the target (a
    /// unit vector), up…
    fn camera_old(&self, a: &mut Ar, c: &Camera) {
        a.f64s(&c.eye);
        let d = [c.target[0] - c.eye[0], c.target[1] - c.eye[1], c.target[2] - c.eye[2]];
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        a.f64s_derived(&if len > 0.0 { d.map(|x| x / len) } else { [0.0, 0.0, -1.0] });
        a.f64s(&c.up);
        a.f64(c.near);
        a.f64(c.far);
        a.bool(c.perspective);
        a.f64(c.fov);
        a.f64_derived(c.height);
        a.f64s(&[0.0; 3]);
    }

    fn camera(&self, a: &mut Ar, c: &Camera) {
        a.obj(k("CCamera", OWN + a.out.len() as u64), |a| {
            let s = a.schema("CCamera");
            a.f64s(&c.eye);
            a.f64s(&c.target);
            a.f64s(&c.up);
            a.f64(c.near);
            a.f64(c.far);
            a.bool(c.perspective);
            a.f64(c.fov);
            a.f64_derived(c.height);
            a.f64s(&[0.0; 3]);
            a.f64(0.0);
            a.bool(true);
            if s >= 2 {
                a.bool(false);
                if s > 2 {
                    a.str("");
                }
                if s > 3 {
                    a.f64(0.0);
                }
                if s > 4 {
                    a.bool(false);
                    a.f64s(&[1.0, 0.0, 0.0]);
                }
            }
        });
    }

    /// `CRenderingOptions` in place (storing branch).
    fn rendering(&self, a: &mut Ar, ro: &[(u16, Ro)], shadows: bool) {
        a.wrote("CRenderingOptions");
        self.entity(a, 0, &[], None);
        let s = a.schema("CRenderingOptions");
        let get = |tag: u16| ro.iter().find(|(t, _)| *t == tag).map(|(_, v)| *v);
        let int = |tag: u16| match get(tag) {
            Some(Ro::Int(x)) => x,
            Some(Ro::Bool(b)) => b as i32,
            _ => 0,
        };
        let boolean = |tag: u16| match get(tag) {
            Some(Ro::Bool(b)) => b,
            Some(Ro::Int(x)) => x != 0,
            _ => false,
        };
        let real = |tag: u16| match get(tag) {
            Some(Ro::Double(x)) => x,
            Some(Ro::Float(x)) => x as f64,
            _ => 0.0,
        };
        let color = |tag: u16| match get(tag) {
            Some(Ro::Color(c)) => c,
            _ => [0, 0, 0, 0],
        };
        // The storing branch writes the newest render mode (6, monochrome in 2021+) as 2.
        let mode = int(0x733d);
        a.i32(if mode == 6 { 2 } else { mode });
        if (0x11..=0x15).contains(&s) {
            a.i32(if boolean(0x733f) {
                2
            } else if boolean(0x733e) {
                1
            } else {
                0
            });
        } else {
            a.bool(boolean(0x733e));
        }
        if s > 0x15 {
            a.bool(boolean(0x733f));
        }
        a.bool(boolean(0x734b));
        if s < 0x26 {
            if s <= 8 {
                a.bool(int(0x7341) != 0);
            } else {
                a.i32(int(0x7341));
            }
        } else {
            a.bool(boolean(0x734c));
            a.i32(int(0x7341));
        }
        a.color(color(0x7357));
        a.color(color(0x7358));
        a.color(color(0x7359));
        if s >= 0xb {
            a.color(color(0x735b));
            a.bool(shadows);
        } else if s != 0 {
            a.bool(shadows);
        }
        if s == 2 {
            a.bool(boolean(0x7340));
        }
        if s > 3 {
            a.bool(boolean(0x7349));
        }
        if s > 4 {
            a.bool(boolean(0x7347));
        }
        if s >= 6 {
            a.bool(boolean(0x7340));
            a.i32(int(0x7348));
            if s >= 7 {
                a.bool(boolean(0x734d));
                a.i32(int(0x734e));
                a.bool(boolean(0x734f));
                a.i32(int(0x7350));
                if s > 0x19 {
                    a.bool(boolean(0x7351));
                    a.i32(int(0x7352));
                    a.bool(boolean(0x7353));
                    a.i32(int(0x7354));
                    if s > 0x1b {
                        a.bool(boolean(0x7355));
                    }
                }
                if s < 0x27 {
                    a.bool(boolean(0x7380) || boolean(0x7381));
                } else {
                    a.bool(boolean(0x7380));
                    a.bool(boolean(0x7381));
                }
            }
            if s >= 8 {
                a.u32(0);
            }
        }
        if s > 9 {
            a.color(color(0x735c));
            a.color(color(0x735d));
        }
        if s > 0xb {
            a.f64(real(0x736c));
            a.f64(real(0x736d));
            a.bool(boolean(0x736e));
            a.bool(boolean(0x736f));
        }
        if s > 0x1c {
            a.bool(boolean(0x735f));
            a.color(color(0x7360));
            a.bool(boolean(0x7361));
            a.f64(real(0x7362));
            a.f64(real(0x7363));
        }
        if s > 0x1d {
            a.i32(int(0x7364));
        }
        if s > 0x1e {
            a.i32(int(0x7342));
            a.bool(boolean(0x7343));
            a.bool(boolean(0x7344));
            a.bool(boolean(0x7345));
            a.bool(boolean(0x7346));
        }
        if s >= 0x28 || s > 0xc {
            if s >= 0x28 {
                a.bool(boolean(0x7382));
                a.color(color(0x7365));
                a.color(color(0x7366));
            } else if s != 0xd {
                a.color(color(0x7365));
                if s >= 0x18 {
                    a.color(color(0x7366));
                }
            } else {
                a.color(color(0x7365));
                a.color(color(0x7367));
                for t in [0x7368, 0x7369, 0x736a] {
                    a.i32(boolean(t) as i32);
                }
            }
            if s != 0xd {
                a.color(color(0x7367));
                a.bool(boolean(0x7368));
                a.bool(boolean(0x7369));
                a.bool(boolean(0x736a));
            }
            a.i32(int(0x736b));
            if s >= 0xf {
                a.color(color(0x7370));
                a.color(color(0x7371));
                a.color(color(0x7372));
                a.i32(int(0x7374));
                a.u32(int(0x7375) as u32);
                if s >= 0x25 {
                    a.color(color(0x7373));
                    a.bool(boolean(0x7376));
                    a.i32(int(0x7377));
                } else if s == 0x10 {
                    a.bool(boolean(0x733f));
                } else if s == 0x12 {
                    a.bool(int(0x7377) != 0);
                } else if s > 0x12 {
                    a.i32(int(0x7377));
                }
                if s > 0x13 {
                    a.bool(boolean(0x7379));
                    a.f64(real(0x737a));
                }
                if s > 0x14 {
                    a.bool(boolean(0x737b));
                }
            }
        }
        if (0x17..=0x18).contains(&s) {
            a.bool(false);
        }
        if s > 0x1a {
            a.color(color(0x735a));
        }
        if s > 0x1f {
            a.bool(boolean(0x735e));
        }
        if s == 0x21 {
            a.bool(false);
        } else {
            if s > 0x22 {
                // Before 2017, a constant in place of the option.
                a.f64(if self.code < 17000 { 0.5 } else { real(0x7378) });
            }
            if s > 0x23 {
                a.bool(boolean(0x7356));
                a.bool(boolean(0x737c));
                a.f64(real(0x737d));
                a.bool(boolean(0x737e));
                a.f64(real(0x737f));
            }
        }
    }

    fn shadow(&self, a: &mut Ar, sh: &Shadow) {
        a.wrote("CShadowInfo");
        self.entity(a, 0, &[], None);
        let s = a.schema("CShadowInfo");
        a.i32(sh.time as i32);
        a.bool(sh.daylight);
        a.str(&sh.city);
        a.str(&sh.country);
        if s != 0 {
            a.f64(sh.longitude);
            a.f64(sh.latitude);
            a.f64(sh.tz);
        }
        if s > 1 {
            a.f64s_derived(&sh.north);
            a.bool(sh.display_north);
        }
        if s == 3 {
            a.bool(false);
        } else {
            if s > 4 {
                a.bool(sh.on_faces);
                a.bool(sh.on_ground);
                a.bool(sh.from_edges);
                a.i32(sh.light);
                a.i32(sh.dark);
            }
            if s > 5 {
                a.bool(sh.sun_shading);
            }
        }
    }

    fn sketch_cs(&self, a: &mut Ar, axes: &[f64; 12]) {
        a.wrote("CSketchCS");
        let el = El { flags: flags::CASTS | flags::RECEIVES, ..Default::default() };
        self.element(a, &el);
        a.f64s(axes);
    }

    fn options(&self, a: &mut Ar) {
        a.u32(3);
        a.u32(self.p.options.len() as u32);
        for (name, values) in &self.p.options {
            a.str(name);
            for (k, v) in values {
                if k.is_empty() {
                    continue;
                }
                a.str(k);
                variant(a, v, 0);
            }
            a.str("");
        }
    }

    fn pages(&self, a: &mut Ar) {
        a.wrote("CPageList");
        let s = a.schema("CViewPage");
        for sc in &self.p.scenes {
            let _ = s;
            a.obj(k("CViewPage", sc.id), |a| self.page(a, sc));
        }
    }

    fn page(&self, a: &mut Ar, sc: &Page) {
        a.wrote("CSketchUpPage");
        let v = a.schema("CViewPage");
        if a.schema("CEntity") != 0 {
            self.entity(a, sc.id, &sc.dicts, None);
        }
        a.str(&sc.name);
        a.str(&sc.description);
        let f = sc.flags;
        a.u32(f);
        if f & 1 != 0 {
            if v < 7 {
                self.camera_old(a, &sc.camera);
            } else {
                self.camera(a, &sc.camera);
            }
        }
        if f & 2 != 0 {
            let ro = sc.rendering.clone().unwrap_or_else(default_rendering);
            self.rendering(a, &ro, sc.shadow.as_ref().is_some_and(|s| s.display));
            // The scene's style: its own, else the model's.
            if v > 9 {
                self.scene_style(a, sc.style);
            }
        }
        if f & 4 != 0 {
            let sh = sc.shadow.clone().unwrap_or_else(|| self.p.shadow.clone());
            self.shadow(a, &sh);
            if v > 1 {
                a.bool(sh.display);
            }
        }
        if f & 8 != 0 {
            self.sketch_cs(a, &sc.axes.unwrap_or(self.p.axes));
            a.bool(true);
        }
        if f & 0x10 != 0 {
            a.u32(0);
        }
        if v != 0 && f & 0x20 != 0 {
            // SketchUp writes no hidden tag in the scenes of SketchUp 3.
            let hidden: &[Id] = if self.code == 3000 { &[] } else { &sc.hidden_layers };
            a.u32(hidden.len() as u32);
            for &l in hidden {
                self.layer_ref(a, l);
            }
        }
        if v > 0xe && f & 0x20 != 0 {
            a.u32(0);
        }
        if v == 4 {
            if f & 0x40 != 0 {
                a.null();
            }
            a.bool(sc.animate);
        } else if v > 4 {
            if f & 0x40 != 0 {
                a.u32(0);
            }
            a.bool(sc.animate);
        } else if v > 2 {
            a.bool(sc.animate);
        }
        if (6..=10).contains(&v) {
            a.bool(true);
        }
        if v >= 8 {
            // Its transition and delay times, its matched photo, 0x7157, its thumbnail
            // (present, then i32 n and the bytes).
            a.f64(sc.transition);
            if v > 8 {
                a.f64(sc.delay);
                if v > 9 {
                    self.photo_ref(a, sc.photo);
                }
                if v > 0xb {
                    a.bool(sc.v7157);
                    match &sc.thumbnail {
                        Some(t) => {
                            a.bool(true);
                            a.bool(true);
                            a.i32(t.len().min(i32::MAX as usize) as i32);
                            a.out.extend_from_slice(t);
                        }
                        None => a.bool(false),
                    }
                }
            }
        }
    }

    /// A matched photo (`CBackgroundImage`), written where it is first named.
    fn photo_ref(&self, a: &mut Ar, id: Option<Id>) {
        let Some(m) = id.and_then(|i| self.p.photos.iter().find(|m| m.id == i)) else {
            a.null();
            return;
        };
        if !self.has(a, "CBackgroundImage") {
            a.null();
            return;
        }
        a.obj(k("CBackgroundImage", m.id), |a| {
            self.entity(a, 0, &[], None);
            let s = a.schema("CBackgroundImage");
            let image = |a: &mut Ar| match &m.image {
                Some(img) if self.has(a, "CDib") => a.obj(k("CDib", OWN + a.out.len() as u64), |a| self.dib(a, img)),
                _ => a.null(),
            };
            a.str(&m.path);
            if s < 2 {
                // SketchUp 6: the image last.
                a.bool(m.flag);
                a.f64(m.value);
                a.u32(m.points.len() as u32);
                for q in &m.points {
                    a.f64s(q);
                }
                image(a);
                return;
            }
            let p = match s {
                3 => 1,
                4 => 2,
                x if x > 4 => 3,
                _ => 0,
            };
            if p < 3 {
                a.bool(m.kind != 2);
            } else {
                a.u32(m.kind);
            }
            image(a);
            if p > 0 {
                a.u32(m.size[0]);
                a.u32(m.size[1]);
                a.u32(m.v2afe);
            }
            if p > 1 {
                a.u32(m.time);
            }
            a.bool(m.flag);
            a.f64(m.value);
            a.u32(m.points.len() as u32);
            for q in &m.points {
                a.f64s(q);
            }
            if s > 5 {
                a.f64s(&m.offset);
            }
            if s > 6 {
                a.f64(m.scale);
            }
            if s >= 10 {
                a.u32(m.v290b);
            }
        });
    }

    /// A font of the model (`CSkFont`): CEntity, face, italic, bold, points, (schema ≠ 0)
    /// world size and height.
    fn font_ref(&self, a: &mut Ar, id: u64) {
        let Some(f) = self.p.font_list.iter().find(|f| f.id == id) else {
            a.null();
            return;
        };
        a.obj(k("CSkFont", id), |a| {
            self.entity(a, id, &[], None);
            a.str(&f.face);
            a.bool(f.italic);
            a.bool(f.bold);
            a.u32(f.size);
            if a.schema("CSkFont") != 0 {
                a.bool(f.world);
                a.f64(f.height);
            }
        });
    }

    fn font_manager(&self, a: &mut Ar) {
        a.wrote("CFontManager");
        self.entity(a, 0, &[], None);
        a.u32(self.p.font_list.len() as u32);
        // SketchUp lists them in no fixed order (two writings of one model differ).
        let at = a.out.len();
        for f in &self.p.font_list {
            self.font_ref(a, f.id);
        }
        a.variable.push((at, a.out.len() - at));
    }

    /// The model's text style (`CTextStyle`, in place): its font, settings, second font.
    fn text_style(&self, a: &mut Ar) {
        a.wrote("CTextStyle");
        self.entity(a, 0, &[], None);
        let s = a.schema("CTextStyle");
        let t = &self.p.text_style;
        let font = t.font.unwrap_or(self.p.fonts[1]);
        if s == 3 {
            let f = self.p.font_list.iter().find(|f| f.id == font);
            a.str(f.map_or("Tahoma", |f| f.face.as_str()));
            a.u32(f.map_or(12, |f| f.size));
            a.bool(f.is_some_and(|f| f.bold));
            a.bool(f.is_some_and(|f| f.italic));
            a.str("");
        } else {
            self.font_ref(a, font);
        }
        a.u32(t.v1);
        a.u32(t.v2);
        a.bool(t.b1);
        a.u32(t.v3);
        a.bool(t.b2);
        if s >= 2 {
            a.color(t.color);
            if s >= 5 {
                a.color(t.color2);
                self.font_ref(a, t.font2.unwrap_or(self.p.fonts[1]));
            }
        }
    }

    /// The model's dimension style (`CDimensionStyle`, in place).
    fn dim_style(&self, a: &mut Ar) {
        a.wrote("CDimensionStyle");
        self.entity(a, 0, &[], None);
        let s = a.schema("CDimensionStyle");
        let d = &self.p.dim_style;
        a.u32(d.lead);
        if s > 0 {
            self.font_ref(a, d.font.unwrap_or(self.p.fonts[0]));
        }
        a.bool(d.text3d);
        a.bool(d.radial_prefix);
        a.u32(d.ext_offset);
        if s > 2 {
            a.u32(d.ext_overshoot);
        }
        a.u32(d.line_weight);
        a.i32(d.arrow);
        if s > 2 {
            a.u32(d.arrow_size);
        }
        a.bool(d.highlight);
        a.color(d.highlight_color);
        a.bool(d.b5fbf);
        a.bool(d.hide_out_of_plane);
        a.f64(d.hide_out_of_plane_value);
        a.bool(d.hide_small);
        a.f64(d.hide_small_value);
        if s > 1 {
            a.color(d.color);
            a.color(d.text_color);
        }
        if s >= 4 {
            a.u32(d.v5fc6);
        }
    }
    fn line_style_manager(&self, a: &mut Ar) {
        a.wrote("CLineStyleManager");
        self.entity(a, 0, &[], None);
        a.u32(self.p.line_styles.len() as u32);
        for s in &self.p.line_styles {
            self.line_style_ref(a, s.id);
        }
    }

    fn style(&self, a: &mut Ar, id: u64, name: &str) {
        let key = k("CSkpStyle", if id == 0 { OWN - 2 } else { id });
        a.obj(key, |a| {
            self.entity(a, id, &[], None);
            a.guid(&new_guid());
            a.str("");
            // Its properties: those of a new style.
            a.u32(3);
            a.str(name);
            a.str("");
            // The model's style keeps the settings of a new one; the copy being edited (the
            // style without an id) has the model's rendering options.
            let items = self.style_props(id == 0);
            a.i32(items.len() as i32);
            for (key, ty, v) in items {
                a.u32(key);
                if key == 0x1389 {
                    a.u32(0);
                    continue;
                }
                a.u32(1);
                a.u32(ty as u32);
                match v {
                    StyleValue::U8(x) => a.u8(x),
                    StyleValue::I32(x) => a.i32(x),
                    StyleValue::F64(x) => a.f64(x),
                    StyleValue::F32(x) => a.f32(x),
                    StyleValue::None => {}
                }
            }
        });
    }

    /// The properties of a style: those of a new one, or (`from_rendering`) the model's
    /// rendering options as SketchUp fills a style from them.
    fn style_props(&self, from_rendering: bool) -> Vec<(u32, u8, StyleValue)> {
        let ro = &self.p.rendering;
        let get = |tag: u16| if from_rendering { ro.iter().find(|(t, _)| *t == tag).map(|(_, v)| *v) } else { None };
        super::xml::style_items_legacy()
            .map(|(key, ty, v)| {
                let from = super::xml::STYLE_FROM_RENDERING.iter().find(|(k, _)| *k == key).and_then(|(_, t)| get(*t));
                let hidden = key == 7010 && from_rendering;
                let hidden_value =
                    matches!(get(0x7380), Some(Ro::Bool(true))) || matches!(get(0x7381), Some(Ro::Bool(true)));
                let val = match ty {
                    1 => StyleValue::U8(match from {
                        _ if hidden => hidden_value as u8,
                        Some(Ro::Bool(b)) => b as u8,
                        Some(Ro::Int(i)) => (i != 0) as u8,
                        _ => v.parse::<u8>().unwrap_or(0),
                    }),
                    4 => StyleValue::I32(match from {
                        Some(Ro::Int(i)) => i,
                        Some(Ro::Color(c)) => {
                            // The system colours resolved: black ink, white paper.
                            let c = match (key, c) {
                                (1014, [0, 0, 0, 0]) => [0, 0, 0, 255],
                                (4000, [0, 0, 0, 0]) => [255, 255, 255, 255],
                                _ => c,
                            };
                            i32::from_le_bytes(c)
                        }
                        Some(Ro::Bool(b)) => b as i32,
                        _ => v.parse::<i32>().unwrap_or(0),
                    }),
                    7 => StyleValue::F64(match from {
                        Some(Ro::Double(x)) => x,
                        Some(Ro::Float(x)) => x as f64,
                        _ => v.parse::<f64>().unwrap_or(0.0),
                    }),
                    6 => StyleValue::F32(v.parse::<f32>().unwrap_or(0.0)),
                    _ => StyleValue::None,
                };
                (key, ty, val)
            })
            .collect()
    }

    fn style_manager(&self, a: &mut Ar) {
        a.wrote("CSkpStyleManager");
        self.entity(a, 0, &[], None);
        let st = &self.p.styles;
        if !st.list.is_empty() {
            a.i32(st.list.len() as i32);
            for s in &st.list {
                self.style_rec(a, s.id, s);
            }
            self.scene_style(a, None);
            match &st.active {
                Some(act) => self.style_rec(a, OWN - 2, act),
                None => a.null(),
            }
            if a.schema("CSkpStyleManager") > 1 {
                a.bool(st.changed);
            }
            return;
        }
        a.i32(1);
        self.style(a, STYLE_ID, "Style");
        self.style(a, STYLE_ID, "Style");
        self.style(a, 0, "Style");
        // Whether the style being edited differs from the model's.
        if a.schema("CSkpStyleManager") > 1 {
            a.bool(self.style_props(true) != self.style_props(false));
        }
    }

    /// A reference to a style of the model (`id`: one of the list, else the selected one).
    fn scene_style(&self, a: &mut Ar, id: Option<Id>) {
        let st = &self.p.styles;
        if st.list.is_empty() {
            self.style(a, STYLE_ID, "Style");
            return;
        }
        let s = id
            .and_then(|i| st.list.iter().find(|s| s.id == i))
            .or_else(|| st.selected.and_then(|i| st.list.get(i)))
            .or(st.list.first());
        match s {
            Some(s) => self.style_rec(a, s.id, s),
            None => a.null(),
        }
    }

    /// `CSkpStyle` (storing branch): CEntity, GUID, the `.style` it came
    /// from, its properties (version 3, name, description, the items but
    /// those the archive does not have: 7019, 7020 and the ambient occlusion).
    fn style_rec(&self, a: &mut Ar, key: u64, s: &crate::native::style::StyleRec) {
        use crate::native::style::{DRAW_HIDDEN, Item, ItemV};
        a.obj(k("CSkpStyle", key), |a| {
            self.entity(a, if key >= OWN - 2 { 0 } else { s.id }, &[], None);
            a.guid(&crate::native::style::guid_bytes(&s.guid).unwrap_or_else(new_guid));
            a.str(&s.path);
            a.u32(3);
            a.str(&s.name);
            a.str(&s.description);
            const SKIPPED: [u32; 8] = [7019, 7020, 8100, 8102, 8103, 8105, 8106, 8107];
            let mut items: Vec<Item> =
                s.items.iter().filter(|i| !SKIPPED.contains(&i.id) && !matches!(i.v, ItemV::Xml(_))).cloned().collect();
            // Before 2020 one item says « hidden geometry or hidden objects » (7010).
            if a.schema("CSkpStyle") < 2 {
                let on = |id: u32| items.iter().any(|i| i.id == id && i.v.f64().is_some_and(|x| x != 0.0));
                let h = on(7017) || on(7018);
                items.retain(|i| i.id != DRAW_HIDDEN);
                items.push(Item { id: DRAW_HIDDEN, v: ItemV::Num(1, h as u64) });
                items.sort_by_key(|i| i.id);
            }
            a.i32(items.len() as i32);
            for it in &items {
                a.u32(it.id);
                match &it.v {
                    ItemV::Npr(n) => {
                        a.u32(2);
                        a.f64(n.dropout);
                        a.f64(n.fade);
                        a.u32(n.width);
                        a.bool(n.halo);
                        a.u32(n.halo_length);
                        a.u32(n.protos);
                        a.i32(n.lengths.len() as i32);
                        for l in &n.lengths {
                            a.u32(*l);
                        }
                        a.i32(n.images.len() as i32);
                        for (i, img) in n.images.iter().enumerate() {
                            a.obj(k("CDib", (1 << 50) + (s.id << 8) + key % 2 * (1 << 49) + i as u64), |a| {
                                self.dib(a, img)
                            });
                        }
                    }
                    ItemV::Watermarks(list) => {
                        a.u32(list.len() as u32);
                        for w in list {
                            match w {
                                Some(w) => self.watermark(a, w),
                                None => a.null(),
                            }
                        }
                    }
                    ItemV::Num(t, b) => {
                        a.u32(1);
                        a.u32(*t);
                        match t {
                            1 | 2 => a.u8(*b as u8),
                            3 => a.u16(*b as u16),
                            7 | 15 => a.u64(*b),
                            _ => a.u32(*b as u32),
                        }
                    }
                    ItemV::Vec3(t, v) => {
                        a.u32(1);
                        a.u32(*t);
                        a.f64s(v);
                    }
                    ItemV::Xform(x) => {
                        a.u32(1);
                        a.u32(12);
                        a.f64s(&x[..]);
                    }
                    ItemV::Xml(_) => {}
                }
            }
        });
    }

    /// `CWatermark` (storing branch).
    fn watermark(&self, a: &mut Ar, w: &crate::native::style::Wm) {
        a.obj(k("CWatermark", w.id), |a| {
            self.entity(a, w.id, &[], None);
            a.bool(w.found);
            a.str(&w.file);
            a.u32(w.time.clamp(0, u32::MAX as i64) as u32);
            a.u32(w.position);
            a.bool(w.fitting == 0);
            a.bool(w.fitting == 1);
            a.bool(w.fitting == 1 && w.keep_aspect);
            a.bool(w.background);
            a.bool(w.mask);
            a.f64(w.alpha);
            a.f64(w.scale);
            a.str(&w.name);
            match &w.image {
                Some(img) if !img.is_empty() => a.obj(k("CDib", (1 << 51) + w.id), |a| self.dib(a, img)),
                _ => a.null(),
            }
        });
    }

    fn watermark_manager(&self, a: &mut Ar) {
        a.wrote("CWatermarkManager");
        self.entity(a, 0, &[], None);
        if a.schema("CWatermarkManager") < 2 {
            a.u32(0);
        }
        let list = &self.p.styles.watermarks;
        if list.iter().all(Option::is_none) {
            a.i32(0);
            return;
        }
        a.i32(list.len() as i32);
        for w in list {
            match w {
                Some(w) => self.watermark(a, w),
                None => a.null(),
            }
        }
    }

    // --- The model -------------------------------------------------------------------------

    /// `CSketchUpModel::Serialize`, storing branch.
    fn model(&self, a: &mut Ar) {
        a.wrote("CSketchUpModel");
        let p = self.p;
        let c = self.code;
        let m = a.schema("CSketchUpModel");
        a.mark(|| "modèle".into());
        a.u32(1);
        a.u32(0x4b0);
        if c >= 6000 {
            a.u32(0);
        }
        if c >= 17000 {
            a.variable.push((a.out.len(), 8));
            a.u64(p.next_id);
        }
        // The preview, and whether it is up to date.
        match &p.thumbnail {
            Some(png) if self.has(a, "CDib") => a.obj(k("CDib", OWN - 3), |a| self.dib(a, png)),
            _ => a.null(),
        }
        a.bool(true);
        a.mark(|| "comportement".into());
        self.behavior(a, &Behavior::default());
        a.str(&p.description);
        if c >= 7000 {
            a.mark(|| "options".into());
            self.options(a);
            // The model's name and description (its dictionaries go with the root component).
            let mut dicts = Vec::new();
            let mut props = Dict { name: "ModelProperties".into(), ..Default::default() };
            if !p.description.is_empty() {
                props.values.push(("Description".into(), Tv::Str(p.description.clone())));
            }
            for key in ["IsClassified", "IsDynamic", "IsLive"] {
                props.values.push((key.into(), Tv::Bool(false)));
            }
            if !p.name.is_empty() {
                props.values.push(("Name".into(), Tv::Str(p.name.clone())));
            }
            dicts.insert(0, props);
            a.obj(k("CAttributeContainer", OWN - 4), |a| self.container(a, &dicts, None));
            a.null();
        }
        a.mark(|| "caméra".into());
        if c == 3000 {
            self.camera_old(a, &p.camera);
        } else {
            self.camera(a, &p.camera);
        }
        a.mark(|| "rendu".into());
        self.rendering(a, &p.rendering, p.shadow.display);
        a.u32(0);
        if c >= 6000 {
            a.u32(1);
        }
        a.mark(|| "racine".into());
        self.component(a, &p.root, true);
        a.mark(|| "ombres".into());
        self.shadow(a, &p.shadow);
        a.mark(|| "scènes".into());
        if c == 3000 {
            a.u32(p.scenes.len() as u32);
            self.pages(a);
        } else {
            self.entity(a, 0, &[], None);
            a.u32(p.scenes.len() as u32);
            self.pages(a);
            match p.selected_scene {
                Some(id) => a.obj(k("CViewPage", id), |_| {}),
                None => a.null(),
            }
        }
        a.mark(|| "axes".into());
        self.sketch_cs(a, &p.axes);
        for _ in 0..16 {
            a.u32(0);
        }
        if c < 7000 {
            self.options(a);
        }
        a.mark(|| "styles de cote et de texte".into());
        self.dim_style(a);
        self.text_style(a);
        self.font_manager(a);
        if c >= 19000 {
            self.line_style_manager(a);
        }
        if c >= 6000 {
            // The active matched photo.
            self.photo_ref(a, p.active_photo);
            a.mark(|| "styles".into());
            self.style_manager(a);
            self.watermark_manager(a);
        }
        // The model's values whose use is not known (`doc::Doc::model_flags`).
        let flags = p.model_flags;
        if c >= 7000 {
            a.u32(flags.v20f);
            a.bool(flags.v20c);
        }
        if c >= 14000 {
            // The classification schemas (`CSchemaZipFile`): the file each came
            // from, its name, its `.skc`.
            let schemas: &[crate::native::doc::SchemaFile] =
                if self.has(a, "CSchemaZipFile") { &p.schemas } else { &[] };
            a.u32(schemas.len() as u32);
            for (i, s) in schemas.iter().enumerate() {
                a.obj(k("CSchemaZipFile", i as u64), |a| {
                    a.str(&s.path);
                    a.str(&s.name);
                    a.i32(s.data.len().min(i32::MAX as usize) as i32);
                    a.out.extend_from_slice(&s.data);
                });
            }
        }
        if c >= 18000 {
            a.u32(flags.v20d);
        }
        let _ = (m, self.v);
    }
}

/// A value of a style's properties.
#[derive(Clone, Copy, PartialEq)]
enum StyleValue {
    U8(u8),
    I32(i32),
    F64(f64),
    F32(f32),
    None,
}

/// The advanced attributes of SketchUp 2018 (`SU_DefinitionSet` of a component, `SU_InstanceSet`
/// of its instances) are not written in older versions, which do not have them (SketchUp gives
/// them back, empty, when it opens such a file).
fn strip_advanced(dicts: &mut Vec<Dict>, name: &str) {
    let before = dicts.len();
    dicts.retain(|d| d.name != name);
    // The entity keeps its (empty) container, as SketchUp writes it.
    if dicts.is_empty() && before > 0 {
        dicts.push(Dict { name: EMPTY_CONTAINER.into(), ..Default::default() });
    }
}

/// A dictionary that is not written: it only keeps its entity's container.
const EMPTY_CONTAINER: &str = "\u{1}";

/// An image file other than PNG or JPEG (BMP…) as a PNG; a 1×1 grey PNG when it cannot be
/// decoded.
fn to_png(bytes: &[u8]) -> Vec<u8> {
    let img = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.decode().ok())
        .unwrap_or_else(|| {
            image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(1, 1, image::Rgb([128, 128, 128])))
        });
    let mut out = std::io::Cursor::new(Vec::new());
    let _ = img.write_to(&mut out, image::ImageFormat::Png);
    out.into_inner()
}

/// A typed value: its type, then the value.
fn variant(a: &mut Ar, v: &Tv, depth: usize) {
    a.u8(v.variant());
    match v {
        Tv::Byte(x) => a.u8(*x),
        Tv::Short(x) => a.u16(*x as u16),
        Tv::Int(x) => a.i32(*x),
        Tv::Float(x) => a.f32(*x),
        Tv::Double(x) => a.f64(*x),
        Tv::Bool(x) => a.bool(*x),
        Tv::Color(c) => a.color(*c),
        Tv::Time(t) => a.i32((*t).clamp(i32::MIN as i64, i32::MAX as i64) as i32),
        Tv::Str(s) => a.str(s),
        Tv::Array(items) => {
            let items: &[Tv] = if depth < 16 { items } else { &[] };
            a.i32(items.len() as i32);
            for it in items {
                variant(a, it, depth + 1);
            }
        }
        Tv::Point(p) | Tv::Vector(p) => a.f64s(p),
    }
}
