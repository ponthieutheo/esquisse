//! States of the model kept by the undo history, sharing what did not change.
//!
//! A [`Frozen`] state is the model without its geometry (the "skeleton": definitions'
//! names and attributes, materials, tags, scenes…, cheap to copy) and the geometry of each
//! collection (root and definitions) in four `Arc`s — vertices, edges, faces, instances —
//! and a fifth for the rest (guides, texts, section planes, properties, marks). Freezing the
//! model again after a change copies only the parts that differ from the previous state
//! (moving a group copies the instances of its collection, not its geometry); the others
//! are the same `Arc`. Restoring a state copies only the parts that differ from the model
//! as it is, and moves the others back.
//!
//! "Differ" is decided by an exact comparison ([`same_entities`]): floating-point values
//! bit for bit (a zero's sign included), every slot of the slot maps with its version (the
//! free slots too, which decide the keys of what is added later), lists in their order. The
//! comparison names every field of the model's types: a field added to one of them stops
//! the build here until it is compared too.

use std::collections::HashMap;
use std::sync::Arc;

use esq_core::glam::{DMat4, DVec2, DVec3};
use esq_core::model::{
    AnnotKind, Annotation, AttrValue, Attributes, DefId, Edge, EdgeId, Entities, Face, FaceId, Guide, Instance,
    InstanceId, Model, PointLight, SectionPlane, SnapPoint, Space, Vertex, VertexId,
};
use slotmap::SlotMap;

/// A collection's state: its four big slot maps and the rest (`rest` holds them empty).
#[derive(Clone)]
struct Coll {
    verts: Arc<SlotMap<VertexId, Vertex>>,
    edges: Arc<SlotMap<EdgeId, Edge>>,
    faces: Arc<SlotMap<FaceId, Face>>,
    instances: Arc<SlotMap<InstanceId, Instance>>,
    rest: Arc<Entities>,
}

/// The model's attribute dictionaries kept apart from entities (`Model::attr_store`): some
/// models carry tens of megabytes of them.
type AttrStore = std::collections::BTreeMap<String, Attributes>;

/// A state of the model (see the module's documentation).
#[derive(Clone)]
pub struct Frozen {
    /// The model without its collections nor `attr_store`.
    skel: Arc<Model>,
    attr_store: Arc<AttrStore>,
    root: Coll,
    defs: HashMap<DefId, Coll>,
}

/// What `tag_new` reads of a collection as it was.
pub struct Keys<'a> {
    pub edges: &'a SlotMap<EdgeId, Edge>,
    pub faces: &'a SlotMap<FaceId, Face>,
    pub instances: &'a SlotMap<InstanceId, Instance>,
    pub guides: &'a SlotMap<esq_core::model::GuideId, Guide>,
    pub annots: &'a SlotMap<esq_core::model::AnnotId, Annotation>,
    pub sections: &'a SlotMap<esq_core::model::SectionId, SectionPlane>,
}

impl Frozen {
    /// Approximate bytes of model this state owns alone (not shared with `other`).
    pub fn bytes_beyond(&self, other: Option<&Frozen>) -> u64 {
        fn alone<T>(a: &Arc<T>, b: Option<&Arc<T>>) -> bool {
            !b.is_some_and(|b| Arc::ptr_eq(a, b))
        }
        fn map<K: slotmap::Key, V>(m: &SlotMap<K, V>, lists: impl Fn(&V) -> usize) -> u64 {
            (m.capacity() * (std::mem::size_of::<V>() + 8) + m.values().map(lists).sum::<usize>() * 8) as u64
        }
        fn coll(a: &Coll, b: Option<&Coll>) -> u64 {
            let mut n = 0;
            if alone(&a.verts, b.map(|b| &b.verts)) {
                n += map(&a.verts, |v| v.edges.len());
            }
            if alone(&a.edges, b.map(|b| &b.edges)) {
                n += map(&a.edges, |e| e.faces.len());
            }
            if alone(&a.faces, b.map(|b| &b.faces)) {
                n += map(&a.faces, |f| f.outer.len() + f.holes.iter().map(Vec::len).sum::<usize>());
            }
            if alone(&a.instances, b.map(|b| &b.instances)) {
                n += map(&a.instances, |_| 0);
            }
            if alone(&a.rest, b.map(|b| &b.rest)) {
                n += crate::perf::entities_bytes(&a.rest);
            }
            n
        }
        let mut n = skeleton_bytes(&self.skel) + coll(&self.root, other.map(|o| &o.root));
        if alone(&self.attr_store, other.map(|o| &o.attr_store)) {
            // Keys and values, roughly.
            n += self.attr_store.values().map(|a| a.values().map(|d| d.len() as u64 * 64).sum::<u64>()).sum::<u64>();
        }
        for (d, c) in &self.defs {
            n += coll(c, other.and_then(|o| o.defs.get(d)));
        }
        n
    }

    /// Tests: how many parts of collections this state has, and how many it shares with
    /// `other`.
    #[cfg(test)]
    pub fn shared_parts(&self, other: &Frozen) -> (usize, usize) {
        let pair = |a: &Coll, b: Option<&Coll>| {
            let Some(b) = b else { return 0 };
            [
                Arc::ptr_eq(&a.verts, &b.verts),
                Arc::ptr_eq(&a.edges, &b.edges),
                Arc::ptr_eq(&a.faces, &b.faces),
                Arc::ptr_eq(&a.instances, &b.instances),
                Arc::ptr_eq(&a.rest, &b.rest),
            ]
            .iter()
            .filter(|s| **s)
            .count()
        };
        let mut shared = pair(&self.root, Some(&other.root));
        for (d, c) in &self.defs {
            shared += pair(c, other.defs.get(d));
        }
        (5 * (1 + self.defs.len()), shared)
    }

    /// The keys of a collection in this state (`None` = the root).
    pub fn keys(&self, def: Option<DefId>) -> Option<Keys<'_>> {
        let c = match def {
            None => &self.root,
            Some(d) => self.defs.get(&d)?,
        };
        Some(Keys {
            edges: &c.edges,
            faces: &c.faces,
            instances: &c.instances,
            guides: &c.rest.guides,
            annots: &c.rest.annots,
            sections: &c.rest.sections,
        })
    }
}

/// About how many bytes a copy of a model without its geometry takes: its definitions,
/// materials, tags and scenes (attributes and texts left out).
fn skeleton_bytes(m: &Model) -> u64 {
    use esq_core::model::{Definition, Layer, Material, Scene};
    use std::mem::size_of;
    (m.defs.capacity() * (size_of::<Definition>() + 8)
        + m.materials.capacity() * (size_of::<Material>() + 8)
        + m.layers.len() * size_of::<Layer>()
        + m.scenes.len() * size_of::<Scene>()) as u64
}

/// The collection without its four big slot maps.
fn rest_of(e: &Entities) -> Entities {
    let Entities {
        verts: _,
        edges: _,
        faces: _,
        instances: _,
        guides,
        annots,
        sections,
        active_section,
        next_curve,
        props,
        pids,
        snaps,
        lights,
        spaces,
        polylines,
        last_mark_id,
        replaced: _,
    } = e;
    Entities {
        verts: SlotMap::default(),
        edges: SlotMap::default(),
        faces: SlotMap::default(),
        instances: SlotMap::default(),
        guides: guides.clone(),
        annots: annots.clone(),
        sections: sections.clone(),
        active_section: *active_section,
        next_curve: *next_curve,
        props: props.clone(),
        pids: pids.clone(),
        snaps: snaps.clone(),
        lights: lights.clone(),
        spaces: spaces.clone(),
        polylines: polylines.clone(),
        last_mark_id: *last_mark_id,
        replaced: Vec::new(),
    }
}

/// A part of a collection: the previous state's when it is the same, else a copy.
fn keep<T: Clone>(live: &T, old: Option<&Arc<T>>, same: impl Fn(&T, &T) -> bool) -> Arc<T> {
    match old {
        Some(o) if same(o, live) => o.clone(),
        _ => Arc::new(live.clone()),
    }
}

fn freeze_coll(e: &Entities, prev: Option<&Coll>) -> Coll {
    Coll {
        verts: keep(&e.verts, prev.map(|p| &p.verts), |a, b| same_slots(a, b, vertex)),
        edges: keep(&e.edges, prev.map(|p| &p.edges), |a, b| same_slots(a, b, edge)),
        faces: keep(&e.faces, prev.map(|p| &p.faces), |a, b| same_slots(a, b, face)),
        instances: keep(&e.instances, prev.map(|p| &p.instances), |a, b| same_slots(a, b, instance)),
        rest: match prev.map(|p| &p.rest) {
            Some(o) if same_rest(o, e) => o.clone(),
            _ => Arc::new(rest_of(e)),
        },
    }
}

/// From this many vertices, edges and faces in the model, `freeze` compares the collections
/// and the attributes on several threads.
const PARALLEL: usize = 100_000;

/// `m`'s state, sharing with `prev` the parts that are the same.
pub fn freeze(m: &mut Model, prev: Option<&Frozen>) -> Frozen {
    // The root first, then the definitions.
    let colls: Vec<(Option<DefId>, &Entities)> =
        std::iter::once((None, &m.root)).chain(m.defs.iter().map(|(d, def)| (Some(d), &def.entities))).collect();
    let prev_of = |d: Option<DefId>| match d {
        None => prev.map(|p| &p.root),
        Some(d) => prev.and_then(|p| p.defs.get(&d)),
    };
    let attrs = || {
        keep(&m.attr_store, prev.map(|p| &p.attr_store), |a, b| {
            a.len() == b.len() && a.iter().zip(b).all(|((ka, va), (kb, vb))| ka == kb && attributes(va, vb))
        })
    };
    let size = |e: &Entities| e.verts.len() + e.edges.len() + e.faces.len();
    #[cfg(test)]
    let parallel = tests::PARALLEL_FROM.with(|c| c.get()).unwrap_or(PARALLEL);
    #[cfg(not(test))]
    let parallel = PARALLEL;
    let (attr_store, frozen) = if colls.iter().map(|(_, e)| size(e)).sum::<usize>() < parallel {
        (attrs(), colls.iter().map(|&(d, e)| freeze_coll(e, prev_of(d))).collect::<Vec<_>>())
    } else {
        // Each worker takes the next collection, the biggest first; they only read.
        let mut order: Vec<usize> = (0..colls.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(size(colls[i].1)));
        let next = std::sync::atomic::AtomicUsize::new(0);
        let workers = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 8);
        std::thread::scope(|s| {
            let a = s.spawn(attrs);
            let hs: Vec<_> = (0..workers)
                .map(|_| {
                    s.spawn(|| {
                        let mut done = Vec::new();
                        while let Some(&i) = order.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed)) {
                            let (d, e) = colls[i];
                            done.push((i, freeze_coll(e, prev_of(d))));
                        }
                        done
                    })
                })
                .collect();
            fn join<T>(h: std::thread::ScopedJoinHandle<'_, T>) -> T {
                h.join().unwrap_or_else(|e| std::panic::resume_unwind(e))
            }
            let mut all: Vec<(usize, Coll)> = hs.into_iter().flat_map(join).collect();
            all.sort_by_key(|x| x.0);
            (join(a), all.into_iter().map(|x| x.1).collect())
        })
    };
    let mut frozen = frozen.into_iter();
    let root = frozen.next().expect("la racine");
    let defs = colls[1..].iter().filter_map(|(d, _)| *d).zip(frozen).collect();
    // The skeleton: the model with its collections and attributes taken out meanwhile.
    let root_ents = std::mem::take(&mut m.root);
    let attrs = std::mem::take(&mut m.attr_store);
    let taken: Vec<(DefId, Entities)> =
        m.defs.iter_mut().map(|(d, def)| (d, std::mem::take(&mut def.entities))).collect();
    let skel = Arc::new(m.clone());
    m.root = root_ents;
    m.attr_store = attrs;
    for (d, e) in taken {
        m.defs[d].entities = e;
    }
    Frozen { skel, attr_store, root, defs }
}

/// A collection of state `s`: each part it shares with `live` (the state of `old`, the
/// collection as it is) is moved from `old`, the others are copied.
fn thaw_coll(s: &Coll, live: Option<&Coll>, old: Option<&mut Entities>) -> Entities {
    fn part<T: Clone + Default>(s: &Arc<T>, live: Option<&Arc<T>>, o: &mut T) -> T {
        if live.is_some_and(|l| Arc::ptr_eq(l, s)) { std::mem::take(o) } else { (**s).clone() }
    }
    let mut o = old.map(std::mem::take).unwrap_or_default();
    let verts = part(&s.verts, live.map(|l| &l.verts), &mut o.verts);
    let edges = part(&s.edges, live.map(|l| &l.edges), &mut o.edges);
    let faces = part(&s.faces, live.map(|l| &l.faces), &mut o.faces);
    let instances = part(&s.instances, live.map(|l| &l.instances), &mut o.instances);
    // The rest: the old collection's own when it is the same, else a copy.
    let mut e = if live.is_some_and(|l| Arc::ptr_eq(&l.rest, &s.rest)) { o } else { (*s.rest).clone() };
    (e.verts, e.edges, e.faces, e.instances) = (verts, edges, faces, instances);
    // What the undone steps replaced no longer applies to the state restored.
    e.replaced.clear();
    e
}

/// The model of state `s`. `live` is the state of `old`, the model as it is: the parts `s`
/// shares with it are moved from `old`, the others copied from `s`.
pub fn thaw(s: &Frozen, live: &Frozen, mut old: Model) -> Model {
    let mut m = (*s.skel).clone();
    m.attr_store = if Arc::ptr_eq(&s.attr_store, &live.attr_store) {
        std::mem::take(&mut old.attr_store)
    } else {
        (*s.attr_store).clone()
    };
    m.root = thaw_coll(&s.root, Some(&live.root), Some(&mut old.root));
    for (d, def) in m.defs.iter_mut() {
        let Some(c) = s.defs.get(&d) else { continue };
        def.entities = thaw_coll(c, live.defs.get(&d), old.defs.get_mut(d).map(|x| &mut x.entities));
    }
    m
}

// ---------------------------------------------------------------------------------------
// Exact comparison

fn f(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits()
}

fn f32s(a: f32, b: f32) -> bool {
    a.to_bits() == b.to_bits()
}

fn v3(a: DVec3, b: DVec3) -> bool {
    f(a.x, b.x) && f(a.y, b.y) && f(a.z, b.z)
}

fn v2(a: DVec2, b: DVec2) -> bool {
    f(a.x, b.x) && f(a.y, b.y)
}

fn fs(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| f(*x, *y))
}

fn v3s(a: &[DVec3], b: &[DVec3]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| v3(*x, *y))
}

fn m4(a: &DMat4, b: &DMat4) -> bool {
    fs(&a.to_cols_array(), &b.to_cols_array())
}

/// Two slot maps hold the same values under the same keys, and their free slots have the
/// same versions.
fn same_slots<K: slotmap::Key, V: serde::Serialize>(
    a: &slotmap::SlotMap<K, V>,
    b: &slotmap::SlotMap<K, V>,
    eq: impl Fn(&V, &V) -> bool,
) -> bool {
    a.len() == b.len()
        && a.iter().zip(b.iter()).all(|((ka, va), (kb, vb))| ka == kb && eq(va, vb))
        && versions::of(a) == versions::of(b)
}

fn json(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    use serde_json::Value as J;
    match (a, b) {
        (J::Number(x), J::Number(y)) => {
            x.as_i64() == y.as_i64()
                && x.as_u64() == y.as_u64()
                && x.as_f64().map(f64::to_bits) == y.as_f64().map(f64::to_bits)
        }
        (J::Array(x), J::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| json(p, q)),
        (J::Object(x), J::Object(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|((ka, va), (kb, vb))| ka == kb && json(va, vb))
        }
        _ => a == b,
    }
}

fn attr(a: &AttrValue, b: &AttrValue) -> bool {
    match (a, b) {
        (AttrValue::Text(x), AttrValue::Text(y)) => x == y,
        (AttrValue::Number(x), AttrValue::Number(y)) => f(*x, *y),
        (AttrValue::Bool(x), AttrValue::Bool(y)) => x == y,
        (AttrValue::Json(x), AttrValue::Json(y)) => json(x, y),
        _ => false,
    }
}

fn dict(a: &std::collections::BTreeMap<String, AttrValue>, b: &std::collections::BTreeMap<String, AttrValue>) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|((ka, va), (kb, vb))| ka == kb && attr(va, vb))
}

fn attributes(a: &Attributes, b: &Attributes) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|((ka, va), (kb, vb))| ka == kb && dict(va, vb))
}

fn uv(a: &esq_core::meta::UvMap, b: &esq_core::meta::UvMap) -> bool {
    let esq_core::meta::UvMap { u, v, q } = a;
    fs(u, &b.u) && fs(v, &b.v) && fs(q, &b.q)
}

fn opt<T>(a: &Option<T>, b: &Option<T>, eq: impl Fn(&T, &T) -> bool) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => eq(x, y),
        (None, None) => true,
        _ => false,
    }
}

fn tex(a: &esq_core::meta::FaceTex, b: &esq_core::meta::FaceTex) -> bool {
    let esq_core::meta::FaceTex { front, back, projection, pins } = a;
    opt(front, &b.front, uv)
        && opt(back, &b.back, uv)
        && (0..2).all(|i| opt(&projection[i], &b.projection[i], |x, y| v3(*x, *y)))
        && (0..2).all(|i| {
            pins[i].len() == b.pins[i].len()
                && pins[i].iter().zip(&b.pins[i]).all(|(p, q)| v3(p.0, q.0) && v2(p.1, q.1))
        })
}

fn vertex(a: &Vertex, b: &Vertex) -> bool {
    let Vertex { pos, edges } = a;
    v3(*pos, b.pos) && *edges == b.edges
}

fn edge(a: &Edge, b: &Edge) -> bool {
    let Edge { v, soft, smooth, hidden, layer, curve, faces } = a;
    *v == b.v
        && *soft == b.soft
        && *smooth == b.smooth
        && *hidden == b.hidden
        && *layer == b.layer
        && *curve == b.curve
        && *faces == b.faces
}

fn face(a: &Face, b: &Face) -> bool {
    let Face { outer, holes, normal, front, back, layer, hidden, tex: t } = a;
    *outer == b.outer
        && *holes == b.holes
        && v3(*normal, b.normal)
        && *front == b.front
        && *back == b.back
        && *layer == b.layer
        && *hidden == b.hidden
        && opt(t, &b.tex, |x, y| tex(x, y))
}

fn instance(a: &Instance, b: &Instance) -> bool {
    let Instance { def, xform, name, material, layer, hidden, locked, attributes: at, ext } = a;
    *def == b.def
        && m4(xform, &b.xform)
        && *name == b.name
        && *material == b.material
        && *layer == b.layer
        && *hidden == b.hidden
        && *locked == b.locked
        && attributes(at, &b.attributes)
        && *ext == b.ext
}

fn guide(a: &Guide, b: &Guide) -> bool {
    let Guide { pos, dir, infinite, layer, hidden } = a;
    v3(*pos, b.pos) && v3(*dir, b.dir) && *infinite == b.infinite && *layer == b.layer && *hidden == b.hidden
}

/// Two attached ends, exactly (their parameters bit for bit).
fn end(a: &esq_core::attach::Attach, b: &esq_core::attach::Attach) -> bool {
    use esq_core::attach::AttachTo as T;
    let to = match (a.to, b.to) {
        (T::OnEdge(x, t), T::OnEdge(y, u)) | (T::OnArc(x, t), T::OnArc(y, u)) => x == y && f(t, u),
        (T::OnGuide(x, t), T::OnGuide(y, u)) => x == y && f(t, u),
        (T::OnFace(x, u, v), T::OnFace(y, u2, v2)) => x == y && f(u, u2) && f(v, v2),
        (T::Section(x, p), T::Section(y, q)) => x == y && v3(p, q),
        (x, y) => x == y,
    };
    to && a.path == b.path
}

fn annot(a: &Annotation, b: &Annotation) -> bool {
    let Annotation { kind, text, layer, hidden, attach } = a;
    let attach_same = (0..2).all(|w| opt(&attach[w], &b.attach[w], end));
    let kind_same = match (kind, &b.kind) {
        (AnnotKind::Linear { a, b, offset }, AnnotKind::Linear { a: a2, b: b2, offset: o2 }) => {
            v3(*a, *a2) && v3(*b, *b2) && v3(*offset, *o2)
        }
        (AnnotKind::Text { point, leader }, AnnotKind::Text { point: p2, leader: l2 }) => {
            v3(*point, *p2) && v3(*leader, *l2)
        }
        (AnnotKind::ScreenText { x, y }, AnnotKind::ScreenText { x: x2, y: y2 }) => f(*x, *x2) && f(*y, *y2),
        _ => false,
    };
    kind_same && attach_same && *text == b.text && *layer == b.layer && *hidden == b.hidden
}

fn section(a: &SectionPlane, b: &SectionPlane) -> bool {
    let SectionPlane { pos, normal, name, symbol, layer, hidden, troubleshoot } = a;
    v3(*pos, b.pos)
        && v3(*normal, b.normal)
        && *name == b.name
        && *symbol == b.symbol
        && *layer == b.layer
        && *hidden == b.hidden
        && *troubleshoot == b.troubleshoot
}

fn snap(a: &SnapPoint, b: &SnapPoint) -> bool {
    let SnapPoint { pos, dir, up, id, hidden, layer } = a;
    v3(*pos, b.pos) && v3(*dir, b.dir) && v3(*up, b.up) && *id == b.id && *hidden == b.hidden && *layer == b.layer
}

fn light(a: &PointLight, b: &PointLight) -> bool {
    let PointLight { pos, color, intensity, size } = a;
    v3(*pos, b.pos)
        && (0..3).all(|i| f32s(color[i], b.color[i]))
        && f32s(*intensity, b.intensity)
        && f32s(*size, b.size)
}

fn space(a: &Space, b: &Space) -> bool {
    let Space { id, name, perimeter, holes, orientation, ceiling, hidden, layer } = a;
    *id == b.id
        && *name == b.name
        && v3s(perimeter, &b.perimeter)
        && holes.len() == b.holes.len()
        && holes.iter().zip(&b.holes).all(|(x, y)| v3s(x, y))
        && v3(*orientation, b.orientation)
        && fs(ceiling, &b.ceiling)
        && *hidden == b.hidden
        && *layer == b.layer
}

fn list<T>(a: &[T], b: &[T], eq: impl Fn(&T, &T) -> bool) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| eq(x, y))
}

/// Whether two collections are the same, exactly (see the module's documentation).
#[cfg_attr(not(test), allow(dead_code))]
pub fn same_entities(a: &Entities, b: &Entities) -> bool {
    same_rest(a, b)
        && same_slots(&a.instances, &b.instances, instance)
        && same_slots(&a.faces, &b.faces, face)
        && same_slots(&a.edges, &b.edges, edge)
        && same_slots(&a.verts, &b.verts, vertex)
}

/// Whether two collections are the same apart from their vertices, edges, faces and
/// instances.
fn same_rest(a: &Entities, b: &Entities) -> bool {
    let Entities {
        verts: _,
        edges: _,
        faces: _,
        instances: _,
        guides,
        annots,
        sections,
        active_section,
        next_curve,
        props,
        pids,
        snaps,
        lights,
        spaces,
        polylines,
        last_mark_id,
        replaced: _,
    } = a;
    *active_section == b.active_section
        && *next_curve == b.next_curve
        && *last_mark_id == b.last_mark_id
        && props.len() == b.props.len()
        && props.iter().zip(&b.props).all(|((ka, va), (kb, vb))| ka == kb && dict(va, vb))
        && *pids == b.pids
        && list(snaps, &b.snaps, snap)
        && list(lights, &b.lights, light)
        && list(spaces, &b.spaces, space)
        && *polylines == b.polylines
        && same_slots(guides, &b.guides, guide)
        && same_slots(annots, &b.annots, annot)
        && same_slots(sections, &b.sections, section)
}

#[cfg(test)]
mod tests {
    use super::*;
    use esq_core::glam::DVec3;
    use esq_core::model::{Definition, EntityRef};

    /// A box in the root and a heavy component placed twice.
    fn model() -> Model {
        let mut m = Model::new();
        let square = |e: &mut Entities, p: DVec3| {
            let c =
                [DVec3::ZERO, DVec3::X, DVec3::new(1.0, 1.0, 0.0), DVec3::Y].map(|q| e.add_vertex_raw(p + q * 10.0));
            for k in 0..4 {
                e.add_edge_raw(c[k], c[(k + 1) % 4]);
            }
            e.add_face_raw(c.to_vec(), Vec::new());
        };
        square(&mut m.root, DVec3::ZERO);
        let mut heavy = Entities::default();
        for i in 0..2000 {
            square(&mut heavy, DVec3::new(i as f64 * 20.0, 0.0, 0.0));
        }
        let d = m.defs.insert(Definition {
            name: "Lourd".into(),
            is_group: false,
            description: String::new(),
            entities: heavy,
            attributes: Default::default(),
            ext: Default::default(),
        });
        for x in [0.0, 500.0] {
            m.root.instances.insert(esq_core::model::Instance {
                def: d,
                xform: esq_core::glam::DMat4::from_translation(DVec3::new(x, 100.0, 0.0)),
                name: String::new(),
                material: None,
                layer: 0,
                hidden: false,
                locked: false,
                attributes: Default::default(),
                ext: Default::default(),
            });
        }
        m
    }

    fn json(m: &Model) -> String {
        serde_json::to_string(m).unwrap()
    }

    thread_local! {
        /// Tests: the size from which `freeze` works on several threads (`PARALLEL`).
        pub static PARALLEL_FROM: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
    }

    /// Which parts of each collection `s` shares with `prev`, the root first, then the
    /// definitions in their order.
    fn sharing(s: &Frozen, prev: &Frozen, m: &Model) -> Vec<[bool; 5]> {
        let pair = |a: &Coll, b: &Coll| {
            [
                Arc::ptr_eq(&a.verts, &b.verts),
                Arc::ptr_eq(&a.edges, &b.edges),
                Arc::ptr_eq(&a.faces, &b.faces),
                Arc::ptr_eq(&a.instances, &b.instances),
                Arc::ptr_eq(&a.rest, &b.rest),
            ]
        };
        let mut v = vec![pair(&s.root, &prev.root)];
        v.extend(m.defs.keys().map(|d| pair(&s.defs[&d], &prev.defs[&d])));
        v.push([Arc::ptr_eq(&s.attr_store, &prev.attr_store); 5]);
        v
    }

    /// The collections and the attributes compared on several threads: the same parts
    /// shared, the same models restored, as on one.
    #[test]
    fn freezing_on_several_threads_shares_and_restores_the_same() {
        let mut start = model();
        for k in 0..7 {
            let mut e = Entities::default();
            for i in 0..=k * 3 {
                let c = [DVec3::ZERO, DVec3::X, DVec3::Y].map(|q| e.add_vertex_raw(q * (i + 5) as f64));
                e.add_face_raw(c.to_vec(), Vec::new());
            }
            start.defs.insert(Definition {
                name: format!("D{k}"),
                is_group: false,
                description: String::new(),
                entities: e,
                attributes: Default::default(),
                ext: Default::default(),
            });
        }
        start.attr_store.insert("dico".into(), Default::default());
        let run = |from: usize| {
            PARALLEL_FROM.with(|c| c.set(Some(from)));
            let mut m = start.clone();
            let mut states: Vec<(Frozen, String)> = Vec::new();
            let mut shared = Vec::new();
            let defs: Vec<DefId> = m.defs.keys().collect();
            for k in 0..12 {
                match k % 4 {
                    0 => {
                        let e = &mut m.defs[defs[k % defs.len()]].entities;
                        let v = e.verts.keys().next().unwrap();
                        e.verts[v].pos.z += 1.0;
                    }
                    1 => {
                        let i = m.root.instances.keys().next().unwrap();
                        m.root.instances[i].xform = DMat4::from_translation(DVec3::Z * k as f64);
                    }
                    2 => {
                        m.attr_store.insert(format!("k{k}"), Default::default());
                    }
                    _ => {}
                }
                let s = freeze(&mut m, states.last().map(|x| &x.0));
                if let Some((p, _)) = states.last() {
                    shared.push(sharing(&s, p, &m));
                }
                states.push((s, json(&m)));
            }
            // Every state restored, from the last one.
            let mut live = states.last().unwrap().0.clone();
            for (s, j) in states.iter().rev() {
                m = thaw(s, &live, m);
                assert_eq!(&json(&m), j);
                live = s.clone();
            }
            PARALLEL_FROM.with(|c| c.set(None));
            shared
        };
        let one = run(usize::MAX);
        let several = run(0);
        assert_eq!(one, several);
        // Each step shares what it did not change.
        assert!(one.iter().all(|s| s.iter().flatten().filter(|x| !**x).count() <= 5), "{one:?}");
    }

    #[test]
    fn exactly_the_same_collections_are_the_same() {
        let a = model();
        let d = a.defs.keys().next().unwrap();
        assert!(same_entities(&a.root, &a.root.clone()));
        assert!(same_entities(&a.defs[d].entities, &a.defs[d].entities.clone()));
        type Change = fn(&mut Entities);
        let changes: [(&str, Change); 6] = [
            ("signe d'un zéro", |e| {
                let v = e.verts.keys().next().unwrap();
                e.verts[v].pos.x = -0.0;
            }),
            ("emplacement libéré puis repris", |e| {
                let v = e.add_vertex_raw(DVec3::splat(3.0));
                e.verts.remove(v);
            }),
            ("ordre des arêtes d'un sommet", |e| {
                let v = e.verts.keys().next().unwrap();
                e.verts[v].edges.reverse();
            }),
            ("attribut -0", |e| {
                e.set_prop("f1", "x", AttrValue::Json(serde_json::json!({ "v": -0.0 })));
            }),
            ("plan de section (vue)", |e| {
                let s = e.sections.insert(SectionPlane::new(DVec3::ZERO, DVec3::Z));
                e.sections[s].troubleshoot = true;
            }),
            ("position de texture", |e| {
                let f = e.faces.keys().next().unwrap();
                e.faces[f].tex = Some(Box::default());
            }),
        ];
        for (what, change) in changes {
            let mut b = a.root.clone();
            change(&mut b);
            assert!(!same_entities(&a.root, &b), "{what} : changement non vu");
        }
        // -0.0 and 0.0 in an attribute are different numbers for the file too.
        let (mut x, mut y) = (a.root.clone(), a.root.clone());
        x.set_prop("f1", "x", AttrValue::Json(serde_json::json!(0.0)));
        y.set_prop("f1", "x", AttrValue::Json(serde_json::json!(-0.0)));
        assert!(!same_entities(&x, &y));
    }

    #[test]
    fn the_history_copies_what_changes_and_undoes_exactly() {
        let mut doc = crate::doc::Document::new(model());
        let start = json(&doc.model);
        let heavy = crate::perf::model_bytes(&doc.model);
        let b0 = crate::perf::get().undo_bytes;
        let mut states = vec![start.clone()];
        for k in 0..50 {
            let target = doc.model.root.edges.keys().next().unwrap();
            doc.commit("Déplacer", |m, d| {
                let e = m.entities_mut(d);
                e.transform_selection(&[EntityRef::Edge(target)], &esq_core::glam::DMat4::from_translation(DVec3::X));
                if k % 10 == 3 {
                    // Now and then something added and taken away again (free slots).
                    let v = e.add_vertex_raw(DVec3::splat(k as f64));
                    e.verts.remove(v);
                }
            });
            states.push(json(&doc.model));
        }
        let per_op = (crate::perf::get().undo_bytes - b0) / 50;
        // The component (most of the model) is copied once at most, not at each step.
        assert!(per_op * 20 < heavy, "{per_op} octets copiés par opération pour un modèle de {heavy}");
        for k in (0..50).rev() {
            doc.undo();
            assert_eq!(json(&doc.model), states[k], "annulation {k}");
        }
        for (k, state) in states.iter().enumerate().skip(1) {
            doc.redo();
            assert_eq!(&json(&doc.model), state, "rétablissement {k}");
        }
        // A failed change leaves the model as it was.
        let r: Result<(), &str> = doc.try_commit("Échec", |m, _| {
            m.root.verts.clear();
            Err("non")
        });
        assert!(r.is_err());
        assert_eq!(json(&doc.model), states[50]);
        // Moving a copy of the component in a root full of geometry copies the instances of
        // the root, not its geometry.
        let mut m = Model::new();
        m.root = std::mem::take(&mut doc.model.defs.values_mut().next().unwrap().entities);
        let d = m.defs.insert(Definition {
            name: "Petit".into(),
            is_group: false,
            description: String::new(),
            entities: Entities::default(),
            attributes: Default::default(),
            ext: Default::default(),
        });
        for k in 0..20 {
            m.root.instances.insert(esq_core::model::Instance {
                def: d,
                xform: esq_core::glam::DMat4::from_translation(DVec3::splat(k as f64)),
                name: String::new(),
                material: None,
                layer: 0,
                hidden: false,
                locked: false,
                attributes: Default::default(),
                ext: Default::default(),
            });
        }
        let geometry = crate::perf::entities_bytes(&m.root);
        let mut big = crate::doc::Document::new(m);
        let target = big.model.root.instances.keys().next().unwrap();
        big.commit("Déplacer", |_, _| {});
        let b0 = crate::perf::get().undo_bytes;
        let before = json(&big.model);
        big.commit("Déplacer", |m, d| {
            m.entities_mut(d).transform_selection(
                &[EntityRef::Instance(target)],
                &esq_core::glam::DMat4::from_translation(DVec3::X),
            );
        });
        let copied = crate::perf::get().undo_bytes - b0;
        assert!(copied * 50 < geometry, "{copied} octets copiés pour une collection de {geometry}");
        big.undo();
        assert_eq!(json(&big.model), before);
    }
}

/// The version of every slot of a slot map, free ones included: what its serialization
/// writes besides the values (`SlotMap` writes its slots as `{ value, version }`).
mod versions {
    use serde::ser::{self, Impossible, Serialize, SerializeSeq, SerializeStruct, Serializer};

    #[derive(Debug)]
    pub struct Stop;

    impl std::fmt::Display for Stop {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("not a slot map")
        }
    }

    impl std::error::Error for Stop {}

    impl ser::Error for Stop {
        fn custom<T: std::fmt::Display>(_: T) -> Self {
            Stop
        }
    }

    pub fn of<T: Serialize>(map: &T) -> Option<Vec<u32>> {
        let mut out = Vec::new();
        map.serialize(Seq(&mut out)).ok()?;
        Some(out)
    }

    /// The serializers below refuse everything but what they expect.
    macro_rules! refuse {
        ($($name:ident($($t:ty),*)),* $(,)?) => {
            $(fn $name(self, $(_: $t),*) -> Result<(), Stop> { Err(Stop) })*
        };
    }

    macro_rules! refuse_compound {
        () => {
            type SerializeTuple = Impossible<(), Stop>;
            type SerializeTupleStruct = Impossible<(), Stop>;
            type SerializeTupleVariant = Impossible<(), Stop>;
            type SerializeMap = Impossible<(), Stop>;
            type SerializeStructVariant = Impossible<(), Stop>;
            refuse!(
                serialize_bool(bool),
                serialize_i8(i8),
                serialize_i16(i16),
                serialize_i32(i32),
                serialize_i64(i64),
                serialize_u8(u8),
                serialize_u16(u16),
                serialize_u64(u64),
                serialize_f32(f32),
                serialize_f64(f64),
                serialize_char(char),
                serialize_str(&str),
                serialize_bytes(&[u8]),
                serialize_none(),
                serialize_unit(),
                serialize_unit_struct(&'static str),
                serialize_unit_variant(&'static str, u32, &'static str),
            );
            fn serialize_some<T: ?Sized + Serialize>(self, _: &T) -> Result<(), Stop> {
                Err(Stop)
            }
            fn serialize_newtype_struct<T: ?Sized + Serialize>(self, _: &'static str, _: &T) -> Result<(), Stop> {
                Err(Stop)
            }
            fn serialize_newtype_variant<T: ?Sized + Serialize>(
                self,
                _: &'static str,
                _: u32,
                _: &'static str,
                _: &T,
            ) -> Result<(), Stop> {
                Err(Stop)
            }
            fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, Stop> {
                Err(Stop)
            }
            fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeTupleStruct, Stop> {
                Err(Stop)
            }
            fn serialize_tuple_variant(
                self,
                _: &'static str,
                _: u32,
                _: &'static str,
                _: usize,
            ) -> Result<Self::SerializeTupleVariant, Stop> {
                Err(Stop)
            }
            fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, Stop> {
                Err(Stop)
            }
            fn serialize_struct_variant(
                self,
                _: &'static str,
                _: u32,
                _: &'static str,
                _: usize,
            ) -> Result<Self::SerializeStructVariant, Stop> {
                Err(Stop)
            }
        };
    }

    /// The slot map: a sequence of slots.
    struct Seq<'a>(&'a mut Vec<u32>);

    impl<'a> Serializer for Seq<'a> {
        type Ok = ();
        type Error = Stop;
        type SerializeSeq = Self;
        type SerializeStruct = Impossible<(), Stop>;
        refuse_compound!();
        refuse!(serialize_u32(u32));
        fn serialize_seq(self, _: Option<usize>) -> Result<Self, Stop> {
            Ok(self)
        }
        fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeStruct, Stop> {
            Err(Stop)
        }
    }

    impl SerializeSeq for Seq<'_> {
        type Ok = ();
        type Error = Stop;
        fn serialize_element<T: ?Sized + Serialize>(&mut self, v: &T) -> Result<(), Stop> {
            v.serialize(Slot(self.0))
        }
        fn end(self) -> Result<(), Stop> {
            Ok(())
        }
    }

    /// A slot: `{ value, version }`; only the version is read.
    struct Slot<'a>(&'a mut Vec<u32>);

    impl<'a> Serializer for Slot<'a> {
        type Ok = ();
        type Error = Stop;
        type SerializeSeq = Impossible<(), Stop>;
        type SerializeStruct = Self;
        refuse_compound!();
        refuse!(serialize_u32(u32));
        fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Stop> {
            Err(Stop)
        }
        fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self, Stop> {
            Ok(self)
        }
    }

    impl SerializeStruct for Slot<'_> {
        type Ok = ();
        type Error = Stop;
        fn serialize_field<T: ?Sized + Serialize>(&mut self, key: &'static str, v: &T) -> Result<(), Stop> {
            if key == "version" { v.serialize(Version(self.0)) } else { Ok(()) }
        }
        fn end(self) -> Result<(), Stop> {
            Ok(())
        }
    }

    struct Version<'a>(&'a mut Vec<u32>);

    impl<'a> Serializer for Version<'a> {
        type Ok = ();
        type Error = Stop;
        type SerializeSeq = Impossible<(), Stop>;
        type SerializeStruct = Impossible<(), Stop>;
        refuse_compound!();
        fn serialize_u32(self, v: u32) -> Result<(), Stop> {
            self.0.push(v);
            Ok(())
        }
        fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Stop> {
            Err(Stop)
        }
        fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeStruct, Stop> {
            Err(Stop)
        }
    }
}
