//! Turns the model into GPU vertex lists, and keeps a world-space copy for picking.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use esq_core::glam::{DMat4, DVec2, DVec3};
use esq_core::math::TOL;
use esq_core::meta::{UvMap, default_uv};
use esq_core::model::{DefId, EdgeId, Entities, EntityRef, InstanceId, MaterialId, Model, SectionPlane};

use crate::textures::Texels;
use esq_core::section::{chain_loops, clip_segment, face_cut, fill_triangles};
use esq_core::tess::face_triangles;
use esq_core::walk::{visit, visit_all};

use crate::style::{FaceStyle, ViewStyle};

// The colours come from the style (`ViewStyle`); this is the default face colour, shown by
// the Materials panel for faces without a material.
pub const FRONT: [u8; 3] = [255, 255, 255];
/// The default back colour (the Simple style's).
pub const BACK: [u8; 3] = [162, 183, 198];
/// Colour of the Section Plane tool's preview (the default style's active section colour).
pub const SECTION_PLANE: [u8; 3] = [255, 135, 0];
/// The shader can cut with this many planes at once.
pub const MAX_CLIPS: usize = 16;
/// Line flags from this bit up say which clip planes cut the line.
pub const CLIP_SHIFT: u32 = 8;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct FaceVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    /// Colour of each side; on a textured side, the colour the texture is mixed towards
    /// (selection, fading), see `tex`.
    pub front: [f32; 4],
    pub back: [f32; 4],
    /// Bit i set: clip plane i cuts this face.
    pub clip: u32,
    /// Homogeneous texture coordinates (u, v, q) of the front and back sides, in tiles: the
    /// fragment shader divides by q, so that a texture distorted in perspective (4 pins)
    /// shows no seam where the face's triangles meet.
    pub uv_front: [f32; 3],
    pub uv_back: [f32; 3],
    /// Texture layer + 1 of the front side (bits 0–8) and of the back side (bits 9–17), 0 when
    /// untextured; bits 18–25: how much of the side's colour covers its texture (0–255).
    pub tex: u32,
}

impl FaceVertex {
    pub fn plain(pos: DVec3, normal: DVec3, front: [f32; 4], back: [f32; 4], clip: u32) -> Self {
        let uv = [0.0, 0.0, 1.0];
        FaceVertex { pos: v3(pos), normal: v3(normal), front, back, clip, uv_front: uv, uv_back: uv, tex: 0 }
    }
}

/// The texture array holds this many layers at most (wgpu's guaranteed minimum).
pub const MAX_TEX_LAYERS: usize = 256;
const TEX_BACK_SHIFT: u32 = 9;
const TEX_MIX_SHIFT: u32 = 18;
/// The texture does not repeat (an image's picture).
const TEX_CLAMP: u32 = 1 << 26;
/// Drawn only in the sun's view / never in it (Components › Edit › Shadows face sun).
pub const SHADOW_ONLY: u32 = 1 << 27;
pub const NO_SHADOW: u32 = 1 << 28;
/// The face shows no cast shadow on it (Entity Info › Receive Shadows off).
pub const NO_RECEIVE: u32 = 1 << 29;

/// One line segment, drawn as a screen-aligned quad of `width` pixels.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct LineInst {
    pub a: [f32; 3],
    pub b: [f32; 3],
    /// Directions from the edge into its first and second face (zero when absent). The
    /// shader uses them to draw silhouette edges ("profiles") thicker.
    pub t1: [f32; 3],
    pub t2: [f32; 3],
    pub color: [f32; 4],
    pub width: f32,
    pub flags: u32,
}

pub const LINE_DASHED: u32 = 1;
pub const LINE_PROFILE: u32 = 2;
/// Drawn even when View › Edges is off (axes, guides, selection).
pub const LINE_KEEP: u32 = 4;
/// Shown dashed where faces hide it, whatever the edge style (section planes).
pub const LINE_BACK: u32 = 8;
/// An edge of the model: the edge style (extension, endpoints, jitter) applies to it.
pub const LINE_EDGE: u32 = 16;
/// Bits 5-6: the thick end of an edge at its start (1) or its end (2), Edge style > Endpoints.
pub const CAP_SHIFT: u32 = 5;
/// Bits 24–27: the dash pattern of the edge's tag (see `dash_pattern`), 0 = solid.
pub const DASH_SHIFT: u32 = 24;
/// Bits 28–29: which of the three strokes of a jittered edge this is (0 = not jittered).
pub const JITTER_SHIFT: u32 = 28;

/// Length used to draw infinite lines (axes, guides).
const FAR: f64 = 1.0e6;

#[derive(Default)]
pub struct SceneMesh {
    pub opaque: Vec<FaceVertex>,
    pub transparent: Vec<FaceVertex>,
    pub lines: Vec<LineInst>,
    /// Active section planes in world coordinates (normal, offset): the part where
    /// `normal · p < offset` is cut away.
    pub clips: Vec<[f32; 4]>,
    /// Bounding box of the faces (world), for the sun's view.
    pub bounds: Option<(DVec3, DVec3)>,
    /// The textures the faces use, one per layer of the renderer's texture array.
    pub textures: Vec<Arc<Texels>>,
}

/// The textured side of a face: which layer, how its points map to texture coordinates,
/// and whether some of its pixels are transparent.
struct SideTex {
    layer: u32,
    map: UvMap,
    alpha: bool,
}

/// Assigns texture layers to materials while the scene is built.
#[derive(Default)]
struct TexTable {
    layers: Vec<Arc<Texels>>,
    by_ptr: HashMap<usize, u32>,
    by_mat: HashMap<MaterialId, Option<(u32, [f64; 2], bool)>>,
}

impl TexTable {
    /// Layer (+ 1), tile size (mm) and transparency of a material's texture.
    fn material(&mut self, model: &Model, m: MaterialId) -> Option<(u32, [f64; 2], bool)> {
        if let Some(r) = self.by_mat.get(&m) {
            return *r;
        }
        let r = (|| {
            let mat = model.materials.get(m)?;
            let tile = mat.ext.texture.as_ref()?.size;
            let texels = crate::textures::of_material(&mat.ext)?;
            let ptr = Arc::as_ptr(&texels) as usize;
            let layer = match self.by_ptr.get(&ptr) {
                Some(l) => *l,
                None if self.layers.len() < MAX_TEX_LAYERS => {
                    self.layers.push(texels.clone());
                    let l = self.layers.len() as u32;
                    self.by_ptr.insert(ptr, l);
                    l
                }
                None => return None,
            };
            Some((layer, tile, texels.alpha))
        })();
        self.by_mat.insert(m, r);
        r
    }

    fn side(&mut self, model: &Model, m: Option<MaterialId>, map: Option<UvMap>, normal: DVec3) -> Option<SideTex> {
        let (layer, tile, alpha) = self.material(model, m?)?;
        Some(SideTex { layer, map: map.unwrap_or_else(|| default_uv(normal, tile)), alpha })
    }
}

/// The colour a texture shows through: `mix(texture, target, t)`. Starts as the bare texture.
#[derive(Clone, Copy)]
struct Veil {
    target: [f32; 3],
    t: f32,
}

impl Veil {
    const NONE: Veil = Veil { target: [1.0; 3], t: 0.0 };

    /// The veil after mixing the result with `c` by `a` (as `mix` does on plain colours).
    fn then(self, c: [u8; 3], a: f32) -> Veil {
        let t = 1.0 - (1.0 - self.t) * (1.0 - a);
        if t <= 0.0 {
            return self;
        }
        let c = c.map(|v| v as f32 / 255.0);
        let target = [0, 1, 2].map(|i| (self.target[i] * self.t * (1.0 - a) + c[i] * a) / t);
        Veil { target, t }
    }
}

pub struct SceneOptions<'a> {
    pub ctx: &'a [InstanceId],
    /// Definition being edited (to hide similar components).
    pub ctx_def: Option<DefId>,
    pub sel: &'a [EntityRef],
    pub axes_xform: DMat4,
    pub style: &'a ViewStyle,
}

/// Whether an entity casts shadows: unless its property `casts` (Ruby's `casts_shadows=`, a
/// .skp's `SUDrawingElementGetCastsShadows`) is false. `kind` + id is the key of `Entities::props`.
fn casts(e: &Entities, kind: char, id: u64) -> bool {
    flag_on(e, kind, id, "casts")
}

/// A shadow flag of an entity (`casts`, `receives`): on unless its property says false.
fn flag_on(e: &Entities, kind: char, id: u64, name: &str) -> bool {
    // Most collections have no property at all: no key to make for them.
    e.props.is_empty() || !matches!(e.prop(&format!("{kind}{id}"), name), Some(esq_core::model::AttrValue::Bool(false)))
}

/// What `build` works out once per collection, however many times it is placed: the
/// openings cut in its faces, the triangles of each face (its own coordinates, openings
/// cut, in the order of `Entities::faces`), and the sides of its edges.
#[derive(Default)]
struct Coll {
    openings: Option<HashMap<esq_core::model::FaceId, Vec<Vec<DVec3>>>>,
    tris: Vec<Option<Vec<[DVec3; 3]>>>,
    sides: Option<HashMap<EdgeId, Vec<DVec3>>>,
}

fn line(a: DVec3, b: DVec3, color: [f32; 4], width: f32, flags: u32) -> LineInst {
    LineInst { a: v3(a), b: v3(b), t1: [0.0; 3], t2: [0.0; 3], color, width, flags }
}

/// For each edge, the directions pointing from it into the visible faces it bounds.
fn edge_sides(model: &Model, ents: &Entities) -> HashMap<EdgeId, Vec<DVec3>> {
    let vis = |l: usize| model.layers.get(l).is_none_or(|x| x.visible);
    let mut out: HashMap<EdgeId, Vec<DVec3>> = HashMap::new();
    for (_, face) in ents.faces.iter() {
        if face.hidden || !vis(face.layer) {
            continue;
        }
        for l in face.loops() {
            for i in 0..l.len() {
                let (u, w) = (l[i], l[(i + 1) % l.len()]);
                let Some(e) = ents.find_edge(u, w) else { continue };
                // Loops turn counter-clockwise (holes clockwise): the face is on the left.
                let t = face.normal.cross(ents.pos(w) - ents.pos(u)).normalize_or_zero();
                out.entry(e).or_default().push(t);
            }
        }
    }
    out
}

fn rgba(c: [u8; 3], a: f32) -> [f32; 4] {
    [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, a]
}

fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    [0, 1, 2].map(|i| (a[i] as f32 * (1.0 - t) + b[i] as f32 * t).round() as u8)
}

fn v3(p: DVec3) -> [f32; 3] {
    [p.x as f32, p.y as f32, p.z as f32]
}

/// Red, green and blue of the axes (also the edge colours of Edge color › By axis).
pub const AXIS_COLORS: [[u8; 3]; 3] = [[220, 30, 30], [20, 150, 40], [30, 50, 230]];

/// Troubleshoot Section Fill: the colour of what keeps a cut open.
pub const TROUBLESHOOT_RED: [u8; 3] = [230, 30, 30];

/// Colour of a tag that has none (as `Layer#color` reports it).
pub const TAG_GREY: [u8; 3] = [128, 128, 128];

/// Display colour of tag `l` (Color by tag).
pub fn tag_color(model: &Model, l: usize) -> [u8; 3] {
    model.layers.get(l).and_then(|x| x.color).unwrap_or(TAG_GREY)
}

/// The tag a collection placed through `path` shows for its untagged entities: the tag of
/// the innermost tagged group or component around it.
fn path_tag(model: &Model, path: &[InstanceId]) -> usize {
    let mut ents = &model.root;
    let mut tag = 0;
    for &i in path {
        let Some(inst) = ents.instances.get(i) else { break };
        if inst.layer != 0 {
            tag = inst.layer;
        }
        let Some(d) = model.defs.get(inst.def) else { break };
        ents = &d.entities;
    }
    tag
}

/// The dash pattern drawn for a tag's line style (Edge style › Dashes), 0 for a solid line:
/// SketchUp's patterns, and the names the Ruby API gives some of them.
pub fn dash_pattern(name: &str) -> u32 {
    match name {
        "Dotted Basic" | "Dotted Line" => 1,
        "Short Dashes Basic" | "Short Dashes Line" => 2,
        "Long Dashes Basic" | "Dashes Line" => 3,
        "Dot Dash Basic" | "Dash Dot Dash Line" => 4,
        "Center Basic" => 5,
        "Demolition Basic" => 6,
        "Property Basic" => 7,
        _ => 0,
    }
}

/// SketchUp's dash patterns, in the order of `dash_pattern`, with their French names.
pub const DASHES: [(&str, &str); 8] = [
    ("Solid Basic", "Continu"),
    ("Dotted Basic", "Pointillé"),
    ("Short Dashes Basic", "Tirets courts"),
    ("Long Dashes Basic", "Tirets longs"),
    ("Dot Dash Basic", "Point-tiret"),
    ("Center Basic", "Axe"),
    ("Demolition Basic", "Démolition"),
    ("Property Basic", "Limite de propriété"),
];

/// Colour of an edge that is neither selected nor hidden (Edge settings › Color): one colour
/// for all, the edge's material, or the colour of the axis it runs along. `tag`: colour the
/// edge by this tag instead (Color by tag, with edges by material).
fn edge_color(
    model: &Model,
    ents: &Entities,
    e: EdgeId,
    st: &ViewStyle,
    axes: DMat4,
    dir: DVec3,
    tag: Option<usize>,
) -> [u8; 3] {
    match st.edge_color_mode {
        crate::style::EdgeColor::AllSame => st.edge_color,
        crate::style::EdgeColor::ByMaterial => {
            if let Some(t) = tag {
                return tag_color(model, t);
            }
            if ents.props.is_empty() {
                return st.edge_color;
            }
            let key = format!("e{}", slotmap::Key::data(&e).as_ffi());
            match ents.prop(&key, "material") {
                Some(esq_core::model::AttrValue::Text(n)) => {
                    model.find_material(n).map_or(st.edge_color, |m| model.materials[m].color)
                }
                _ => st.edge_color,
            }
        }
        crate::style::EdgeColor::ByAxis => {
            let d = dir.normalize_or_zero();
            (0..3)
                .find(|&i| d.dot(axes.col(i).truncate().normalize_or_zero()).abs() > 1.0 - 1e-9)
                .map_or(st.edge_color, |i| AXIS_COLORS[i])
        }
    }
}

/// Whether a placed collection is the active context or inside it, and whether it is part
/// of the selection (a selected instance highlights everything it contains).
fn classify(path: &[InstanceId], ctx: &[InstanceId], sel: &HashSet<EntityRef>) -> (bool, bool) {
    if !path.starts_with(ctx) {
        return (false, false);
    }
    let selected = path.len() > ctx.len() && sel.contains(&EntityRef::Instance(path[ctx.len()]));
    (true, selected)
}

/// Whether the selected group or component a placed collection belongs to (the one right
/// under the open context on `path`) is locked.
fn selected_locked(model: &Model, ctx: &[InstanceId], path: &[InstanceId]) -> bool {
    let Some(&top) = path.get(ctx.len()) else { return false };
    let mut ents = &model.root;
    for &i in ctx {
        match ents.instances.get(i).and_then(|x| model.defs.get(x.def)) {
            Some(d) => ents = &d.entities,
            None => return false,
        }
    }
    ents.instances.get(top).is_some_and(|i| i.locked)
}

/// Slot of a section plane in the clip table (a plane seen twice shares its slot).
fn clip_slot(clips: &mut Vec<SectionPlane>, c: &SectionPlane) -> Option<usize> {
    if let Some(i) =
        clips.iter().position(|x| x.normal.dot(c.normal) > 1.0 - 1e-9 && (c.pos - x.pos).dot(x.normal).abs() < TOL)
    {
        return Some(i);
    }
    if clips.len() >= MAX_CLIPS {
        return None;
    }
    clips.push(c.clone());
    Some(clips.len() - 1)
}

/// Points of a collection and of everything it contains, in its own coordinates.
fn collection_points(model: &Model, ents: &Entities, m: &DMat4, depth: usize, out: &mut Vec<DVec3>) {
    out.extend(ents.verts.values().map(|v| m.transform_point3(v.pos)));
    if depth > 16 {
        return;
    }
    for inst in ents.instances.values() {
        if let Some(d) = model.defs.get(inst.def) {
            collection_points(model, &d.entities, &(*m * inst.xform), depth + 1, out);
        }
    }
}

/// Unit vectors along the sides of a section plane's rectangle: horizontal, then up the
/// plane (along red and green for a horizontal plane).
pub fn section_axes(n: DVec3) -> (DVec3, DVec3) {
    let u = DVec3::Z.cross(n);
    if u.length_squared() < 1e-9 {
        return (DVec3::X, n.cross(DVec3::X).normalize());
    }
    let u = u.normalize();
    (u, n.cross(u))
}

/// Corners of the rectangle drawn for a section plane, in world coordinates: it covers
/// what its collection contains, with a margin.
pub fn section_rect(model: &Model, ents: &Entities, xform: &DMat4, sp: &SectionPlane) -> [DVec3; 4] {
    let mut pts = Vec::new();
    collection_points(model, ents, &DMat4::IDENTITY, 0, &mut pts);
    let (u, v) = section_axes(sp.normal);
    let (lo, hi) = if pts.is_empty() {
        (DVec2::splat(-500.0), DVec2::splat(500.0))
    } else {
        let q: Vec<DVec2> = pts.iter().map(|p| DVec2::new((*p - sp.pos).dot(u), (*p - sp.pos).dot(v))).collect();
        (
            q.iter().fold(DVec2::splat(f64::INFINITY), |a, b| a.min(*b)),
            q.iter().fold(DVec2::splat(f64::NEG_INFINITY), |a, b| a.max(*b)),
        )
    };
    let margin = ((hi - lo).max_element() * 0.08).max(50.0);
    let (lo, hi) = (lo - margin, hi + margin);
    [DVec2::new(lo.x, lo.y), DVec2::new(hi.x, lo.y), DVec2::new(hi.x, hi.y), DVec2::new(lo.x, hi.y)]
        .map(|c| xform.transform_point3(sp.pos + u * c.x + v * c.y))
}

/// Whether faces show their materials' textures: Shaded with Textures, and the photoreal
/// materials (which are shaded with textures).
pub fn textures_shown(st: &ViewStyle) -> bool {
    st.faces == FaceStyle::ShadedWithTextures || (st.photoreal && st.faces == FaceStyle::Shaded)
}

pub fn build(model: &Model, opt: &SceneOptions) -> SceneMesh {
    build_parts(model, opt).0
}

/// What the placed collections of a build share: the options, and the tables they fill
/// (texture layers, clip planes, what is worked out once per collection).
struct Shared<'a> {
    model: &'a Model,
    opt: &'a SceneOptions<'a>,
    sel: HashSet<EntityRef>,
    editing: bool,
    bbox_only: bool,
    edge_shadows: Option<(DVec3, f64)>,
    texs: TexTable,
    clips: Vec<SectionPlane>,
    /// Per collection, by address: the model does not change between the builds that
    /// share this table (see [`Parts`]).
    colls: HashMap<usize, Coll>,
}

/// The settings a build reads besides the model and the options.
fn bbox_only() -> bool {
    crate::settings::with(|s| s.bbox_highlight)
}

/// Shadows › From edges: the sun's direction and how wide the shadow of an edge is (a
/// thousandth of the model's size, 2 mm at least).
fn edge_shadows(model: &Model) -> Option<(DVec3, f64)> {
    (model.shadow.display && model.shadow.from_edges).then(|| {
        let (lo, hi) = model_bounds(model);
        let size = if lo.is_finite() && hi.is_finite() { (hi - lo).length() } else { 0.0 };
        (model.shadow.sun_direction().normalize_or_zero(), (size * 1e-3).max(2.0))
    })
}

/// One placed collection's part of a built mesh: where its vertices lie in each list, and
/// what they were made from.
struct Part {
    key: PartKey,
    opaque: std::ops::Range<usize>,
    transparent: std::ops::Range<usize>,
    lines: std::ops::Range<usize>,
    bounds: Option<(DVec3, DVec3)>,
}

/// What a placed collection's vertices depend on besides the model, the options and the
/// tables of the build: its place, how it is reached, and the selection it shows.
#[derive(PartialEq)]
struct PartKey {
    path: Vec<InstanceId>,
    xform: [u64; 16],
    hidden: bool,
    inherited: Option<MaterialId>,
    cuts: Vec<[u64; 6]>,
    /// Shown selected as part of a selected group or component, and in which colour.
    selected: Option<[u8; 3]>,
    /// The collection of the open context: the selection (its own entities).
    direct: Option<Vec<EntityRef>>,
    /// Shadows face sun: the copy turned to the sun.
    sun: Option<[u64; 16]>,
}

fn bits(m: &DMat4) -> [u64; 16] {
    m.to_cols_array().map(f64::to_bits)
}

fn part_key(sh: &Shared, p: &esq_core::walk::Placed) -> PartKey {
    let (model, opt) = (sh.model, sh.opt);
    let (inside, selected) = classify(&p.path, opt.ctx, &sh.sel);
    let direct = p.path.len() == opt.ctx.len() && inside;
    let selected = selected.then(|| {
        if selected_locked(model, opt.ctx, &p.path) { opt.style.locked_color } else { opt.style.highlight_color }
    });
    PartKey {
        path: p.path.clone(),
        xform: bits(&p.xform),
        hidden: p.hidden,
        inherited: p.inherited,
        cuts: p
            .cuts
            .iter()
            .map(|c| [c.pos.x, c.pos.y, c.pos.z, c.normal.x, c.normal.y, c.normal.z].map(f64::to_bits))
            .collect(),
        selected,
        // Its own selected faces, edges, guides and section planes (a selected group or
        // component shows in the part of what it places).
        direct: direct.then(|| opt.sel.iter().filter(|e| !matches!(e, EntityRef::Instance(_))).copied().collect()),
        sun: crate::dialogs::comp_edit::sun_copy(model, p.def, &p.path).map(|m| bits(&m)),
    }
}

/// A built mesh's parts and the tables of its build: what [`update_parts`] needs to rebuild
/// only the parts that changed.
pub struct Parts {
    parts: Vec<Part>,
    texs: TexTable,
    clips: Vec<SectionPlane>,
    colls: HashMap<usize, Coll>,
    /// The settings read, and the shadows of edges.
    bbox_only: bool,
    edge_shadows: Option<(DVec3, f64)>,
}

/// New vertices for some ranges of a built mesh (see [`update_parts`]).
#[derive(Default)]
pub struct Patch {
    pub opaque: Vec<(usize, Vec<FaceVertex>)>,
    pub transparent: Vec<(usize, Vec<FaceVertex>)>,
    pub lines: Vec<(usize, Vec<LineInst>)>,
    pub bounds: Option<(DVec3, DVec3)>,
}

fn fold_bounds(b: Option<(DVec3, DVec3)>, p: DVec3) -> Option<(DVec3, DVec3)> {
    Some(match b {
        None => (p, p),
        Some((lo, hi)) => (p.min(lo), p.max(hi)),
    })
}

fn bounds_of(v: &[FaceVertex]) -> Option<(DVec3, DVec3)> {
    v.iter().map(|v| DVec3::from(v.pos.map(f64::from))).fold(None, fold_bounds)
}

fn union_bounds(parts: &[Part]) -> Option<(DVec3, DVec3)> {
    parts.iter().filter_map(|p| p.bounds).fold(None, |b, (lo, hi)| fold_bounds(fold_bounds(b, lo), hi))
}

/// The mesh, and its parts for later updates.
pub fn build_parts(model: &Model, opt: &SceneOptions) -> (SceneMesh, Parts) {
    crate::perf::count(|c| c.mesh_builds += 1);
    let mut out = SceneMesh::default();
    let mut sh = Shared {
        model,
        opt,
        sel: opt.sel.iter().copied().collect(),
        editing: !opt.ctx.is_empty(),
        bbox_only: bbox_only(),
        edge_shadows: edge_shadows(model),
        texs: TexTable::default(),
        clips: Vec::new(),
        colls: HashMap::new(),
    };
    let mut parts = Vec::new();
    visit_all(model, opt.style.hidden_objects, |p| {
        let key = part_key(&sh, p);
        let start = (out.opaque.len(), out.transparent.len(), out.lines.len());
        place(&mut sh, p, &mut out);
        parts.push(Part {
            key,
            opaque: start.0..out.opaque.len(),
            transparent: start.1..out.transparent.len(),
            lines: start.2..out.lines.len(),
            bounds: bounds_of(&out.opaque[start.0..]),
        });
    });
    axes_lines(opt, &mut out);
    out.bounds = union_bounds(&parts);
    out.clips = clip_rows(&sh.clips);
    out.textures = sh.texs.layers.clone();
    let Shared { texs, clips, colls, bbox_only, edge_shadows, .. } = sh;
    (out, Parts { parts, texs, clips, colls, bbox_only, edge_shadows })
}

/// The new vertices of the parts whose inputs changed since `parts` was built (the
/// selection they show, the place of components that face the camera), when the model,
/// the options and the settings are those of that build, and every new part has the size
/// of the old one. `None`: build again.
pub fn update_parts(model: &Model, opt: &SceneOptions, parts: &mut Parts) -> Option<Patch> {
    if parts.bbox_only != bbox_only()
        || parts.edge_shadows.map(|(d, w)| (d.to_array().map(f64::to_bits), w.to_bits()))
            != edge_shadows(model).map(|(d, w)| (d.to_array().map(f64::to_bits), w.to_bits()))
    {
        return None;
    }
    let mut sh = Shared {
        model,
        opt,
        sel: opt.sel.iter().copied().collect(),
        editing: !opt.ctx.is_empty(),
        bbox_only: parts.bbox_only,
        edge_shadows: parts.edge_shadows,
        texs: std::mem::take(&mut parts.texs),
        clips: std::mem::take(&mut parts.clips),
        colls: std::mem::take(&mut parts.colls),
    };
    let (layers, planes) = (sh.texs.layers.len(), sh.clips.len());
    let mut patch = Patch::default();
    let mut i = 0;
    let mut ok = true;
    visit_all(model, opt.style.hidden_objects, |p| {
        if !ok {
            return;
        }
        let Some(part) = parts.parts.get_mut(i) else {
            ok = false;
            return;
        };
        i += 1;
        let key = part_key(&sh, p);
        if key == part.key {
            return;
        }
        if key.path != part.key.path {
            ok = false;
            return;
        }
        let mut out = SceneMesh::default();
        place(&mut sh, p, &mut out);
        if out.opaque.len() != part.opaque.len()
            || out.transparent.len() != part.transparent.len()
            || out.lines.len() != part.lines.len()
        {
            ok = false;
            return;
        }
        part.key = key;
        part.bounds = bounds_of(&out.opaque);
        for (list, range, new) in [
            (&mut patch.opaque, &part.opaque, out.opaque),
            (&mut patch.transparent, &part.transparent, out.transparent),
        ] {
            if !new.is_empty() {
                list.push((range.start, new));
            }
        }
        if !out.lines.is_empty() {
            patch.lines.push((part.lines.start, out.lines));
        }
    });
    // The tables did not grow: the parts rebuilt found what the first build had put there.
    let ok = ok && i == parts.parts.len() && sh.texs.layers.len() == layers && sh.clips.len() == planes;
    let Shared { texs, clips, colls, .. } = sh;
    (parts.texs, parts.clips, parts.colls) = (texs, clips, colls);
    if !ok {
        return None;
    }
    crate::perf::count(|c| c.mesh_updates += 1);
    patch.bounds = union_bounds(&parts.parts);
    Some(patch)
}

/// The model's axes (solid on the positive side, dotted on the negative side).
fn axes_lines(opt: &SceneOptions, out: &mut SceneMesh) {
    let st = opt.style;
    if st.axes {
        let m = opt.axes_xform;
        let o = m.transform_point3(DVec3::ZERO);
        // Preferences › Accessibility: the axis colours.
        for (i, axis) in [DVec3::X, DVec3::Y, DVec3::Z].into_iter().enumerate() {
            let color = crate::settings::axis_color(i);
            let d = m.transform_vector3(axis).normalize();
            // As in SketchUp: solid on the positive side, dotted on the negative side.
            out.lines.push(line(o, o + d * FAR, rgba(color, 1.0), 1.0, LINE_KEEP));
            let faded = rgba(mix(color, st.bg_color, 0.35), 1.0);
            out.lines.push(line(o, o - d * FAR, faded, 1.0, LINE_DASHED | LINE_KEEP));
        }
    }
}

fn clip_rows(clips: &[SectionPlane]) -> Vec<[f32; 4]> {
    clips
        .iter()
        .map(|c| [c.normal.x as f32, c.normal.y as f32, c.normal.z as f32, c.normal.dot(c.pos) as f32])
        .collect()
}

/// A placed collection's vertices, appended to `out`.
fn place(sh: &mut Shared, p: &esq_core::walk::Placed, out: &mut SceneMesh) {
    let (model, opt, editing, bbox_only, edge_shadows) = (sh.model, sh.opt, sh.editing, sh.bbox_only, sh.edge_shadows);
    let st = opt.style;
    let Shared { sel, texs, clips, colls, .. } = sh;
    let sel: &HashSet<EntityRef> = sel;
    {
        let Coll { openings, tris: coll_tris, sides } = colls
            .entry(p.ents as *const Entities as usize)
            .or_insert_with(|| Coll { tris: vec![None; p.ents.faces.len()], ..Default::default() });
        let (inside, mut inst_selected) = classify(&p.path, opt.ctx, sel);
        // Styles › Modeling › Locked: a selected locked group or component shows in that
        // colour instead of the selection's.
        let hl = if inst_selected && selected_locked(model, opt.ctx, &p.path) {
            st.locked_color
        } else {
            st.highlight_color
        };
        // Preferences › Compatibility › Bounding box only: a selected group or component shows
        // its box instead of its highlighted edges.
        if inst_selected && bbox_only {
            if p.path.len() == opt.ctx.len() + 1
                && let Some(d) = p.def
            {
                for (a, b) in crate::dialogs::pref_pages::box_edges(model, d, &p.xform) {
                    out.lines.push(line(a, b, rgba(hl, 1.0), 1.0, LINE_KEEP));
                }
            }
            inst_selected = false;
        }
        let cuts: &[SectionPlane] = if st.section_cuts { &p.cuts } else { &[] };
        let slots: Vec<Option<usize>> = cuts.iter().map(|c| clip_slot(clips, c)).collect();
        let clip = slots.iter().flatten().fold(0u32, |m, i| m | 1 << i);
        // Component editing: View › Component Edit › Hide Rest of Model / Similar Components.
        if editing && !inside && (st.hide_rest || (st.hide_similar && p.def.is_some() && p.def == opt.ctx_def)) {
            return;
        }
        let direct = p.path.len() == opt.ctx.len() && inside;
        // Outside the open group, or a hidden object shown through View › Hidden Objects.
        let fade = if p.hidden {
            0.75
        } else if editing && !inside {
            // Model Info › Components: fade similar components / the rest of the model.
            let f = crate::dialogs::mi_misc::edit_fades(st);
            if p.def.is_some() && p.def == opt.ctx_def { f.1 } else { f.0 }
        } else {
            0.0
        };
        if crate::dialogs::mi_misc::show_instance_axes(st)
            && let Some(d) = p.def
            && !p.path.is_empty()
            && !model.defs[d].is_group
        {
            crate::dialogs::mi_misc::instance_axes(model, d, &p.xform, &mut |a, b, c| {
                out.lines.push(line(a, b, rgba(c, 1.0), 1.0, LINE_KEEP))
            });
        }
        let layer_visible = |l: usize| model.layers.get(l).is_none_or(|x| x.visible);
        // Images always show their picture; other faces show their textures in the textured
        // and photoreal face styles (unless they are coloured by tag).
        let is_image = p.def.is_some_and(|d| model.is_image_def(d));
        let by_tag = st.color_by_tag && matches!(st.faces, FaceStyle::Shaded | FaceStyle::ShadedWithTextures);
        let textured = (textures_shown(st) && !by_tag) || is_image;
        // What lies on Untagged inside a tagged group shows that group's tag.
        let inherited_tag = path_tag(model, &p.path);
        let tag_of = |l: usize| if l == 0 { inherited_tag } else { l };
        // Every group/component on the way casts (receives) shadows: none switched it off.
        let (path_casts, path_receives) = {
            let mut ents = &model.root;
            let (mut cast, mut receive) = (true, true);
            for &i in &p.path {
                let id = slotmap::Key::data(&i).as_ffi();
                cast &= flag_on(ents, 'i', id, "casts");
                receive &= flag_on(ents, 'i', id, "receives");
                match ents.instances.get(i).and_then(|x| model.defs.get(x.def)) {
                    Some(d) => ents = &d.entities,
                    None => break,
                }
            }
            (cast, receive)
        };
        if st.faces != FaceStyle::Wireframe {
            // Components › Edit › Cut opening: the components glued to a face open it.
            let openings = openings.get_or_insert_with(|| crate::dialogs::comp_edit::openings(model, p.ents));
            // Shadows face sun: the faces shown cast no shadow, a copy turned to the sun does.
            let sun_copy = crate::dialogs::comp_edit::sun_copy(model, p.def, &p.path);
            if let Some(m) = sun_copy {
                for (f, face) in p.ents.faces.iter().filter(|(_, x)| layer_visible(x.layer) && !x.hidden) {
                    let n = m.transform_vector3(face.normal).normalize_or_zero();
                    for t in face_triangles(p.ents, f) {
                        for v in t {
                            let mut fv = FaceVertex::plain(m.transform_point3(v), n, [1.0; 4], [1.0; 4], clip);
                            fv.tex = SHADOW_ONLY;
                            out.opaque.push(fv);
                        }
                    }
                }
            }
            for (k, (f, face)) in p.ents.faces.iter().enumerate() {
                if !layer_visible(face.layer) || (face.hidden && !st.hidden_geometry) {
                    continue;
                }
                let selected = inst_selected || (direct && sel.contains(&EntityRef::Face(f)));
                // Faces without a material take the one painted on their group, both sides.
                let (front_mat, back_mat) = (face.front.or(p.inherited), face.back.or(p.inherited));
                let mat = front_mat.and_then(|m| model.materials.get(m));
                let back = back_mat.and_then(|m| model.materials.get(m));
                let (ft, bt) = if textured {
                    let maps = face.tex.as_deref();
                    // A face's own positioning applies to its own material only.
                    let own = |m: Option<MaterialId>, mine: Option<MaterialId>, map: Option<UvMap>| {
                        if m.is_some() && m == mine { map } else { None }
                    };
                    (
                        texs.side(
                            model,
                            front_mat,
                            own(front_mat, face.front, maps.and_then(|t| t.front)),
                            face.normal,
                        ),
                        texs.side(model, back_mat, own(back_mat, face.back, maps.and_then(|t| t.back)), face.normal),
                    )
                } else {
                    (None, None)
                };
                let (mut fc, mut bc) = match st.faces {
                    _ if is_image => ([255; 3], [255; 3]),
                    FaceStyle::HiddenLine => (st.bg_color, st.bg_color),
                    FaceStyle::Monochrome => (st.front_color, st.back_color),
                    _ if by_tag => {
                        let c = tag_color(model, tag_of(face.layer));
                        (c, c)
                    }
                    _ => {
                        (mat.map(|m| m.color).unwrap_or(st.front_color), back.map(|m| m.color).unwrap_or(st.back_color))
                    }
                };
                // Face settings › Enable transparency: off, transparent materials are opaque.
                let mut alpha = match st.faces {
                    FaceStyle::HiddenLine | FaceStyle::Monochrome if !is_image => 1.0,
                    _ if by_tag || !st.transparency => 1.0,
                    _ => mat.map(|m| m.opacity).unwrap_or(1.0),
                };
                let (mut fv, mut bv) = (Veil::NONE, Veil::NONE);

                if selected {
                    fc = mix(fc, hl, 0.45);
                    bc = mix(bc, hl, 0.45);
                    fv = fv.then(hl, 0.45);
                    bv = bv.then(hl, 0.45);
                }
                if fade > 0.0 {
                    fc = mix(fc, st.bg_color, fade);
                    bc = mix(bc, st.bg_color, fade);
                    fv = fv.then(st.bg_color, fade);
                    bv = bv.then(st.bg_color, fade);
                }
                if st.xray {
                    // « Rapide » shows X-ray at 50 %, « Meilleure » at its slider (`oit.rs`).
                    alpha = alpha.min(crate::oit::xray_opacity(st) as f32);
                }
                if face.hidden {
                    alpha = alpha.min(0.3);
                }
                let side_color = |c: [u8; 3], t: &Option<SideTex>, v: Veil| match t {
                    Some(_) => [v.target[0], v.target[1], v.target[2], alpha],
                    None => rgba(c, alpha),
                };
                let (front, back_c) = (side_color(fc, &ft, fv), side_color(bc, &bt, bv));
                let mix_bits =
                    |t: &Option<SideTex>, v: Veil| if t.is_some() { (v.t * 255.0).round() as u32 } else { 0 };
                // Entity Info › Cast Shadows (`Drawingelement#casts_shadows=`), of the face or of
                // a group/component holding it.
                let fid = slotmap::Key::data(&f).as_ffi();
                let no_cast = sun_copy.is_some() || !path_casts || !casts(p.ents, 'f', fid);
                // Entity Info › Receive Shadows (`Drawingelement#receives_shadows=`), likewise.
                let no_receive = !path_receives || !flag_on(p.ents, 'f', fid, "receives");
                let tex = ft.as_ref().map_or(0, |t| t.layer)
                    | bt.as_ref().map_or(0, |t| t.layer) << TEX_BACK_SHIFT
                    | mix_bits(&ft, fv).max(mix_bits(&bt, bv)) << TEX_MIX_SHIFT
                    | if is_image { TEX_CLAMP } else { 0 }
                    | if no_cast { NO_SHADOW } else { 0 }
                    | if no_receive { NO_RECEIVE } else { 0 };
                let see_through =
                    alpha < 0.999 || ft.as_ref().is_some_and(|t| t.alpha) || bt.as_ref().is_some_and(|t| t.alpha);
                let n = p.xform.transform_vector3(face.normal).normalize_or_zero();
                let mirrored = p.xform.determinant() < 0.0;
                // Roughness and metalness for the photoreal style ride in the clip mask's top bits.
                let pbr = mat.map(crate::style::pbr_of).unwrap_or_else(crate::style::pbr_default);
                let clip = clip | pbr << 16;
                let target = if see_through { &mut out.transparent } else { &mut out.opaque };
                let tris = coll_tris[k].get_or_insert_with(|| {
                    crate::perf::count(|c| c.tessellations += 1);
                    match openings.get(&f) {
                        Some(holes) => crate::dialogs::comp_edit::cut_triangles(p.ents, f, holes),
                        None => face_triangles(p.ents, f),
                    }
                });
                for &t in tris.iter() {
                    let w = t.map(|v| p.xform.transform_point3(v));
                    let order: [usize; 3] = if mirrored { [0, 2, 1] } else { [0, 1, 2] };
                    for i in order {
                        let uv_of = |s: &Option<SideTex>| v3(s.as_ref().map_or(DVec3::Z, |s| s.map.uvq(t[i])));
                        target.push(FaceVertex {
                            pos: v3(w[i]),
                            normal: v3(n),
                            front,
                            back: back_c,
                            clip,
                            uv_front: uv_of(&ft),
                            uv_back: uv_of(&bt),
                            tex,
                        });
                    }
                }
            }
        }
        let sides = sides.get_or_insert_with(|| edge_sides(model, p.ents));
        for (e, edge) in p.ents.edges.iter() {
            // An image shows no outline, except the selection's.
            if !layer_visible(edge.layer) || (is_image && !inst_selected) {
                continue;
            }
            // Hidden and soft edges show as dashes with View › Hidden Geometry.
            let ghost = edge.hidden || edge.soft;
            if ghost && !st.hidden_geometry {
                continue;
            }
            let (a, b) = p.ents.edge_points(e);
            let (a, b) = (p.xform.transform_point3(a), p.xform.transform_point3(b));
            // Shadows › From edges: an edge that bounds no face casts a thin shadow, a ribbon
            // turned to the sun drawn in the sun's view only.
            if let Some((sun, width)) = edge_shadows
                && !ghost
                && edge.faces.is_empty()
                && path_casts
            {
                let side = (b - a).cross(sun).normalize_or_zero() * (width * 0.5);
                if side != DVec3::ZERO {
                    for v in [a - side, a + side, b + side, a - side, b + side, b - side] {
                        let mut fv = FaceVertex::plain(v, sun, [1.0; 4], [1.0; 4], clip);
                        fv.tex = SHADOW_ONLY;
                        out.opaque.push(fv);
                    }
                }
            }
            let selected = inst_selected || (direct && sel.contains(&EntityRef::Edge(e)));
            let mut c = if selected {
                hl
            } else if ghost {
                [110, 110, 110]
            } else {
                edge_color(model, p.ents, e, st, opt.axes_xform, b - a, by_tag.then(|| tag_of(edge.layer)))
            };
            if fade > 0.0 {
                c = mix(c, st.bg_color, fade + 0.05);
            }
            let mut flags = if ghost { LINE_DASHED } else { LINE_EDGE };
            // Edge style › Dashes: the dash pattern of the edge's tag.
            if st.dashes && !ghost {
                let tag = tag_of(edge.layer);
                flags |= dash_pattern(model.layers.get(tag).map_or("", |l| l.line_style.as_str())) << DASH_SHIFT;
            }
            let mut l = line(a, b, rgba(c, 1.0), 1.0, flags);
            if !ghost
                && !is_image
                && let Some(s) = sides.get(&e)
            {
                let side = |i: usize| s.get(i).map(|t| v3(p.xform.transform_vector3(*t).normalize_or_zero()));
                l.t1 = side(0).unwrap_or_default();
                // Edges between more than two faces are never silhouettes.
                l.t2 = if s.len() > 2 { l.t1 } else { side(1).unwrap_or_default() };
                if s.len() <= 2 && st.faces != FaceStyle::Wireframe {
                    l.flags |= LINE_PROFILE;
                }
            }
            if selected {
                // Selected edges stay visible when View › Edges is off.
                l.flags |= LINE_KEEP;
            }
            l.flags |= clip << CLIP_SHIFT;
            out.lines.push(l);
            // Edge style › Endpoints: the thick ends, drawn over the edge by the shader.
            if st.endpoints && !ghost {
                for k in 1..=2 {
                    let mut e = l;
                    e.flags |= k << CAP_SHIFT;
                    out.lines.push(e);
                }
            }
            // Edge style › Jitter: two more strokes, drawn slightly off by the shader.
            if st.jitter && !ghost {
                for k in 1..=2 {
                    let mut j = l;
                    j.flags |= k << JITTER_SHIFT;
                    out.lines.push(j);
                }
            }
        }
        // Section cuts: thick lines where the active planes cross the faces, and the fill.
        for (k, cut) in cuts.iter().enumerate() {
            let plane = cut.plane();
            let others: Vec<SectionPlane> =
                cuts.iter().enumerate().filter(|(j, _)| *j != k).map(|(_, c)| c.clone()).collect();
            let mut segs = Vec::new();
            for (f, face) in p.ents.faces.iter() {
                if layer_visible(face.layer) && !face.hidden {
                    segs.extend(face_cut(p.ents, f, &p.xform, &plane));
                }
            }
            let fade_to = |c: [u8; 3]| if fade > 0.0 { mix(c, st.bg_color, fade) } else { c };
            for &(a, b) in &segs {
                if let Some((a, b)) = clip_segment(a, b, &others) {
                    let col = rgba(fade_to(st.section_cut_color), 1.0);
                    out.lines.push(line(a, b, col, st.section_cut_width as f32, LINE_KEEP));
                }
            }
            // Troubleshoot Section Fill: the open chains that keep the cut from being filled
            // show in red, thicker than the cut.
            if cut.troubleshoot && st.section_fill {
                for (a, b) in esq_core::section::open_segments(&segs) {
                    if let Some((a, b)) = clip_segment(a, b, &others) {
                        let w = st.section_cut_width as f32 + 3.0;
                        out.lines.push(line(a, b, rgba(TROUBLESHOOT_RED, 1.0), w, LINE_KEEP));
                    }
                }
            }
            if st.section_fill && st.faces != FaceStyle::Wireframe {
                let own = slots[k].map(|i| 1u32 << i).unwrap_or(0);
                let fill = rgba(fade_to(st.section_fill_color), 1.0);
                for t in fill_triangles(&plane, &chain_loops(&segs)) {
                    for v in t {
                        out.opaque.push(FaceVertex::plain(v, plane.n, fill, fill, clip & !own));
                    }
                }
            }
        }
        if st.section_planes {
            let active = p.ents.active_section;
            for (id, sp) in p.ents.sections.iter() {
                if sp.hidden || !layer_visible(sp.layer) {
                    continue;
                }
                let selected = inst_selected || (direct && sel.contains(&EntityRef::Section(id)));
                let mut c = if selected {
                    st.highlight_color
                } else if active == Some(id) {
                    st.section_active_color
                } else {
                    st.section_inactive_color
                };
                if fade > 0.0 {
                    c = mix(c, st.bg_color, fade);
                }
                let col = rgba(c, 1.0);
                let r = section_rect(model, p.ents, &p.xform, sp);
                for i in 0..4 {
                    out.lines.push(line(r[i], r[(i + 1) % 4], col, 1.0, LINE_KEEP | LINE_BACK));
                }
                // Arrows at the corners, pointing at the side that stays visible.
                let n = sp.transformed(&p.xform).normal;
                let len = r[0].distance(r[1]).max(r[1].distance(r[2])) * 0.06;
                for i in 0..4 {
                    let side = (r[(i + 1) % 4] - r[i]).normalize_or_zero();
                    let tip = r[i] + n * len;
                    out.lines.push(line(r[i], tip, col, 1.0, LINE_KEEP));
                    out.lines.push(line(tip, tip - n * len * 0.4 + side * len * 0.25, col, 1.0, LINE_KEEP));
                    out.lines.push(line(tip, tip - n * len * 0.4 - side * len * 0.25, col, 1.0, LINE_KEEP));
                }
                let tint = rgba(c, if selected { 0.15 } else { 0.07 });
                for i in [0, 1, 2, 0, 2, 3] {
                    out.transparent.push(FaceVertex::plain(r[i], n, tint, tint, 0));
                }
            }
        }
        // 3D polylines of a .skp (`CPolyline3d`): lines drawn as edges.
        for pl in p.ents.polylines.iter().filter(|x| !x.hidden && layer_visible(x.layer)) {
            let mut c = st.edge_color;
            if fade > 0.0 {
                c = mix(c, st.bg_color, fade + 0.05);
            }
            for w in pl.points.windows(2) {
                let (a, b) = (p.xform.transform_point3(w[0]), p.xform.transform_point3(w[1]));
                out.lines.push(line(a, b, rgba(c, 1.0), 1.0, LINE_EDGE));
            }
        }
        if st.guides {
            for (g, guide) in p.ents.guides.iter() {
                if guide.hidden || !layer_visible(guide.layer) || guide.is_point() {
                    continue;
                }
                let w = guide.transformed(&p.xform);
                let selected = inst_selected || (direct && sel.contains(&EntityRef::Guide(g)));
                let mut c = if selected { st.highlight_color } else { st.guide_color };
                if fade > 0.0 {
                    c = mix(c, st.bg_color, fade + 0.05);
                }
                let col = rgba(c, 1.0);
                let flags = LINE_DASHED | LINE_KEEP;
                if w.infinite {
                    // Two halves starting at the anchor, so that the dashes stay put.
                    out.lines.push(line(w.pos, w.pos + w.dir * FAR, col, 1.0, flags));
                    out.lines.push(line(w.pos, w.pos - w.dir * FAR, col, 1.0, flags));
                } else {
                    out.lines.push(line(w.pos, w.pos + w.dir, col, 1.0, flags));
                }
            }
        }
    }
}

/// World positions of the visible guide points (drawn as small crosses over the view).
pub fn guide_points(model: &Model, opt: &SceneOptions) -> Vec<(DVec3, bool)> {
    let mut out = Vec::new();
    if !opt.style.guides {
        return out;
    }
    let sel: HashSet<EntityRef> = opt.sel.iter().copied().collect();
    visit(model, |p| {
        let direct = p.path.as_slice() == opt.ctx;
        for (g, guide) in p.ents.guides.iter() {
            if guide.is_point() && !guide.hidden && model.layers.get(guide.layer).is_none_or(|x| x.visible) {
                out.push((p.xform.transform_point3(guide.pos), direct && sel.contains(&EntityRef::Guide(g))));
            }
        }
    });
    out
}

/// Bounding box of everything visible, in world coordinates.
pub fn model_bounds(model: &Model) -> (DVec3, DVec3) {
    let mut min = DVec3::splat(f64::INFINITY);
    let mut max = DVec3::splat(f64::NEG_INFINITY);
    visit(model, |p| {
        let pts = p.ents.verts.values().map(|v| v.pos);
        // Spaces (their outline) and snaps are drawn too: Zoom Extents frames them.
        let marks = p.ents.spaces.iter().flat_map(|s| s.outline().into_iter().flat_map(|(a, b)| [a, b]));
        let lines = p.ents.polylines.iter().flat_map(|l| l.points.iter().copied());
        for q in pts.chain(marks).chain(p.ents.snaps.iter().map(|s| s.pos)).chain(lines) {
            let w = p.xform.transform_point3(q);
            min = min.min(w);
            max = max.max(w);
        }
    });
    (min, max)
}

#[cfg(test)]
impl Patch {
    /// What `Viewport3d::patch` does to the GPU's lists, on a mesh.
    fn apply(&self, m: &mut SceneMesh) {
        for (at, v) in &self.opaque {
            m.opaque[*at..*at + v.len()].copy_from_slice(v);
        }
        for (at, v) in &self.transparent {
            m.transparent[*at..*at + v.len()].copy_from_slice(v);
        }
        for (at, v) in &self.lines {
            m.lines[*at..*at + v.len()].copy_from_slice(v);
        }
        m.bounds = self.bounds;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(e: &mut Entities, p: DVec3) -> esq_core::model::FaceId {
        let c = [DVec3::ZERO, DVec3::X, DVec3::new(1.0, 1.0, 0.0), DVec3::Y].map(|q| e.add_vertex_raw(p + q * 10.0));
        for k in 0..4 {
            e.add_edge_raw(c[k], c[(k + 1) % 4]);
        }
        e.add_face_raw(c.to_vec(), Vec::new()).unwrap()
    }

    /// A component of `faces` squares (some see-through) placed `copies` times, and a few
    /// squares in the root.
    fn component_model(faces: usize, copies: usize) -> Model {
        let mut m = Model::new();
        let glass = m.add_material("Verre", [90, 160, 220]);
        m.materials[glass].opacity = 0.5;
        let mut e = Entities::default();
        for i in 0..faces {
            let f = square(&mut e, DVec3::new(i as f64 * 20.0, 0.0, (i % 3) as f64 * 5.0));
            if i % 4 == 1 {
                e.faces[f].front = Some(glass);
            }
        }
        for i in 0..5 {
            square(&mut m.root, DVec3::new(-100.0 - i as f64 * 20.0, 0.0, 0.0));
        }
        let d = m.defs.insert(esq_core::model::Definition {
            name: "Carrés".into(),
            is_group: false,
            description: String::new(),
            entities: e,
            attributes: Default::default(),
            ext: Default::default(),
        });
        for i in 0..copies {
            m.root.instances.insert(esq_core::model::Instance {
                def: d,
                xform: DMat4::from_translation(DVec3::new(0.0, i as f64 * 30.0, 0.0)),
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

    fn same_mesh(a: &SceneMesh, b: &SceneMesh) -> bool {
        let bytes = |v: &[FaceVertex]| bytemuck::cast_slice::<FaceVertex, u8>(v).to_vec();
        bytes(&a.opaque) == bytes(&b.opaque)
            && bytes(&a.transparent) == bytes(&b.transparent)
            && bytemuck::cast_slice::<LineInst, u8>(&a.lines) == bytemuck::cast_slice::<LineInst, u8>(&b.lines)
            && a.bounds == b.bounds
            && a.clips == b.clips
    }

    /// Selecting a copy, a face of the root, then the whole root: only the parts that show
    /// the change are made again, and the mesh is then the one a whole build makes.
    #[test]
    fn a_selection_change_rebuilds_only_the_parts_it_shows_on() {
        let m = component_model(40, 30);
        let style = ViewStyle::default();
        fn options<'a>(sel: &'a [EntityRef], style: &'a ViewStyle) -> SceneOptions<'a> {
            SceneOptions { ctx: &[], ctx_def: None, sel, axes_xform: DMat4::IDENTITY, style }
        }
        let (mut mesh, mut parts) = build_parts(&m, &options(&[], &style));
        let inst = m.root.instances.keys().nth(5).unwrap();
        let face = m.root.faces.keys().next().unwrap();
        let mut all: Vec<EntityRef> = m.root.faces.keys().map(EntityRef::Face).collect();
        all.extend(m.root.instances.keys().map(EntityRef::Instance));
        all.sort();
        for (sel, rebuilt) in
            [(vec![EntityRef::Instance(inst)], 1), (vec![EntityRef::Face(face)], 2), (all, 31), (Vec::new(), 31)]
        {
            let (t0, b0) = (crate::perf::get().tessellations, crate::perf::get().mesh_builds);
            let patch = update_parts(&m, &options(&sel, &style), &mut parts).expect("mise à jour");
            assert_eq!(crate::perf::get().tessellations - t0, 0, "rien à trianguler à nouveau");
            assert_eq!(crate::perf::get().mesh_builds - b0, 0);
            let touched = patch.opaque.len().max(patch.lines.len());
            assert!(touched <= rebuilt, "{touched} parties refaites pour {sel:?}");
            patch.apply(&mut mesh);
            assert!(same_mesh(&mesh, &build(&m, &options(&sel, &style))), "{sel:?}");
        }
    }

    /// Components that always face the camera: when the eye moves, their parts alone are
    /// made again, at their new place.
    #[test]
    fn components_facing_the_camera_move_alone() {
        let mut m = component_model(10, 8);
        let d = m.defs.keys().next().unwrap();
        m.defs[d].ext.behavior.always_face_camera = true;
        let style = ViewStyle::default();
        let opts = SceneOptions { ctx: &[], ctx_def: None, sel: &[], axes_xform: DMat4::IDENTITY, style: &style };
        esq_core::walk::set_eye(Some(DVec3::new(500.0, -800.0, 300.0)));
        let (mut mesh, mut parts) = build_parts(&m, &opts);
        for k in 0..5 {
            esq_core::walk::set_eye(Some(DVec3::new(500.0 + k as f64 * 300.0, -800.0 + k as f64 * 90.0, 300.0)));
            let patch = update_parts(&m, &opts, &mut parts).expect("mise à jour");
            assert!(patch.opaque.len() <= 8, "{} parties", patch.opaque.len());
            patch.apply(&mut mesh);
            assert!(same_mesh(&mesh, &build(&m, &opts)), "œil {k}");
        }
        esq_core::walk::set_eye(None);
    }

    /// A component of 50 squares placed 200 times: its faces are triangulated once for the
    /// view and once for picking, not once per copy.
    #[test]
    fn a_component_is_triangulated_once_however_many_copies() {
        let mut m = Model::new();
        let mut e = Entities::default();
        for i in 0..50 {
            square(&mut e, DVec3::new(i as f64 * 20.0, 0.0, 0.0));
        }
        let d = m.defs.insert(esq_core::model::Definition {
            name: "Carrés".into(),
            is_group: false,
            description: String::new(),
            entities: e,
            attributes: Default::default(),
            ext: Default::default(),
        });
        for i in 0..200 {
            m.root.instances.insert(esq_core::model::Instance {
                def: d,
                xform: DMat4::from_translation(DVec3::new(0.0, i as f64 * 30.0, 0.0)),
                name: String::new(),
                material: None,
                layer: 0,
                hidden: false,
                locked: false,
                attributes: Default::default(),
                ext: Default::default(),
            });
        }
        let style = ViewStyle::default();
        let opts = SceneOptions { ctx: &[], ctx_def: None, sel: &[], axes_xform: DMat4::IDENTITY, style: &style };
        let before = crate::perf::get().tessellations;
        let mesh = build(&m, &opts);
        assert_eq!(crate::perf::get().tessellations - before, 50);
        assert_eq!(mesh.opaque.len(), 200 * 50 * 2 * 3);
        let before = crate::perf::get().tessellations;
        let _ = crate::pick::PickCache::build(&m, &style);
        assert_eq!(crate::perf::get().tessellations - before, 50);
    }

    /// A face with many holes (`esq_core::tess::BIG` points or more) is triangulated once for
    /// the view and the picking cache, and not again when they are rebuilt after a change
    /// elsewhere in the model: same triangles, found again.
    #[test]
    fn a_face_with_many_holes_is_triangulated_once_for_every_rebuild() {
        let mut m = Model::new();
        let e = &mut m.root;
        let outer =
            [DVec3::ZERO, DVec3::new(2410.0, 0.0, 0.0), DVec3::new(2410.0, 610.0, 0.0), DVec3::new(0.0, 610.0, 0.0)]
                .map(|p| e.add_vertex_raw(p + DVec3::new(0.0, 0.0, 7.0)))
                .to_vec();
        let holes = (0..80)
            .map(|i| {
                let o = DVec3::new(10.0 + (i % 40) as f64 * 60.0, 10.0 + (i / 40) as f64 * 300.0, 7.0);
                [DVec3::ZERO, DVec3::new(0.0, 250.0, 0.0), DVec3::new(40.0, 250.0, 0.0), DVec3::new(40.0, 0.0, 0.0)]
                    .map(|p| e.add_vertex_raw(o + p))
                    .to_vec()
            })
            .collect();
        e.add_face_raw(outer, holes).expect("face trouée");
        let elsewhere = square(e, DVec3::new(5000.0, 0.0, 0.0));
        let style = ViewStyle::default();
        let opts = SceneOptions { ctx: &[], ctx_def: None, sel: &[], axes_xform: DMat4::IDENTITY, style: &style };
        let before = esq_core::tess::big_faces_triangulated();
        let first = build(&m, &opts);
        let _ = crate::pick::PickCache::build(&m, &style);
        assert!(esq_core::tess::big_faces_triangulated() - before <= 1);
        let made = esq_core::tess::big_faces_triangulated();
        // Another face moves: the view and the picking cache are made again.
        let v = m.root.faces[elsewhere].outer[0];
        m.root.verts[v].pos.z += 1.0;
        let again = build(&m, &opts);
        let _ = crate::pick::PickCache::build(&m, &style);
        assert_eq!(esq_core::tess::big_faces_triangulated(), made);
        assert_eq!(first.opaque.len(), again.opaque.len());
    }
}
