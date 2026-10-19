//! COLLADA 1.4.1 (.dae): SketchUp's DAE exporter (inches, Z up, a "SketchUp" root node,
//! components in `library_nodes`, one `<triangles>` per material, edges as `<lines>`) and
//! a COLLADA importer (triangles, polylists, polygons with holes, strips and fans, lines,
//! node hierarchy and instancing, Lambert/Phong/Blinn colours and textures).

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use esq_core::glam::{DMat4, DVec2, DVec3, DVec4};
use esq_core::model::{DefId, MaterialId, Model};

use crate::build::{BuildOpts, DefIn, LineIn, MatIn, MeshIn, NodeIn, PolyIn, SceneIn};
use crate::obj::{BACK_COLOR, FRONT_COLOR};
use crate::options::{DaeExport, DaeImport};
use crate::scene::{Cache, Collection, Opts, WorldFace, placements};
use crate::tex::TexNames;
use crate::xml::{Xml, num, nums};

const IN: f64 = 25.4;

// ------------------------------------------------------------------------------------------
// Writing

/// A file the export writes next to the document (textures).
pub struct Extra {
    pub path: String,
    pub data: Vec<u8>,
}

struct MatSlot {
    mat: Option<MaterialId>,
    front: bool,
    id: String,
    effect: String,
    image: Option<(String, String)>,
    name: String,
}

struct Writer<'a> {
    model: &'a Model,
    o: &'a DaeExport,
    next: u32,
    mats: Vec<MatSlot>,
    tex_dir: String,
    texnames: TexNames,
    extras: Vec<Extra>,
    geometries: Xml,
    lib_nodes: Xml,
    node_ids: HashMap<(Option<DefId>, Option<MaterialId>), String>,
    edges_material: Option<String>,
}

impl Writer<'_> {
    fn id(&mut self) -> String {
        self.next += 1;
        format!("ID{}", self.next)
    }

    /// The COLLADA material for a SketchUp material seen from one side.
    fn material(&mut self, mat: Option<MaterialId>, front: bool) -> String {
        let front = mat.is_some() || front;
        if let Some(m) = self.mats.iter().find(|m| m.mat == mat && m.front == front) {
            return m.id.clone();
        }
        let id = self.id();
        let effect = self.id();
        let m = mat.and_then(|m| self.model.materials.get(m));
        let name = match m {
            Some(m) => clean(&m.name),
            None if front => "material".into(),
            None => "BackColor".into(),
        };
        let image = match m.and_then(|m| m.ext.texture.as_ref()) {
            Some(t) if self.o.texture_maps => {
                let file = self.texnames.name_for(t);
                let rel = format!("{}/{}", self.tex_dir, file);
                self.extras.push(Extra { path: rel.clone(), data: t.image.0.to_vec() });
                let img = self.id();
                Some((img, rel))
            }
            _ => None,
        };
        self.mats.push(MatSlot { mat, front, id: id.clone(), effect, image, name });
        id
    }

    fn edge_material(&mut self) -> String {
        if let Some(m) = &self.edges_material {
            return m.clone();
        }
        let id = self.id();
        let effect = self.id();
        self.mats.push(MatSlot {
            mat: None,
            front: true,
            id: id.clone(),
            effect,
            image: None,
            name: "edge_color000255".into(),
        });
        self.edges_material = Some(id.clone());
        id
    }

    /// Writes the geometry of `faces` (in the coordinates of `xform`) and returns the
    /// `<instance_geometry>` to put in a node: (geometry id, symbol → material id).
    fn geometry(
        &mut self,
        coll: &Collection,
        xform: DMat4,
        inherited: Option<MaterialId>,
    ) -> Option<(String, Vec<(String, String)>)> {
        if coll.faces.is_empty() {
            return None;
        }
        let mut pos: Vec<DVec3> = Vec::new();
        let mut nor: Vec<DVec3> = Vec::new();
        let mut uv: Vec<DVec2> = Vec::new();
        let mut index: HashMap<[u64; 7], usize> = HashMap::new();
        // material id → (textured, polygons as vertex lists)
        let mut prims: Vec<(String, bool, Vec<Vec<usize>>)> = Vec::new();
        let sides: &[bool] = if self.o.two_sided { &[true, false] } else { &[true] };
        // Every front first, then the backs (importers keep the first of two coincident faces).
        for &front in sides {
            for face in &coll.faces {
                let wf = WorldFace { face, xform, front: face.front.or(inherited), back: face.back.or(inherited) };
                let pts = wf.points();
                let mat = if front { wf.front } else { wf.back };
                let mid = self.material(mat, front);
                let uvs = if self.o.texture_maps { face.uvs(self.model, mat, front) } else { None };
                let n = if front { wf.normal() } else { -wf.normal() };
                let mut vid = |i: usize| {
                    let p = pts[i] / IN;
                    let t = uvs.as_ref().map_or(DVec2::ZERO, |u| u[i]);
                    let key = [p.x, p.y, p.z, n.x, n.y, n.z, t.x * 1e3 + t.y].map(f64::to_bits);
                    *index.entry(key).or_insert_with(|| {
                        pos.push(p);
                        nor.push(n);
                        uv.push(t);
                        pos.len() - 1
                    })
                };
                let polys: Vec<Vec<usize>> = if !self.o.triangulate && face.is_simple() {
                    let mut l = wf.loops().remove(0);
                    if !front {
                        l.reverse();
                    }
                    vec![l.into_iter().map(&mut vid).collect()]
                } else {
                    wf.tris()
                        .into_iter()
                        .map(|t| {
                            let t = if front { t } else { [t[0], t[2], t[1]] };
                            t.iter().map(|&i| vid(i)).collect()
                        })
                        .collect()
                };
                match prims.iter_mut().find(|p| p.0 == mid) {
                    Some(p) => p.2.extend(polys),
                    None => prims.push((mid, uvs.is_some(), polys)),
                }
            }
        }
        let gid = self.id();
        let (sp, sn, st, sv) = (self.id(), self.id(), self.id(), self.id());
        let any_uv = prims.iter().any(|p| p.1);
        let g = &mut self.geometries;
        g.open("geometry", &[("id", &gid)]);
        g.open("mesh", &[]);
        source(g, &sp, &pos.iter().flat_map(|p| [p.x, p.y, p.z]).collect::<Vec<_>>(), &["X", "Y", "Z"], &mut self.next);
        source(g, &sn, &nor.iter().flat_map(|p| [p.x, p.y, p.z]).collect::<Vec<_>>(), &["X", "Y", "Z"], &mut self.next);
        if any_uv {
            source(g, &st, &uv.iter().flat_map(|p| [p.x, p.y]).collect::<Vec<_>>(), &["S", "T"], &mut self.next);
        }
        g.open("vertices", &[("id", &sv)]);
        g.empty("input", &[("semantic", "POSITION"), ("source", &format!("#{sp}"))]);
        g.empty("input", &[("semantic", "NORMAL"), ("source", &format!("#{sn}"))]);
        g.close();
        let mut binds = Vec::new();
        for (k, (mid, textured, polys)) in prims.iter().enumerate() {
            let symbol = format!("Material{}", k + 2);
            binds.push((symbol.clone(), mid.clone()));
            let tris = polys.iter().all(|p| p.len() == 3);
            let count = polys.len().to_string();
            g.open(if tris { "triangles" } else { "polylist" }, &[("count", &count), ("material", &symbol)]);
            g.empty("input", &[("offset", "0"), ("semantic", "VERTEX"), ("source", &format!("#{sv}"))]);
            if *textured {
                g.empty(
                    "input",
                    &[("offset", "1"), ("semantic", "TEXCOORD"), ("source", &format!("#{st}")), ("set", "0")],
                );
            }
            if !tris {
                let vc: Vec<String> = polys.iter().map(|p| p.len().to_string()).collect();
                g.text("vcount", &[], &vc.join(" "));
            }
            let idx: Vec<String> = polys
                .iter()
                .flatten()
                .flat_map(|i| if *textured { vec![i.to_string(), i.to_string()] } else { vec![i.to_string()] })
                .collect();
            g.text("p", &[], &idx.join(" "));
            g.close();
        }
        g.close();
        g.close();
        Some((gid, binds))
    }

    fn edges_geometry(&mut self, coll: &Collection, xform: DMat4) -> Option<(String, Vec<(String, String)>)> {
        if !self.o.edges || coll.edges.is_empty() {
            return None;
        }
        let mid = self.edge_material();
        let mut pos: Vec<DVec3> = Vec::new();
        let mut index: HashMap<[u64; 3], usize> = HashMap::new();
        let mut lines = Vec::new();
        for e in &coll.edges {
            for p in [e.a, e.b] {
                let p = xform.transform_point3(p) / IN;
                let i = *index.entry([p.x, p.y, p.z].map(f64::to_bits)).or_insert_with(|| {
                    pos.push(p);
                    pos.len() - 1
                });
                lines.push(i);
            }
        }
        let gid = self.id();
        let (sp, sv) = (self.id(), self.id());
        let g = &mut self.geometries;
        g.open("geometry", &[("id", &gid)]);
        g.open("mesh", &[]);
        source(g, &sp, &pos.iter().flat_map(|p| [p.x, p.y, p.z]).collect::<Vec<_>>(), &["X", "Y", "Z"], &mut self.next);
        g.open("vertices", &[("id", &sv)]);
        g.empty("input", &[("semantic", "POSITION"), ("source", &format!("#{sp}"))]);
        g.close();
        g.open("lines", &[("count", &(lines.len() / 2).to_string()), ("material", "Material2")]);
        g.empty("input", &[("offset", "0"), ("semantic", "VERTEX"), ("source", &format!("#{sv}"))]);
        g.text("p", &[], &lines.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(" "));
        g.close();
        g.close();
        g.close();
        Some((gid, vec![("Material2".into(), mid)]))
    }
}

fn source(g: &mut Xml, id: &str, data: &[f64], params: &[&str], next: &mut u32) {
    *next += 1;
    let arr = format!("ID{next}");
    g.open("source", &[("id", id)]);
    g.text("float_array", &[("id", &arr), ("count", &data.len().to_string())], &nums(data.iter().copied()));
    g.open("technique_common", &[]);
    g.open(
        "accessor",
        &[
            ("count", &(data.len() / params.len()).to_string()),
            ("source", &format!("#{arr}")),
            ("stride", &params.len().to_string()),
        ],
    );
    for p in params {
        g.empty("param", &[("name", p), ("type", "float")]);
    }
    g.close();
    g.close();
    g.close();
}

fn instance_geometry(x: &mut Xml, gid: &str, binds: &[(String, String)], textured: &dyn Fn(&str) -> bool) {
    x.open("instance_geometry", &[("url", &format!("#{gid}"))]);
    x.open("bind_material", &[]);
    x.open("technique_common", &[]);
    for (symbol, target) in binds {
        let attrs = [("symbol", symbol.as_str()), ("target", &format!("#{target}"))];
        if textured(target) {
            x.open("instance_material", &attrs);
            x.empty("bind_vertex_input", &[("semantic", "UVSET0"), ("input_semantic", "TEXCOORD"), ("input_set", "0")]);
            x.close();
        } else {
            x.empty("instance_material", &attrs);
        }
    }
    x.close();
    x.close();
    x.close();
}

fn matrix_text(m: &DMat4) -> String {
    // Row-major, translation in inches.
    let mut m = *m;
    m.w_axis.x /= IN;
    m.w_axis.y /= IN;
    m.w_axis.z /= IN;
    let t = m.transpose().to_cols_array();
    nums(t)
}

/// Names that other programs accept (SketchUp asks before replacing unusual characters).
pub fn clean(s: &str) -> String {
    let c: String = s.chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
    if c.is_empty() { "_".into() } else { c }
}

/// The COLLADA document of a model; textures go in `tex_dir` (relative to the document).
pub fn write(model: &Model, o: &DaeExport, cx: &crate::ExportCx, tex_dir: &str) -> (String, Vec<Extra>) {
    let mut cache = Cache::new(model, Opts { hidden: o.hidden });
    let mut w = Writer {
        model,
        o,
        next: 0,
        mats: Vec::new(),
        tex_dir: tex_dir.to_string(),
        texnames: TexNames::new(),
        extras: Vec::new(),
        geometries: Xml::new("", "    "),
        lib_nodes: Xml::new("", "    "),
        node_ids: HashMap::new(),
        edges_material: None,
    };
    let scene_id = w.id();
    let mut root = Xml::new("", "    ");
    root.open("node", &[("name", "SketchUp")]);
    let mut instances = 0usize;
    if o.preserve_hierarchies {
        write_node_body(&mut w, &mut cache, None, None, &mut root, &mut instances);
    } else {
        // Everything in world coordinates, in one geometry per collection.
        let placed = placements(&mut cache);
        for p in placed {
            let coll = cache.get(p.def).clone();
            if let Some((g, b)) = w.geometry(&coll, p.xform, p.inherited) {
                let tex: Vec<String> = textured_ids(&w);
                instance_geometry(&mut root, &g, &b, &|t| tex.iter().any(|x| x == t));
            }
            if let Some((g, b)) = w.edges_geometry(&coll, p.xform) {
                instance_geometry(&mut root, &g, &b, &|_| false);
            }
        }
    }
    let mut cameras = Xml::new("", "    ");
    if let Some(v) = &cx.view {
        let cam = w.id();
        cameras.open("camera", &[("id", &cam), ("name", "skp_camera_Last_Saved_SketchUp_View")]);
        cameras.open("optics", &[]);
        cameras.open("technique_common", &[]);
        if v.perspective {
            cameras.open("perspective", &[]);
            cameras.text("yfov", &[], &num(v.fov_deg));
        } else {
            cameras.open("orthographic", &[]);
            cameras.text("ymag", &[], &num(v.eye.distance(v.target) / IN));
        }
        cameras.text("aspect_ratio", &[], "0");
        cameras.text("znear", &[], "1");
        cameras.text("zfar", &[], "1000");
        cameras.close();
        cameras.close();
        cameras.close();
        cameras.close();
        // Camera node: looking down its -Z, Y up.
        let f = (v.target - v.eye).normalize_or(DVec3::Y);
        let r = f.cross(v.up).normalize_or(DVec3::X);
        let u = r.cross(f);
        let m = DMat4::from_cols(
            r.extend(0.0),
            u.extend(0.0),
            (-f).extend(0.0),
            DVec4::new(v.eye.x, v.eye.y, v.eye.z, 1.0),
        );
        root.open("node", &[("name", "skp_camera_Last_Saved_SketchUp_View")]);
        root.text("matrix", &[], &matrix_text(&m));
        root.empty("instance_camera", &[("url", &format!("#{cam}"))]);
        root.close();
    }
    root.close();
    // Assemble the document.
    let mut x = Xml::new("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>", "    ");
    x.open("COLLADA", &[("xmlns", "http://www.collada.org/2005/11/COLLADASchema"), ("version", "1.4.1")]);
    x.open("asset", &[]);
    x.open("contributor", &[]);
    x.text("authoring_tool", &[], "SketchUp 26.1");
    x.close();
    let now = iso_now();
    x.text("created", &[], &now);
    x.text("modified", &[], &now);
    x.empty("unit", &[("meter", "0.0254"), ("name", "inch")]);
    x.text("up_axis", &[], "Z_UP");
    x.close();
    let indent = |s: &str, depth: usize| -> String {
        s.lines().filter(|l| !l.is_empty()).map(|l| format!("{}{l}\n", "    ".repeat(depth))).collect()
    };
    if cx.view.is_some() {
        x.open("library_cameras", &[]);
        x.out.push_str(&indent(&cameras.finish(), 2));
        x.close();
    }
    x.open("library_visual_scenes", &[]);
    x.open("visual_scene", &[("id", &scene_id)]);
    x.out.push_str(&indent(&root.finish(), 3));
    x.close();
    x.close();
    let lib_nodes = std::mem::replace(&mut w.lib_nodes, Xml::new("", "    ")).finish();
    if !lib_nodes.trim().is_empty() {
        x.open("library_nodes", &[]);
        x.out.push_str(&indent(&lib_nodes, 2));
        x.close();
    }
    let geoms = std::mem::replace(&mut w.geometries, Xml::new("", "    ")).finish();
    x.open("library_geometries", &[]);
    x.out.push_str(&indent(&geoms, 2));
    x.close();
    x.open("library_materials", &[]);
    for m in &w.mats {
        x.open("material", &[("id", &m.id), ("name", &m.name)]);
        x.empty("instance_effect", &[("url", &format!("#{}", m.effect))]);
        x.close();
    }
    x.close();
    x.open("library_effects", &[]);
    let mut next = w.next;
    for m in &w.mats {
        let mat = m.mat.and_then(|id| model.materials.get(id));
        let (c, opacity) = match mat {
            Some(x) => (x.color, x.opacity as f64),
            None if m.name.starts_with("edge_color") => ([0, 0, 0], 1.0),
            None if m.front => (FRONT_COLOR, 1.0),
            None => (BACK_COLOR, 1.0),
        };
        x.open("effect", &[("id", &m.effect)]);
        x.open("profile_COMMON", &[]);
        let sampler = m.image.as_ref().map(|(img, _)| {
            next += 2;
            let (surf, samp) = (format!("ID{}", next - 1), format!("ID{next}"));
            x.open("newparam", &[("sid", &surf)]);
            x.open("surface", &[("type", "2D")]);
            x.text("init_from", &[], img);
            x.close();
            x.close();
            x.open("newparam", &[("sid", &samp)]);
            x.open("sampler2D", &[]);
            x.text("source", &[], &surf);
            x.close();
            x.close();
            samp
        });
        x.open("technique", &[("sid", "COMMON")]);
        x.open(if m.name.starts_with("edge_color") { "constant" } else { "lambert" }, &[]);
        if !m.name.starts_with("edge_color") {
            x.open("diffuse", &[]);
            match &sampler {
                Some(s) => x.empty("texture", &[("texture", s), ("texcoord", "UVSET0")]),
                None => {
                    x.text("color", &[], &nums([c[0] as f64 / 255.0, c[1] as f64 / 255.0, c[2] as f64 / 255.0, 1.0]))
                }
            }
            x.close();
        } else {
            x.open("emission", &[]);
            x.text("color", &[], "0 0 0 1");
            x.close();
        }
        if opacity < 1.0 {
            x.open("transparent", &[("opaque", "A_ONE")]);
            x.text("color", &[], "1 1 1 1");
            x.close();
            x.open("transparency", &[]);
            x.text("float", &[], &num(opacity));
            x.close();
        }
        x.close();
        x.close();
        x.close();
        x.close();
    }
    x.close();
    let images: Vec<&(String, String)> = w.mats.iter().filter_map(|m| m.image.as_ref()).collect();
    if !images.is_empty() {
        x.open("library_images", &[]);
        for (id, rel) in images {
            x.open("image", &[("id", id)]);
            x.text("init_from", &[], rel);
            x.close();
        }
        x.close();
    }
    x.open("scene", &[]);
    x.empty("instance_visual_scene", &[("url", &format!("#{scene_id}"))]);
    x.close();
    let doc = x.finish();
    (doc, std::mem::take(&mut w.extras))
}

fn textured_ids(w: &Writer) -> Vec<String> {
    w.mats.iter().filter(|m| m.image.is_some()).map(|m| m.id.clone()).collect()
}

/// The content of a node for a collection: its geometry and its instances.
fn write_node_body(
    w: &mut Writer,
    cache: &mut Cache,
    def: Option<DefId>,
    inherited: Option<MaterialId>,
    x: &mut Xml,
    instances: &mut usize,
) {
    let coll = cache.get(def).clone();
    if let Some((g, b)) = w.geometry(&coll, DMat4::IDENTITY, inherited) {
        let tex = textured_ids(w);
        instance_geometry(x, &g, &b, &|t| tex.iter().any(|y| y == t));
    }
    if let Some((g, b)) = w.edges_geometry(&coll, DMat4::IDENTITY) {
        instance_geometry(x, &g, &b, &|_| false);
    }
    for inst in &coll.instances {
        let inh = inst.material.or(inherited);
        let target = library_node(w, cache, inst.def, inh, instances);
        let id = w.id();
        let name = if inst.name.is_empty() { format!("instance_{}", *instances) } else { clean(&inst.name) };
        *instances += 1;
        x.open("node", &[("id", &id), ("name", &name)]);
        x.text("matrix", &[], &matrix_text(&inst.xform));
        x.empty("instance_node", &[("url", &format!("#{target}"))]);
        x.close();
    }
}

fn library_node(
    w: &mut Writer,
    cache: &mut Cache,
    def: DefId,
    inherited: Option<MaterialId>,
    instances: &mut usize,
) -> String {
    if let Some(id) = w.node_ids.get(&(Some(def), inherited)) {
        return id.clone();
    }
    let id = w.id();
    w.node_ids.insert((Some(def), inherited), id.clone());
    let name = clean(&w.model.defs[def].name);
    let mut x = Xml::new("", "    ");
    x.open("node", &[("id", &id), ("name", &name)]);
    write_node_body(w, cache, Some(def), inherited, &mut x, instances);
    x.close();
    let body = x.finish();
    w.lib_nodes.out.push_str(&body);
    id
}

pub(crate) fn iso_now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil date from days since 1970 (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem / 60 % 60, rem % 60)
}

/// File › Export › 3D Model › COLLADA: the .dae and its textures in a folder named after it.
pub fn export(model: &Model, path: &Path, o: &DaeExport, cx: &crate::ExportCx) -> Result<()> {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "model".into());
    let (doc, extras) = write(model, o, cx, &stem);
    std::fs::write(path, doc)?;
    let dir = path.parent().unwrap_or(Path::new("."));
    for e in extras {
        let p = dir.join(&e.path);
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(p, e.data)?;
    }
    Ok(())
}

// ------------------------------------------------------------------------------------------
// Reading

/// Where the importer finds the images a document names (next to the file, or in a KMZ).
pub trait ImageSource {
    fn image(&self, reference: &str) -> Option<(String, Vec<u8>)>;
}

pub struct Beside<'a>(pub Option<&'a Path>);

impl ImageSource for Beside<'_> {
    fn image(&self, reference: &str) -> Option<(String, Vec<u8>)> {
        crate::tex::find_beside(self.0?, reference)
    }
}

fn floats(s: &str) -> Vec<f64> {
    s.split_ascii_whitespace().filter_map(|x| x.parse().ok()).collect()
}

fn ints(s: &str) -> Vec<i64> {
    s.split_ascii_whitespace().filter_map(|x| x.parse().ok()).collect()
}

struct Doc<'a, 'i> {
    ids: HashMap<&'a str, roxmltree::Node<'a, 'i>>,
    problems: Vec<String>,
}

fn kids<'a, 'i>(n: roxmltree::Node<'a, 'i>, tag: &'a str) -> impl Iterator<Item = roxmltree::Node<'a, 'i>> + 'a {
    n.children().filter(move |c| c.is_element() && c.tag_name().name() == tag)
}

fn kid<'a, 'i>(n: roxmltree::Node<'a, 'i>, tag: &'a str) -> Option<roxmltree::Node<'a, 'i>> {
    kids(n, tag).next()
}

impl<'a, 'i> Doc<'a, 'i> {
    fn get(&mut self, url: &str) -> Option<roxmltree::Node<'a, 'i>> {
        let id = url.trim_start_matches('#');
        let r = self.ids.get(id).copied();
        if r.is_none() {
            self.problems.push(format!("référence introuvable : {url}"));
        }
        r
    }

    /// Floats of a `<source>`, with the accessor's stride.
    fn source(&mut self, url: &str) -> Option<(Vec<f64>, usize)> {
        let s = self.get(url)?;
        let arr = kid(s, "float_array")?;
        let data = floats(arr.text().unwrap_or(""));
        let acc = kid(s, "technique_common").and_then(|t| kid(t, "accessor"));
        let stride = acc.and_then(|a| a.attribute("stride")).and_then(|x| x.parse().ok()).unwrap_or(3usize).max(1);
        if let Some(n) = arr.attribute("count").and_then(|x| x.parse::<usize>().ok())
            && n != data.len()
        {
            self.problems.push(format!("tableau {} : {} valeurs pour {n} annoncées", url, data.len()));
        }
        Some((data, stride))
    }
}

fn node_matrix(n: roxmltree::Node) -> DMat4 {
    let mut m = DMat4::IDENTITY;
    for c in n.children().filter(|c| c.is_element()) {
        let v = floats(c.text().unwrap_or(""));
        let t = match c.tag_name().name() {
            "matrix" if v.len() == 16 => DMat4::from_cols_array(&v.try_into().unwrap()).transpose(),
            "translate" if v.len() == 3 => DMat4::from_translation(DVec3::new(v[0], v[1], v[2])),
            "scale" if v.len() == 3 => DMat4::from_scale(DVec3::new(v[0], v[1], v[2])),
            "rotate" if v.len() == 4 => {
                let axis = DVec3::new(v[0], v[1], v[2]);
                if axis.length() < 1e-12 {
                    continue;
                }
                DMat4::from_axis_angle(axis.normalize(), v[3].to_radians())
            }
            "lookat" if v.len() == 9 => {
                let (eye, at, up) =
                    (DVec3::new(v[0], v[1], v[2]), DVec3::new(v[3], v[4], v[5]), DVec3::new(v[6], v[7], v[8]));
                {
                    let f = (at - eye).normalize_or(DVec3::NEG_Z);
                    let r = f.cross(up).normalize_or(DVec3::X);
                    let u = r.cross(f);
                    DMat4::from_cols(r.extend(0.0), u.extend(0.0), (-f).extend(0.0), eye.extend(1.0))
                }
            }
            _ => continue,
        };
        m *= t;
    }
    m
}

struct Importer<'a, 'i, 's> {
    doc: Doc<'a, 'i>,
    scene: SceneIn,
    mats: HashMap<String, u32>,
    images: &'s dyn ImageSource,
    /// (geometry id, binding) → mesh index.
    meshes: HashMap<(String, Vec<(String, u32)>), usize>,
    nodes: HashMap<String, usize>,
    depth: usize,
}

impl Importer<'_, '_, '_> {
    fn material(&mut self, url: &str) -> Option<u32> {
        if let Some(&m) = self.mats.get(url) {
            return Some(m);
        }
        let mat = self.doc.get(url)?;
        let name = mat.attribute("name").or(mat.attribute("id")).unwrap_or("Matière").to_string();
        let effect = kid(mat, "instance_effect").and_then(|e| e.attribute("url")).and_then(|u| self.doc.get(u));
        let mut color = [255u8, 255, 255];
        let mut opacity = 1.0f32;
        let mut texture = None;
        if let Some(effect) = effect {
            let profile = kid(effect, "profile_COMMON");
            let tech = profile.and_then(|p| kid(p, "technique"));
            let shader = tech.and_then(|t| {
                t.children().find(|c| matches!(c.tag_name().name(), "lambert" | "phong" | "blinn" | "constant"))
            });
            if let Some(sh) = shader {
                let diffuse = kid(sh, "diffuse").or_else(|| kid(sh, "emission"));
                if let Some(d) = diffuse {
                    if let Some(c) = kid(d, "color") {
                        let v = floats(c.text().unwrap_or(""));
                        if v.len() >= 3 {
                            color = [v[0], v[1], v[2]].map(|x| (x.clamp(0.0, 1.0) * 255.0).round() as u8);
                        }
                    }
                    if let Some(t) = kid(d, "texture")
                        && let Some(sampler) = t.attribute("texture")
                    {
                        let image = self.sampler_image(profile, sampler);
                        if let Some((n, bytes)) = image {
                            texture = crate::tex::texture_from_bytes(&n, bytes, None);
                            if let Some(t) = &texture {
                                color = t.avg;
                            }
                        }
                    }
                }
                let a_one =
                    kid(sh, "transparent").is_none_or(|t| t.attribute("opaque").is_none_or(|o| o != "RGB_ZERO"));
                let alpha = kid(sh, "transparent")
                    .and_then(|t| kid(t, "color"))
                    .map(|c| floats(c.text().unwrap_or("")))
                    .and_then(|v| v.get(3).copied())
                    .unwrap_or(1.0);
                if let Some(f) = kid(sh, "transparency")
                    .and_then(|t| kid(t, "float"))
                    .and_then(|f| f.text()?.trim().parse::<f64>().ok())
                {
                    let o = if a_one { alpha * f } else { 1.0 - f };
                    // Files that write 0 for opaque (a common mistake) stay opaque.
                    opacity = if o <= 0.0 { 1.0 } else { o.clamp(0.0, 1.0) as f32 };
                }
            }
        }
        let i = self.scene.materials.len() as u32;
        self.scene.materials.push(MatIn { name, color, opacity, texture });
        self.mats.insert(url.to_string(), i);
        Some(i)
    }

    fn sampler_image(&mut self, profile: Option<roxmltree::Node>, sid: &str) -> Option<(String, Vec<u8>)> {
        let params: Vec<roxmltree::Node> = profile.map(|p| kids(p, "newparam").collect()).unwrap_or_default();
        let find = |sid: &str| params.iter().find(|p| p.attribute("sid") == Some(sid)).copied();
        let image_id = match find(sid) {
            Some(p) => {
                let sampler = kid(p, "sampler2D")?;
                if let Some(ii) = kid(sampler, "instance_image") {
                    ii.attribute("url")?.trim_start_matches('#').to_string()
                } else {
                    let surf_sid = kid(sampler, "source")?.text()?.trim().to_string();
                    let surf = find(&surf_sid)?;
                    kid(kid(surf, "surface")?, "init_from")?.text()?.trim().to_string()
                }
            }
            // Some writers name the image directly.
            None => sid.to_string(),
        };
        let img = self.doc.get(&image_id)?;
        let init = kid(img, "init_from")?;
        let reference = kid(init, "ref").and_then(|r| r.text()).or(init.text())?.trim().to_string();
        self.images.image(&reference)
    }

    fn geometry(&mut self, url: &str, binds: &[(String, u32)]) -> Option<usize> {
        let key = (url.to_string(), binds.to_vec());
        if let Some(&m) = self.meshes.get(&key) {
            return Some(m);
        }
        let g = self.doc.get(url)?;
        let mesh_el = kid(g, "mesh")?;
        let mut mesh = MeshIn::default();
        let vertices = kid(mesh_el, "vertices");
        for prim in mesh_el.children().filter(|c| c.is_element()) {
            let tag = prim.tag_name().name();
            if !matches!(tag, "triangles" | "polylist" | "polygons" | "tristrips" | "trifans" | "lines" | "linestrips")
            {
                continue;
            }
            let symbol = prim.attribute("material").unwrap_or("");
            let mat = binds.iter().find(|b| b.0 == symbol).map(|b| b.1);
            // Inputs: (semantic, source url, offset, set).
            let mut inputs: Vec<(String, String, usize)> = Vec::new();
            for inp in kids(prim, "input") {
                let sem = inp.attribute("semantic").unwrap_or("").to_string();
                let src = inp.attribute("source").unwrap_or("").to_string();
                let off = inp.attribute("offset").and_then(|x| x.parse().ok()).unwrap_or(0usize);
                if sem == "VERTEX" {
                    if let Some(v) = vertices {
                        for vi in kids(v, "input") {
                            inputs.push((
                                vi.attribute("semantic").unwrap_or("").to_string(),
                                vi.attribute("source").unwrap_or("").to_string(),
                                off,
                            ));
                        }
                    }
                } else {
                    inputs.push((sem, src, off));
                }
            }
            // Offsets from the file: an input past the number of indices is no input.
            inputs.retain(|i| i.2 < 1 << 16);
            let stride = inputs.iter().map(|i| i.2).max().map_or(1, |m| m + 1);
            let get = |me: &mut Self, sem: &str| -> Option<(Vec<f64>, usize, usize)> {
                let (_, src, off) = inputs.iter().find(|i| i.0 == sem)?;
                let (d, s) = me.doc.source(src)?;
                Some((d, s, *off))
            };
            let Some((pos, ps, po)) = get(self, "POSITION") else { continue };
            let nor = get(self, "NORMAL");
            let tex = get(self, "TEXCOORD");
            let textured = mat.is_some_and(|m| self.scene.materials[m as usize].texture.is_some());
            // Polygons as lists of index tuples.
            let mut polys: Vec<Vec<usize>> = Vec::new();
            let mut holes: Vec<Vec<Vec<usize>>> = Vec::new();
            let mut lines: Vec<Vec<usize>> = Vec::new();
            match tag {
                "triangles" => {
                    let p: Vec<i64> = kids(prim, "p").flat_map(|p| ints(p.text().unwrap_or(""))).collect();
                    for t in 0..p.len() / (stride * 3) {
                        polys.push((0..3).map(|k| (t * 3 + k) * stride).collect());
                    }
                    holes.resize(polys.len(), Vec::new());
                    self.emit(&mut mesh, &p, &polys, &holes, &pos, ps, po, nor.as_ref(), tex.as_ref(), mat, textured);
                    continue;
                }
                "polylist" => {
                    let vc: Vec<i64> = kid(prim, "vcount").map(|v| ints(v.text().unwrap_or(""))).unwrap_or_default();
                    let p: Vec<i64> = kid(prim, "p").map(|p| ints(p.text().unwrap_or(""))).unwrap_or_default();
                    let mut at = 0usize;
                    for n in vc {
                        // A count from the file, kept to the indices <p> really has: one
                        // vcount of 9223372036854775807 made a list that large at once.
                        let left = p.len().saturating_sub(at) / stride;
                        let n = (n.max(0) as usize).min(left);
                        polys.push((0..n).map(|k| at + k * stride).collect());
                        at += n * stride;
                    }
                    holes.resize(polys.len(), Vec::new());
                    self.emit(&mut mesh, &p, &polys, &holes, &pos, ps, po, nor.as_ref(), tex.as_ref(), mat, textured);
                    continue;
                }
                "polygons" => {
                    // <p> polygons, and <ph> polygons with holes <h>.
                    let mut all: Vec<i64> = Vec::new();
                    for el in prim.children().filter(|c| c.is_element()) {
                        match el.tag_name().name() {
                            "p" => {
                                let v = ints(el.text().unwrap_or(""));
                                let start = all.len();
                                all.extend(&v);
                                polys.push((0..v.len() / stride).map(|k| start + k * stride).collect());
                                holes.push(Vec::new());
                            }
                            "ph" => {
                                let mut outer = Vec::new();
                                let mut hs = Vec::new();
                                for sub in el.children().filter(|c| c.is_element()) {
                                    let v = ints(sub.text().unwrap_or(""));
                                    let start = all.len();
                                    all.extend(&v);
                                    let l: Vec<usize> = (0..v.len() / stride).map(|k| start + k * stride).collect();
                                    if sub.tag_name().name() == "p" {
                                        outer = l;
                                    } else {
                                        hs.push(l);
                                    }
                                }
                                polys.push(outer);
                                holes.push(hs);
                            }
                            _ => {}
                        }
                    }
                    self.emit(&mut mesh, &all, &polys, &holes, &pos, ps, po, nor.as_ref(), tex.as_ref(), mat, textured);
                    continue;
                }
                "tristrips" | "trifans" => {
                    let mut all: Vec<i64> = Vec::new();
                    for p in kids(prim, "p") {
                        let v = ints(p.text().unwrap_or(""));
                        let start = all.len();
                        all.extend(&v);
                        let n = v.len() / stride;
                        for k in 2..n {
                            let (a, b, c) = if tag == "trifans" {
                                (0, k - 1, k)
                            } else if k % 2 == 0 {
                                (k - 2, k - 1, k)
                            } else {
                                (k - 1, k - 2, k)
                            };
                            polys.push(vec![start + a * stride, start + b * stride, start + c * stride]);
                        }
                    }
                    holes.resize(polys.len(), Vec::new());
                    self.emit(&mut mesh, &all, &polys, &holes, &pos, ps, po, nor.as_ref(), tex.as_ref(), mat, textured);
                    continue;
                }
                "lines" => {
                    let p: Vec<i64> = kids(prim, "p").flat_map(|p| ints(p.text().unwrap_or(""))).collect();
                    for k in 0..p.len() / (stride * 2) {
                        lines.push(vec![k * 2 * stride, (k * 2 + 1) * stride]);
                    }
                    self.emit_lines(&mut mesh, &p, &lines, &pos, ps, po, false);
                }
                _ => {
                    let mut all: Vec<i64> = Vec::new();
                    for p in kids(prim, "p") {
                        let v = ints(p.text().unwrap_or(""));
                        let start = all.len();
                        all.extend(&v);
                        lines.push((0..v.len() / stride).map(|k| start + k * stride).collect());
                    }
                    self.emit_lines(&mut mesh, &all, &lines, &pos, ps, po, false);
                }
            }
        }
        let i = self.scene.add_mesh(mesh);
        self.meshes.insert(key, i);
        Some(i)
    }

    #[allow(clippy::too_many_arguments)]
    fn emit(
        &mut self,
        mesh: &mut MeshIn,
        p: &[i64],
        polys: &[Vec<usize>],
        holes: &[Vec<Vec<usize>>],
        pos: &[f64],
        ps: usize,
        po: usize,
        nor: Option<&(Vec<f64>, usize, usize)>,
        tex: Option<&(Vec<f64>, usize, usize)>,
        mat: Option<u32>,
        textured: bool,
    ) {
        let mut cache: HashMap<i64, u32> = HashMap::new();
        // Indices and strides from the file: checked arithmetic (an index of 2^62 times a
        // stride overflowed).
        let at = |i: i64, s: usize| usize::try_from(i.max(0)).ok()?.checked_mul(s);
        let mut vertex = |mesh: &mut MeshIn, corner: usize| -> Option<u32> {
            let i = *p.get(corner.checked_add(po)?)?;
            if let Some(&v) = cache.get(&i) {
                return Some(v);
            }
            let k = at(i, ps)?;
            let x = pos.get(k..k.checked_add(3)?)?;
            mesh.pos.push(DVec3::new(x[0], x[1], x[2]));
            let v = mesh.pos.len() as u32 - 1;
            cache.insert(i, v);
            Some(v)
        };
        for (pi, poly) in polys.iter().enumerate() {
            let Some(outer) = poly.iter().map(|&c| vertex(mesh, c)).collect::<Option<Vec<u32>>>() else {
                self.doc.problems.push("indice de sommet hors limites".into());
                continue;
            };
            let hs: Vec<Vec<u32>> = holes
                .get(pi)
                .map(|h| {
                    h.iter().filter_map(|l| l.iter().map(|&c| vertex(mesh, c)).collect::<Option<Vec<u32>>>()).collect()
                })
                .unwrap_or_default();
            let normals = nor
                .map(|(d, s, o)| {
                    poly.iter()
                        .filter_map(|&c| {
                            let i = at(*p.get(c.checked_add(*o)?)?, *s)?;
                            let x = d.get(i..i.checked_add(3)?)?;
                            Some(DVec3::new(x[0], x[1], x[2]))
                        })
                        .collect::<Vec<_>>()
                })
                .filter(|n| n.len() == poly.len())
                .unwrap_or_default();
            let uv = if textured {
                tex.map(|(d, s, o)| {
                    poly.iter()
                        .chain(holes.get(pi).into_iter().flatten().flatten())
                        .filter_map(|&c| {
                            let i = at(*p.get(c.checked_add(*o)?)?, *s)?;
                            let x = d.get(i..i.checked_add(2)?)?;
                            Some(DVec2::new(x[0], x[1]))
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
            } else {
                Vec::new()
            };
            mesh.polys.push(PolyIn { outer, holes: hs, mat, uv, normals, ..Default::default() });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_lines(
        &mut self,
        mesh: &mut MeshIn,
        p: &[i64],
        lines: &[Vec<usize>],
        pos: &[f64],
        ps: usize,
        po: usize,
        closed: bool,
    ) {
        for l in lines {
            let mut pts = Vec::new();
            for &c in l {
                let Some(&i) = c.checked_add(po).and_then(|x| p.get(x)) else { continue };
                let Some(k) = usize::try_from(i.max(0)).ok().and_then(|i| i.checked_mul(ps)) else { continue };
                if let Some(x) = k.checked_add(3).and_then(|e| pos.get(k..e)) {
                    mesh.pos.push(DVec3::new(x[0], x[1], x[2]));
                    pts.push(mesh.pos.len() as u32 - 1);
                }
            }
            if pts.len() >= 2 {
                mesh.lines.push(LineIn { pts, closed, layer: None });
            }
        }
    }

    /// Bindings of an `<instance_geometry>`: symbol → material.
    fn binds(&mut self, ig: roxmltree::Node) -> Vec<(String, u32)> {
        let mut out = Vec::new();
        let tc = kid(ig, "bind_material").and_then(|b| kid(b, "technique_common"));
        for im in tc.into_iter().flat_map(|t| kids(t, "instance_material")) {
            let (Some(sym), Some(target)) = (im.attribute("symbol"), im.attribute("target")) else { continue };
            if let Some(m) = self.material(target) {
                out.push((sym.to_string(), m));
            }
        }
        out
    }

    /// The contents of a `<node>` (geometry, child nodes, instanced nodes).
    fn node(&mut self, n: roxmltree::Node) -> NodeIn {
        self.depth += 1;
        let mut out = NodeIn {
            name: n.attribute("name").or(n.attribute("id")).unwrap_or("").to_string(),
            xform: node_matrix(n),
            ..Default::default()
        };
        if self.depth > 64 {
            self.depth -= 1;
            return out;
        }
        for c in n.children().filter(|c| c.is_element()) {
            match c.tag_name().name() {
                "instance_geometry" => {
                    let binds = self.binds(c);
                    if let Some(url) = c.attribute("url")
                        && let Some(m) = self.geometry(url, &binds)
                    {
                        out.meshes.push(m);
                    }
                }
                "node" => {
                    let child = self.node(c);
                    out.children.push(child);
                }
                "instance_node" => {
                    if let Some(url) = c.attribute("url")
                        && let Some(d) = self.def(url)
                    {
                        out.children.push(NodeIn { def: Some(d), ..Default::default() });
                    }
                }
                _ => {}
            }
        }
        self.depth -= 1;
        out
    }

    fn def(&mut self, url: &str) -> Option<usize> {
        if let Some(&d) = self.nodes.get(url) {
            return Some(d);
        }
        let n = self.doc.get(url)?;
        // Reserve the slot first (cycles end here).
        let idx = self.scene.defs.len();
        self.scene.defs.push(DefIn::default());
        self.nodes.insert(url.to_string(), idx);
        let body = self.node(n);
        let d = &mut self.scene.defs[idx];
        d.name = body.name.clone();
        d.meshes = Vec::new();
        d.children = vec![NodeIn {
            xform: body.xform,
            meshes: body.meshes,
            children: body.children,
            kind: crate::build::NodeKind::Flatten,
            ..Default::default()
        }];
        Some(idx)
    }
}

/// Unit (millimetres per document unit) and up axis of a document.
fn asset<'a>(root: roxmltree::Node<'a, '_>) -> (f64, &'a str) {
    let asset = kid(root, "asset");
    let meter = asset
        .and_then(|a| kid(a, "unit"))
        .and_then(|u| u.attribute("meter"))
        .and_then(|m| m.trim().parse::<f64>().ok());
    let up = asset.and_then(|a| kid(a, "up_axis")).and_then(|u| u.text()).map(str::trim).unwrap_or("Y_UP");
    (meter.filter(|m| *m > 0.0).unwrap_or(1.0) * 1000.0, up)
}

/// Reads a COLLADA document. With `validate`, a document that refers to missing elements
/// or whose arrays do not match their counts is refused.
pub fn read(text: &str, images: &dyn ImageSource, validate: bool) -> Result<(SceneIn, f64, bool)> {
    // A DOCTYPE is accepted, not the entities it may declare: roxmltree limits the nesting
    // of references but not their number at the top level, so a few hundred kilobytes of
    // `&a;` (each expanding to 255 × `&b;`) made gigabytes of text. COLLADA writers declare
    // no entity.
    if text.contains("<!ENTITY") {
        bail!("fichier COLLADA refusé : il déclare des entités XML");
    }
    let opts = roxmltree::ParsingOptions { allow_dtd: true, nodes_limit: u32::MAX, ..Default::default() };
    let xml = roxmltree::Document::parse_with_options(text, opts).context("fichier COLLADA illisible")?;
    let root = xml.root_element();
    if root.tag_name().name() != "COLLADA" {
        bail!("pas un document COLLADA");
    }
    let ids = xml.descendants().filter_map(|n| Some((n.attribute("id")?, n))).collect();
    let mut imp = Importer {
        doc: Doc { ids, problems: Vec::new() },
        scene: SceneIn::default(),
        mats: HashMap::new(),
        images,
        meshes: HashMap::new(),
        nodes: HashMap::new(),
        depth: 0,
    };
    let (mm, up) = asset(root);
    let vs = kid(root, "scene")
        .and_then(|s| kid(s, "instance_visual_scene"))
        .and_then(|i| i.attribute("url"))
        .and_then(|u| imp.doc.get(u))
        .or_else(|| kid(root, "library_visual_scenes").and_then(|l| kid(l, "visual_scene")));
    let Some(vs) = vs else { bail!("le document COLLADA n'a pas de scène") };
    for n in kids(vs, "node") {
        let mut node = imp.node(n);
        // SketchUp's own root node holds the model's loose geometry.
        if node.name == "SketchUp" {
            node.kind = crate::build::NodeKind::Flatten;
        }
        imp.scene.root.children.push(node);
    }
    if validate && !imp.doc.problems.is_empty() {
        bail!("Le fichier DAE n'est pas valide : {}", imp.doc.problems.join(" ; "));
    }
    let swap = up == "Y_UP";
    if up == "X_UP" {
        // X up: turn the scene so that X goes to Z.
        let m =
            DMat4::from_cols_array(&[0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        imp.scene.root.xform = m;
    }
    Ok((imp.scene, mm, swap))
}

/// File › Import of a COLLADA file.
pub fn import(text: &str, file: Option<&Path>, o: &DaeImport) -> Result<Model> {
    let (scene, mm, swap) = read(text, &Beside(file), o.validate)?;
    Ok(crate::build::build(
        &scene,
        BuildOpts { scale: mm, swap_yz: swap, merge_coplanar: o.merge_coplanar, orient_faces: false },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use esq_core::model::{Definition, Instance};

    fn model() -> Model {
        let mut m = Model::new();
        let mut cube = esq_core::model::Entities::default();
        let s = 254.0;
        let p = [DVec3::ZERO, DVec3::new(s, 0.0, 0.0), DVec3::new(s, s, 0.0), DVec3::new(0.0, s, 0.0)];
        let v: Vec<_> = p.iter().map(|x| cube.add_vertex_raw(*x)).collect();
        let top: Vec<_> = p.iter().map(|x| cube.add_vertex_raw(*x + DVec3::Z * s)).collect();
        cube.add_face_raw(vec![v[0], v[3], v[2], v[1]], vec![]);
        cube.add_face_raw(top.clone(), vec![]);
        for i in 0..4 {
            let j = (i + 1) % 4;
            cube.add_face_raw(vec![v[i], v[j], top[j], top[i]], vec![]);
        }
        let d = m.defs.insert(Definition {
            name: "Cube".into(),
            is_group: false,
            description: String::new(),
            entities: cube,
            attributes: Default::default(),
            ext: Default::default(),
        });
        let red = m.add_material("Rouge", [255, 0, 0]);
        for (i, mat) in [(0.0, None), (1000.0, Some(red))] {
            m.root.instances.insert(Instance {
                def: d,
                xform: DMat4::from_translation(DVec3::new(i, 0.0, 0.0)),
                name: String::new(),
                material: mat,
                layer: 0,
                hidden: false,
                locked: false,
                attributes: Default::default(),
                ext: Default::default(),
            });
        }
        m
    }

    #[test]
    fn writes_sketchup_layout() {
        let m = model();
        let (doc, extras) = write(&m, &DaeExport::default(), &crate::ExportCx::default(), "x");
        assert!(extras.is_empty());
        assert!(doc.contains("<unit meter=\"0.0254\" name=\"inch\" />"));
        assert!(doc.contains("<up_axis>Z_UP</up_axis>"));
        assert!(doc.contains("<node name=\"SketchUp\">"));
        assert!(doc.contains("<instance_node url=\"#"));
        assert!(doc.contains("name=\"instance_0\""));
        // The painted instance uses its own copy of the definition with red faces.
        assert_eq!(doc.matches("<node id=").count(), 4, "{doc}");
        assert!(doc.contains("<color>1 0 0 1</color>"));
        // Two-sided: 12 front + 12 back triangles per copy.
        assert!(doc.contains("<triangles count=\"12\""));
    }

    #[test]
    fn round_trip_keeps_components_units_and_materials() {
        let m = model();
        let (doc, _) =
            write(&m, &DaeExport { two_sided: false, ..Default::default() }, &crate::ExportCx::default(), "x");
        let back = import(&doc, None, &DaeImport { validate: true, merge_coplanar: true }).unwrap();
        // Two instances at the root, of components holding a cube of 6 faces.
        assert_eq!(back.root.instances.len(), 2);
        let mut faces = 0;
        let mut xs = Vec::new();
        esq_core::walk::visit(&back, |pl| {
            faces += pl.ents.faces.len();
            for v in pl.ents.verts.values() {
                xs.push(pl.xform.transform_point3(v.pos).x);
            }
        });
        assert_eq!(faces, 12);
        let maxx = xs.iter().cloned().fold(f64::MIN, f64::max);
        assert!((maxx - 1254.0).abs() < 1e-4, "{maxx}");
        assert!(back.materials.values().any(|x| x.name == "Rouge" && x.color == [255, 0, 0]));
    }

    #[test]
    fn reads_y_up_polylists_and_textures() {
        let dir = std::env::temp_dir().join(format!("esq_dae_{}", std::process::id()));
        std::fs::create_dir_all(dir.join("tex")).unwrap();
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(2, 2, image::Rgb([10, 200, 30])))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        std::fs::write(dir.join("tex").join("herbe.png"), &png).unwrap();
        let doc = r##"<?xml version="1.0"?>
<COLLADA xmlns="http://www.collada.org/2005/11/COLLADASchema" version="1.4.1">
 <asset><unit meter="1"/><up_axis>Y_UP</up_axis></asset>
 <library_images><image id="img"><init_from>../../evil/tex/herbe.png</init_from></image></library_images>
 <library_effects><effect id="fx"><profile_COMMON>
  <newparam sid="surf"><surface type="2D"><init_from>img</init_from></surface></newparam>
  <newparam sid="samp"><sampler2D><source>surf</source></sampler2D></newparam>
  <technique sid="c"><phong><diffuse><texture texture="samp" texcoord="t"/></diffuse></phong></technique>
 </profile_COMMON></effect></library_effects>
 <library_materials><material id="m" name="Herbe"><instance_effect url="#fx"/></material></library_materials>
 <library_geometries><geometry id="g"><mesh>
  <source id="p"><float_array id="pa" count="12">0 0 0 2 0 0 2 0 -2 0 0 -2</float_array>
   <technique_common><accessor source="#pa" count="4" stride="3"/></technique_common></source>
  <source id="t"><float_array id="ta" count="8">0 0 1 0 1 1 0 1</float_array>
   <technique_common><accessor source="#ta" count="4" stride="2"/></technique_common></source>
  <vertices id="v"><input semantic="POSITION" source="#p"/></vertices>
  <polylist count="1" material="sym"><input semantic="VERTEX" source="#v" offset="0"/>
   <input semantic="TEXCOORD" source="#t" offset="1"/><vcount>4</vcount><p>0 0 1 1 2 2 3 3</p></polylist>
 </mesh></geometry></library_geometries>
 <library_visual_scenes><visual_scene id="s"><node name="sol">
  <instance_geometry url="#g"><bind_material><technique_common>
   <instance_material symbol="sym" target="#m"/></technique_common></bind_material></instance_geometry>
 </node></visual_scene></library_visual_scenes>
 <scene><instance_visual_scene url="#s"/></scene>
</COLLADA>"##;
        let path = dir.join("sol.dae");
        let m = import(doc, Some(&path), &DaeImport::default()).unwrap();
        let mut found = None;
        esq_core::walk::visit(&m, |pl| {
            if let Some(f) = pl.ents.faces.values().next() {
                let pts = pl.ents.verts.values().map(|v| pl.xform.transform_point3(v.pos)).collect::<Vec<_>>();
                found = Some((pl.xform.transform_vector3(f.normal), f.front, f.tex.is_some(), pts));
            }
        });
        let (n, front, tex, pts) = found.unwrap();
        // Y up in the file: the square lies flat, 2 m wide, facing up.
        assert!(n.z.abs() > 0.99, "{n:?}");
        assert!(pts.iter().any(|p| (p.y - 2000.0).abs() < 1e-6 || (p.x - 2000.0).abs() < 1e-6));
        let mat = &m.materials[front.unwrap()];
        assert_eq!(mat.name, "Herbe");
        let t = mat.ext.texture.as_ref().unwrap();
        assert_eq!(t.filename, "herbe.png");
        assert_eq!(t.px, [2, 2]);
        assert!(tex);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn validation_refuses_broken_references() {
        let doc = r##"<COLLADA><library_visual_scenes><visual_scene id="s"><node><instance_geometry url="#nope"/></node></visual_scene></library_visual_scenes><scene><instance_visual_scene url="#s"/></scene></COLLADA>"##;
        assert!(import(doc, None, &DaeImport { validate: true, merge_coplanar: true }).is_err());
        assert!(import(doc, None, &DaeImport { validate: false, merge_coplanar: true }).is_ok());
    }

    /// A triangle mesh with the given primitive element, in a document whose scene shows it.
    fn doc_with(prim: &str, extra_nodes: &str) -> String {
        format!(
            r##"<COLLADA><library_geometries><geometry id="g"><mesh>
<source id="p"><float_array id="pa" count="9">0 0 0 1 0 0 0 1 0</float_array>
<technique_common><accessor source="#pa" count="3" stride="3"/></technique_common></source>
<vertices id="v"><input semantic="POSITION" source="#p"/></vertices>
{prim}</mesh></geometry></library_geometries>
<library_nodes><node id="boucle"><instance_geometry url="#g"/><instance_node url="#boucle"/></node></library_nodes>
<library_visual_scenes><visual_scene id="s"><node><instance_geometry url="#g"/>{extra_nodes}</node></visual_scene></library_visual_scenes>
<scene><instance_visual_scene url="#s"/></scene></COLLADA>"##
        )
    }

    /// `<vcount>` of 9223372036854775807: the polygon's index list was made that long at
    /// once (capacity overflow, the import died). An `offset` of 2^64 - 1 overflowed.
    #[test]
    fn counts_and_offsets_from_the_file_are_bounded() {
        let big = doc_with(
            r##"<polylist count="1"><input semantic="VERTEX" source="#v" offset="0"/><vcount>9223372036854775807</vcount><p>0 1 2</p></polylist>"##,
            "",
        );
        assert!(import(&big, None, &DaeImport::default()).is_ok());
        let off = doc_with(
            r##"<triangles count="1"><input semantic="VERTEX" source="#v" offset="18446744073709551615"/><p>0 1 2</p></triangles>"##,
            "",
        );
        let _ = import(&off, None, &DaeImport::default());
        let idx = doc_with(
            r##"<triangles count="1"><input semantic="VERTEX" source="#v" offset="0"/><p>9223372036854775807 1 2</p></triangles>"##,
            "",
        );
        let _ = import(&idx, None, &DaeImport::default());
    }

    /// A node that instances itself (`<instance_node url="#boucle">` inside `boucle`): the
    /// definition recursed until the stack overflowed.
    #[test]
    fn a_node_that_instances_itself_ends() {
        let doc = doc_with(
            r##"<triangles count="1"><input semantic="VERTEX" source="#v" offset="0"/><p>0 1 2</p></triangles>"##,
            r##"<instance_node url="#boucle"/>"##,
        );
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(
                import(&doc, None, &DaeImport::default()).map(|m| m.defs.values().filter(|d| !d.is_group).count()),
            );
        });
        // The scene's node (a group) and the one component, which does not contain itself.
        let defs = rx.recv_timeout(std::time::Duration::from_secs(20)).expect("the import ends").unwrap();
        assert_eq!(defs, 1);
    }

    /// Entities declared in the DOCTYPE ("billion laughs" spread over many top-level
    /// references, which roxmltree does not limit): refused before parsing. A DOCTYPE
    /// without entities still reads.
    #[test]
    fn entity_declarations_are_refused() {
        let mut doc = String::from("<?xml version=\"1.0\"?><!DOCTYPE COLLADA [<!ENTITY b \"");
        doc.push_str(&"x".repeat(10_000));
        doc.push_str("\"><!ENTITY a \"");
        doc.push_str(&"&b;".repeat(255));
        doc.push_str("\">]><COLLADA><asset><title>");
        doc.push_str(&"&a;".repeat(10_000));
        doc.push_str("</title></asset></COLLADA>");
        let e = import(&doc, None, &DaeImport::default()).unwrap_err();
        assert!(format!("{e:#}").contains("entités"), "{e:#}");
        let plain = format!(
            "<?xml version=\"1.0\"?><!DOCTYPE COLLADA>{}",
            doc_with(
                r##"<triangles count="1"><input semantic="VERTEX" source="#v" offset="0"/><p>0 1 2</p></triangles>"##,
                ""
            )
        );
        assert!(import(&plain, None, &DaeImport::default()).is_ok());
    }
}
