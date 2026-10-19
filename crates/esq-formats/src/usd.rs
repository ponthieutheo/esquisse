//! USD: the .usdz SketchUp exports (SimLab exporter: a binary `.usdc` layer and its textures
//! in a 64-byte aligned ZIP, Y up, metres, UsdPreviewSurface materials) and the USD importer
//! (.usdz, .usd, .usda, .usdc, composed by the `openusd` crate: references, payloads,
//! instancing).

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use esq_core::glam::{DMat4, DQuat, DVec2, DVec3};
use esq_core::model::{DefId, MaterialId, Model};
use openusd::sdf::Value as V;
use openusd::usd;

use crate::build::{BuildOpts, DefIn, MatIn, MeshIn, NodeIn, PolyIn, SceneIn};
use crate::obj::{BACK_COLOR, FRONT_COLOR};
use crate::options::UsdzImport;
use crate::scene::{Cache, Collection, Opts, WorldFace};
use crate::tex::TexNames;

// ------------------------------------------------------------------------------------------
// Writing

/// A USD identifier ("Brique rouge" → "Brique_rouge").
fn ident(s: &str) -> String {
    let mut o: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
    if o.is_empty() || o.starts_with(|c: char| c.is_ascii_digit()) {
        o.insert(0, '_');
    }
    o
}

fn f(x: f64) -> String {
    let s = format!("{x:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() { "0".into() } else { s.to_string() }
}

struct W<'a> {
    model: &'a Model,
    out: String,
    depth: usize,
    conv: DMat4,
    /// (material, side) → prim path of the USD material.
    mats: HashMap<(Option<MaterialId>, bool), String>,
    mat_text: String,
    texnames: TexNames,
    textures: Vec<(String, Vec<u8>)>,
}

impl W<'_> {
    fn line(&mut self, s: &str) {
        for _ in 0..self.depth {
            self.out.push_str("    ");
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn open(&mut self, head: &str) {
        self.line(head);
        self.line("{");
        self.depth += 1;
    }

    fn close(&mut self) {
        self.depth -= 1;
        self.line("}");
    }

    fn material(&mut self, mat: Option<MaterialId>, front: bool) -> String {
        let key = (mat, mat.is_some() || front);
        if let Some(p) = self.mats.get(&key) {
            return p.clone();
        }
        let m = mat.and_then(|m| self.model.materials.get(m));
        let base = ident(m.map_or(if front { "FrontColor" } else { "BackColor" }, |m| m.name.as_str()));
        let mut name = base.clone();
        let mut i = 1;
        while self.mats.values().any(|p| p.ends_with(&format!("/{name}"))) {
            name = format!("{base}_{i}");
            i += 1;
        }
        let path = format!("/SketchUp/Materials/{name}");
        self.mats.insert(key, path.clone());
        let (c, opacity) = match m {
            Some(m) => (m.color, m.opacity as f64),
            None if front => (FRONT_COLOR, 1.0),
            None => (BACK_COLOR, 1.0),
        };
        let t = &mut self.mat_text;
        let _ = writeln!(t, "        def Material \"{name}\"");
        let _ = writeln!(t, "        {{");
        let _ = writeln!(t, "            token outputs:surface.connect = <{path}/Surface.outputs:surface>");
        let _ = writeln!(t);
        let _ = writeln!(t, "            def Shader \"Surface\"");
        let _ = writeln!(t, "            {{");
        let _ = writeln!(t, "                uniform token info:id = \"UsdPreviewSurface\"");
        let _ = writeln!(
            t,
            "                color3f inputs:diffuseColor = ({}, {}, {})",
            f(c[0] as f64 / 255.0),
            f(c[1] as f64 / 255.0),
            f(c[2] as f64 / 255.0)
        );
        let tex = m.and_then(|m| m.ext.texture.as_ref());
        if tex.is_some() {
            let _ = writeln!(t, "                color3f inputs:diffuseColor.connect = <{path}/Texture.outputs:rgb>");
        }
        let _ = writeln!(t, "                float inputs:opacity = {}", f(opacity));
        let _ = writeln!(t, "                float inputs:roughness = 1");
        let _ = writeln!(t, "                float inputs:metallic = 0");
        let _ = writeln!(t, "                token outputs:surface");
        let _ = writeln!(t, "            }}");
        if let Some(tx) = tex {
            let file = self.texnames.name_for(tx);
            self.textures.push((format!("textures/{file}"), tx.image.0.to_vec()));
            let t = &mut self.mat_text;
            let _ = writeln!(t);
            let _ = writeln!(t, "            def Shader \"Texture\"");
            let _ = writeln!(t, "            {{");
            let _ = writeln!(t, "                uniform token info:id = \"UsdUVTexture\"");
            let _ = writeln!(t, "                asset inputs:file = @textures/{file}@");
            let _ = writeln!(t, "                float2 inputs:st.connect = <{path}/Primvar.outputs:result>");
            let _ = writeln!(t, "                token inputs:wrapS = \"repeat\"");
            let _ = writeln!(t, "                token inputs:wrapT = \"repeat\"");
            let _ = writeln!(t, "                float3 outputs:rgb");
            let _ = writeln!(t, "            }}");
            let _ = writeln!(t);
            let _ = writeln!(t, "            def Shader \"Primvar\"");
            let _ = writeln!(t, "            {{");
            let _ = writeln!(t, "                uniform token info:id = \"UsdPrimvarReader_float2\"");
            let _ = writeln!(t, "                token inputs:varname = \"st\"");
            let _ = writeln!(t, "                float2 outputs:result");
            let _ = writeln!(t, "            }}");
        }
        let _ = writeln!(self.mat_text, "        }}");
        path
    }

    fn mesh(&mut self, coll: &Collection, inherited: Option<MaterialId>) {
        if coll.faces.is_empty() {
            return;
        }
        let mut pts: Vec<DVec3> = Vec::new();
        let mut index: HashMap<[u64; 3], usize> = HashMap::new();
        let (mut counts, mut idx, mut normals, mut st) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut subsets: Vec<(String, Vec<usize>)> = Vec::new();
        let mut nface = 0;
        for front in [true, false] {
            for face in &coll.faces {
                let wf = WorldFace {
                    face,
                    xform: DMat4::IDENTITY,
                    front: face.front.or(inherited),
                    back: face.back.or(inherited),
                };
                let mat = if front { wf.front } else { wf.back };
                let mpath = self.material(mat, front);
                let uvs = crate::obj::face_uvs(self.model, &wf, mat, front);
                let n = self.conv.transform_vector3(if front { face.normal } else { -face.normal }).normalize_or_zero();
                let p: Vec<DVec3> = face.points().iter().map(|x| self.conv.transform_point3(*x)).collect();
                for t in wf.tris() {
                    let t = if front { t } else { [t[0], t[2], t[1]] };
                    counts.push(3);
                    for &c in &t {
                        let q = p[c];
                        let k = *index.entry(q.to_array().map(f64::to_bits)).or_insert_with(|| {
                            pts.push(q);
                            pts.len() - 1
                        });
                        idx.push(k);
                        normals.push(n);
                        st.push(uvs[c]);
                    }
                    match subsets.iter_mut().find(|s| s.0 == mpath) {
                        Some(s) => s.1.push(nface),
                        None => subsets.push((mpath.clone(), vec![nface])),
                    }
                    nface += 1;
                }
            }
        }
        let list = |v: Vec<String>| format!("[{}]", v.join(", "));
        self.open("def Mesh \"Mesh\" (\n    prepend apiSchemas = [\"MaterialBindingAPI\"]\n)");
        self.line(&format!("int[] faceVertexCounts = {}", list(counts.iter().map(|c: &i32| c.to_string()).collect())));
        self.line(&format!("int[] faceVertexIndices = {}", list(idx.iter().map(|i| i.to_string()).collect())));
        self.line(&format!(
            "point3f[] points = {}",
            list(pts.iter().map(|p| format!("({}, {}, {})", f(p.x), f(p.y), f(p.z))).collect())
        ));
        self.line(&format!(
            "normal3f[] normals = {} (\n    interpolation = \"faceVarying\"\n)",
            list(normals.iter().map(|p| format!("({}, {}, {})", f(p.x), f(p.y), f(p.z))).collect())
        ));
        self.line(&format!(
            "texCoord2f[] primvars:st = {} (\n    interpolation = \"faceVarying\"\n)",
            list(st.iter().map(|t: &DVec2| format!("({}, {})", f(t.x), f(t.y))).collect())
        ));
        self.line("uniform token subdivisionScheme = \"none\"");
        if subsets.len() == 1 {
            self.line(&format!("rel material:binding = <{}>", subsets[0].0));
        } else {
            for (k, (mpath, faces)) in subsets.iter().enumerate() {
                self.open(&format!(
                    "def GeomSubset \"Subset{k}\" (\n    prepend apiSchemas = [\"MaterialBindingAPI\"]\n)"
                ));
                self.line("uniform token elementType = \"face\"");
                self.line("uniform token familyName = \"materialBind\"");
                self.line(&format!("int[] indices = {}", list(faces.iter().map(|i| i.to_string()).collect())));
                self.line(&format!("rel material:binding = <{mpath}>"));
                self.close();
            }
        }
        self.close();
    }

    fn xform(&mut self, cache: &mut Cache, def: Option<DefId>, inherited: Option<MaterialId>, depth: usize) {
        let coll = cache.get(def).clone();
        self.mesh(&coll, inherited);
        if depth > 64 {
            return;
        }
        let mut children: Vec<String> = Vec::new();
        for inst in &coll.instances {
            let d = &self.model.defs[inst.def];
            let base = ident(if inst.name.is_empty() { &d.name } else { &inst.name });
            let mut name = base.clone();
            let mut i = 1;
            while children.contains(&name) || name == "Mesh" {
                name = format!("{base}_{i}");
                i += 1;
            }
            children.push(name.clone());
            let m = self.conv * inst.xform * self.conv.inverse();
            self.open(&format!("def Xform \"{name}\""));
            let rows: Vec<String> = [m.x_axis, m.y_axis, m.z_axis, m.w_axis]
                .iter()
                .map(|c| format!("({}, {}, {}, {})", f(c.x), f(c.y), f(c.z), f(c.w)))
                .collect();
            self.line(&format!("matrix4d xformOp:transform = ( {} )", rows.join(", ")));
            self.line("uniform token[] xformOpOrder = [\"xformOp:transform\"]");
            self.xform(cache, Some(inst.def), inst.material.or(inherited), depth + 1);
            self.close();
        }
    }
}

/// The USDA text of a model (Y up, metres) and its texture files.
pub fn usda(model: &Model) -> (String, Vec<(String, Vec<u8>)>) {
    let mut cache = Cache::new(model, Opts::default());
    let mut w = W {
        model,
        out: String::new(),
        depth: 1,
        conv: crate::build::y_up_to_z_up().inverse() * DMat4::from_scale(DVec3::splat(0.001)),
        mats: HashMap::new(),
        mat_text: String::new(),
        texnames: TexNames::new(),
        textures: Vec::new(),
    };
    w.xform(&mut cache, None, None, 0);
    let body = std::mem::take(&mut w.out);
    let mut text =
        String::from("#usda 1.0\n(\n    defaultPrim = \"SketchUp\"\n    metersPerUnit = 1\n    upAxis = \"Y\"\n)\n\n");
    text.push_str("def Xform \"SketchUp\" (\n    kind = \"component\"\n)\n{\n");
    if !w.mat_text.is_empty() {
        text.push_str("    def Scope \"Materials\"\n    {\n");
        text.push_str(&w.mat_text);
        text.push_str("    }\n");
    }
    text.push_str(&body);
    text.push_str("}\n");
    (text, w.textures)
}

/// File › Export › 3D Model › USDZ: a binary layer and its textures.
pub fn export_usdz(model: &Model, path: &Path) -> Result<()> {
    let (text, textures) = usda(model);
    let data = openusd::usda::parse(&text).map_err(|e| anyhow!("USD : {e}"))?;
    let mut usdc = std::io::Cursor::new(Vec::new());
    openusd::usdc::CrateWriter::write(&data, &mut usdc).map_err(|e| anyhow!("USD : {e}"))?;
    let mut usdc = usdc.into_inner();
    // The layer only uses what crate version 0.8 already had (meshes, transforms, shading
    // networks): saying so lets older USD readers (Blender, iOS 15…) open it.
    if usdc.len() > 11 && usdc.starts_with(b"PXR-USDC") {
        usdc[8..11].copy_from_slice(&[0, 8, 0]);
    }
    let mut z = crate::zipfile::Writer::new();
    z.add("model.usdc", &usdc, false, 64);
    for (name, data) in textures {
        z.add(&name, &data, false, 64);
    }
    std::fs::write(path, z.finish())?;
    Ok(())
}

// ------------------------------------------------------------------------------------------
// Reading

fn vec3s(v: Option<V>) -> Vec<DVec3> {
    match v {
        Some(V::Vec3fVec(v)) => v.iter().map(|p| DVec3::new(p.x as f64, p.y as f64, p.z as f64)).collect(),
        Some(V::Vec3dVec(v)) => v.iter().map(|p| DVec3::new(p.x, p.y, p.z)).collect(),
        Some(V::Vec3hVec(v)) => v.iter().map(|p| DVec3::new(p.x.to_f64(), p.y.to_f64(), p.z.to_f64())).collect(),
        _ => Vec::new(),
    }
}

fn vec2s(v: Option<V>) -> Vec<DVec2> {
    match v {
        Some(V::Vec2fVec(v)) => v.iter().map(|p| DVec2::new(p.x as f64, p.y as f64)).collect(),
        Some(V::Vec2dVec(v)) => v.iter().map(|p| DVec2::new(p.x, p.y)).collect(),
        Some(V::Vec2hVec(v)) => v.iter().map(|p| DVec2::new(p.x.to_f64(), p.y.to_f64())).collect(),
        _ => Vec::new(),
    }
}

fn ints(v: Option<V>) -> Vec<i64> {
    match v {
        Some(V::IntVec(v)) => v.iter().map(|x| *x as i64).collect(),
        Some(V::Int64Vec(v)) => v,
        Some(V::UintVec(v)) => v.iter().map(|x| *x as i64).collect(),
        _ => Vec::new(),
    }
}

fn num(v: Option<V>) -> Option<f64> {
    match v? {
        V::Double(x) => Some(x),
        V::Float(x) => Some(x as f64),
        V::Half(x) => Some(x.to_f64()),
        V::Int(x) => Some(x as f64),
        _ => None,
    }
}

fn token(v: Option<V>) -> Option<String> {
    match v? {
        V::Token(t) => Some(t.as_str().to_string()),
        V::String(s) => Some(s),
        _ => None,
    }
}

fn get(p: &usd::Prim, name: &str) -> Option<V> {
    p.attribute(name).get::<V>().ok().flatten()
}

/// The local transform of a prim from its `xformOpOrder`.
fn local_xform(p: &usd::Prim) -> DMat4 {
    let order: Vec<String> = match get(p, "xformOpOrder") {
        Some(V::TokenVec(t)) => t.iter().map(|x| x.as_str().to_string()).collect(),
        _ => return DMat4::IDENTITY,
    };
    let mut m = DMat4::IDENTITY;
    for op in order {
        if op == "!resetXformStack!" {
            continue;
        }
        let (invert, name) = match op.strip_prefix("!invert!") {
            Some(n) => (true, n.to_string()),
            None => (false, op.clone()),
        };
        let v = get(p, &name);
        let v3 = |v: &Option<V>| -> Option<DVec3> {
            match v.as_ref()? {
                V::Vec3f(x) => Some(DVec3::new(x.x as f64, x.y as f64, x.z as f64)),
                V::Vec3d(x) => Some(DVec3::new(x.x, x.y, x.z)),
                V::Vec3h(x) => Some(DVec3::new(x.x.to_f64(), x.y.to_f64(), x.z.to_f64())),
                V::Float(x) => Some(DVec3::splat(*x as f64)),
                V::Double(x) => Some(DVec3::splat(*x)),
                _ => None,
            }
        };
        let kind = name.trim_start_matches("xformOp:").split(':').next().unwrap_or("").to_string();
        let t = match kind.as_str() {
            "translate" => v3(&v).map(DMat4::from_translation),
            "scale" => v3(&v).map(DMat4::from_scale),
            "rotateX" => num(v).map(|a| DMat4::from_rotation_x(a.to_radians())),
            "rotateY" => num(v).map(|a| DMat4::from_rotation_y(a.to_radians())),
            "rotateZ" => num(v).map(|a| DMat4::from_rotation_z(a.to_radians())),
            k if k.starts_with("rotate") && k.len() == 9 => v3(&v).map(|a| {
                // rotateXYZ: X first, then Y, then Z (the matrix is Z·Y·X).
                let r = |c: char| match c {
                    'X' => DMat4::from_rotation_x(a.x.to_radians()),
                    'Y' => DMat4::from_rotation_y(a.y.to_radians()),
                    _ => DMat4::from_rotation_z(a.z.to_radians()),
                };
                let axes: Vec<char> = k[6..].chars().collect();
                r(axes[2]) * r(axes[1]) * r(axes[0])
            }),
            "orient" => match &v {
                Some(V::Quatf(q)) => {
                    Some(DMat4::from_quat(DQuat::from_xyzw(q.x as f64, q.y as f64, q.z as f64, q.w as f64).normalize()))
                }
                Some(V::Quatd(q)) => Some(DMat4::from_quat(DQuat::from_xyzw(q.x, q.y, q.z, q.w).normalize())),
                _ => None,
            },
            "transform" => match &v {
                Some(V::Matrix4d(x)) => Some(DMat4::from_cols_array(&x.0)),
                _ => None,
            },
            _ => None,
        };
        if let Some(t) = t {
            m *= if invert { t.inverse() } else { t };
        }
    }
    m
}

struct Reader<'a> {
    stage: &'a usd::Stage,
    scene: SceneIn,
    mats: HashMap<String, u32>,
    protos: HashMap<String, usize>,
    /// The files of a .usdz package, for its textures.
    package: Vec<crate::zipfile::Entry>,
    file: &'a Path,
}

impl Reader<'_> {
    fn image(&self, asset: &str) -> Option<(String, Vec<u8>)> {
        let name = crate::tex::safe_name(asset)?;
        let lower = name.to_lowercase();
        if let Some(e) =
            self.package.iter().find(|e| e.name.rsplit('/').next().is_some_and(|n| n.to_lowercase() == lower))
        {
            return Some((name, e.data.clone()));
        }
        crate::tex::find_beside(self.file, asset)
    }

    /// The prim a connection of `attr` points to.
    fn source(&self, attr: &usd::Attribute) -> Option<usd::Prim> {
        let c = attr.connections().ok()?.into_iter().next()?;
        self.stage.prim(c.prim_path()).ok()
    }

    fn material(&mut self, path: &str) -> Option<u32> {
        if let Some(&m) = self.mats.get(path) {
            return Some(m);
        }
        let mp = self.stage.prim(path).ok()?;
        let name = mp.path().name().unwrap_or("Matière").to_string();
        let mut color = [204u8, 204, 204];
        let mut opacity = 1.0f32;
        let mut texture = None;
        let surface = self.source(&mp.attribute("outputs:surface")).or_else(|| {
            mp.children().ok()?.into_iter().find(|c| token(get(c, "info:id")).as_deref() == Some("UsdPreviewSurface"))
        });
        if let Some(s) = surface {
            let dc = s.attribute("inputs:diffuseColor");
            if let Some(V::Vec3f(c)) = dc.get::<V>().ok().flatten() {
                color = [c.x, c.y, c.z].map(|x| (x.clamp(0.0, 1.0) * 255.0).round() as u8);
            }
            if let Some(tex) = self.source(&dc)
                && let Some(V::AssetPath(a)) = get(&tex, "inputs:file")
                && let Some((n, bytes)) = self.image(&a.authored_path)
            {
                texture = crate::tex::texture_from_bytes(&n, bytes, None);
                if let Some(t) = &texture {
                    color = t.avg;
                }
            }
            if let Some(o) = num(get(&s, "inputs:opacity")) {
                opacity = o.clamp(0.0, 1.0) as f32;
            }
        }
        let i = self.scene.materials.len() as u32;
        self.scene.materials.push(MatIn { name, color, opacity, texture });
        self.mats.insert(path.to_string(), i);
        Some(i)
    }

    /// The material bound to a prim (or one of its ancestors).
    fn bound(&mut self, p: &usd::Prim) -> Option<u32> {
        let mut cur = Some(p.clone());
        while let Some(q) = cur {
            if let Ok(t) = q.relationship("material:binding").targets()
                && let Some(m) = t.first()
            {
                return self.material(m.as_str());
            }
            cur = q.parent();
        }
        None
    }

    fn mesh(&mut self, p: &usd::Prim) -> Option<usize> {
        let pos = vec3s(get(p, "points"));
        let counts = ints(get(p, "faceVertexCounts"));
        let idx = ints(get(p, "faceVertexIndices"));
        if pos.is_empty() || counts.is_empty() {
            return None;
        }
        let left = token(get(p, "orientation")).as_deref() == Some("leftHanded");
        let st_attr = p.attribute("primvars:st");
        let st = vec2s(st_attr.get::<V>().ok().flatten());
        let st_idx = ints(get(p, "primvars:st:indices"));
        let interp =
            token(st_attr.get_metadata::<V>("interpolation").ok().flatten()).unwrap_or_else(|| "vertex".into());
        let mesh_mat = self.bound(p);
        // Face → material, from the material subsets.
        let mut face_mat: HashMap<usize, u32> = HashMap::new();
        for c in p.children().unwrap_or_default() {
            if c.type_name().ok().flatten().is_some_and(|t| t.as_str() == "GeomSubset")
                && token(get(&c, "familyName")).is_none_or(|f| f == "materialBind")
                && let Ok(t) = c.relationship("material:binding").targets()
                && let Some(m) = t.first()
                && let Some(mi) = self.material(m.as_str())
            {
                for i in ints(get(&c, "indices")) {
                    face_mat.insert(i as usize, mi);
                }
            }
        }
        let mut mesh = MeshIn { pos, ..Default::default() };
        let mut at = 0usize;
        for (fi, &n) in counts.iter().enumerate() {
            let n = n.max(0) as usize;
            if at + n > idx.len() {
                break;
            }
            let mut outer: Vec<u32> = idx[at..at + n].iter().map(|&i| i as u32).collect();
            let mat = face_mat.get(&fi).copied().or(mesh_mat);
            let textured = mat.is_some_and(|m| self.scene.materials[m as usize].texture.is_some());
            let mut uv: Vec<DVec2> = Vec::new();
            if textured && !st.is_empty() {
                let pick = |k: usize| -> Option<DVec2> {
                    let i = if st_idx.is_empty() { k } else { *st_idx.get(k)? as usize };
                    st.get(i).copied()
                };
                uv = (0..n)
                    .filter_map(|c| match interp.as_str() {
                        "faceVarying" => pick(at + c),
                        _ => pick(idx[at + c] as usize),
                    })
                    .collect();
                if uv.len() != n {
                    uv.clear();
                }
            }
            if left {
                outer.reverse();
                uv.reverse();
            }
            mesh.polys.push(PolyIn { outer, mat, uv, ..Default::default() });
            at += n;
        }
        Some(self.scene.add_mesh(mesh))
    }

    fn node(&mut self, p: &usd::Prim, depth: usize) -> Option<NodeIn> {
        if !p.is_active().unwrap_or(true) || depth > 64 {
            return None;
        }
        let ty = p.type_name().ok().flatten().map(|t| t.as_str().to_string()).unwrap_or_default();
        if matches!(
            ty.as_str(),
            "Material"
                | "Shader"
                | "NodeGraph"
                | "GeomSubset"
                | "Camera"
                | "DistantLight"
                | "SphereLight"
                | "DomeLight"
                | "RectLight"
        ) {
            return None;
        }
        let mut n =
            NodeIn { name: p.path().name().unwrap_or("").to_string(), xform: local_xform(p), ..Default::default() };
        n.hidden = token(get(p, "visibility")).as_deref() == Some("invisible");
        if ty == "Mesh"
            && let Some(m) = self.mesh(p)
        {
            n.meshes.push(m);
        }
        if p.is_instance().unwrap_or(false)
            && let Ok(Some(proto)) = p.prototype()
        {
            let key = proto.as_str().to_string();
            let d = match self.protos.get(&key) {
                Some(&d) => d,
                None => {
                    let idx = self.scene.defs.len();
                    self.scene.defs.push(DefIn { name: n.name.clone(), ..Default::default() });
                    self.protos.insert(key.clone(), idx);
                    if let Ok(pp) = self.stage.prim(proto.as_str()) {
                        for c in pp.children().unwrap_or_default() {
                            if let Some(cn) = self.node(&c, depth + 1) {
                                self.scene.defs[idx].children.push(cn);
                            }
                        }
                    }
                    idx
                }
            };
            n.children.push(NodeIn { def: Some(d), ..Default::default() });
            return Some(n);
        }
        for c in p.children().unwrap_or_default() {
            if let Some(cn) = self.node(&c, depth + 1) {
                n.children.push(cn);
            }
        }
        if n.meshes.is_empty() && n.children.is_empty() {
            return None;
        }
        Some(n)
    }
}

/// The resolver of the layers a USD file composes (sublayers, references, payloads, clips):
/// openusd's own, except that it never goes to a network or device path the file names (a
/// sublayer `@\\serveur\x.usda@` made Windows send the user's NTLM hash), unless it is in the
/// folder of the file the user opened, and never looks for a bare name in the current folder.
struct LayerResolver {
    inner: openusd::ar::DefaultResolver,
    /// The folder of the file opened (the user chose it, even on a network share), as given
    /// and canonical, lower case with `\`.
    roots: Vec<String>,
}

impl LayerResolver {
    fn new(file: &Path) -> LayerResolver {
        let norm = |s: &str| {
            let mut r = esq_core::untrusted::strip_verbatim(s).replace('/', "\\").to_lowercase();
            if !r.ends_with('\\') {
                r.push('\\');
            }
            r
        };
        let dir = file.parent().filter(|d| !d.as_os_str().is_empty()).map(Path::to_path_buf).unwrap_or_default();
        let mut roots = vec![norm(&dir.to_string_lossy())];
        if let Ok(c) = std::fs::canonicalize(&dir) {
            roots.push(norm(&c.to_string_lossy()));
        }
        LayerResolver { inner: openusd::ar::DefaultResolver::new(), roots }
    }

    fn allowed(&self, path: &str) -> bool {
        let outer = path.split('[').next().unwrap_or(path);
        let p = esq_core::untrusted::strip_verbatim(outer);
        let low = p.replace('/', "\\").to_lowercase();
        let ok = !esq_core::untrusted::is_remote(p) || self.roots.iter().any(|r| r.len() > 2 && low.starts_with(r));
        if !ok {
            esq_core::untrusted::note_refused(path);
        }
        ok
    }
}

impl openusd::ar::Resolver for LayerResolver {
    fn create_identifier(&self, asset_path: &str, anchor: Option<&openusd::ar::ResolvedPath>) -> String {
        if !self.allowed(asset_path) {
            // Kept as written: the default resolver would look at it on disk to canonicalize it.
            return asset_path.to_string();
        }
        self.inner.create_identifier(asset_path, anchor)
    }

    fn resolve(&self, asset_path: &str) -> Option<openusd::ar::ResolvedPath> {
        let outer = asset_path.split('[').next().unwrap_or(asset_path);
        // A bare name that no layer anchored: the default resolver would search the current folder.
        if !self.allowed(asset_path) || !Path::new(esq_core::untrusted::strip_verbatim(outer)).is_absolute() {
            return None;
        }
        let r = self.inner.resolve(asset_path)?;
        self.allowed(&r.to_string_lossy()).then_some(r)
    }

    fn resolve_for_new_asset(&self, asset_path: &str) -> Option<openusd::ar::ResolvedPath> {
        if !self.allowed(asset_path) {
            return None;
        }
        self.inner.resolve_for_new_asset(asset_path)
    }

    fn open_asset(&self, resolved: &openusd::ar::ResolvedPath) -> std::io::Result<Box<dyn openusd::ar::Asset>> {
        if !self.allowed(&resolved.to_string_lossy()) {
            return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "chemin réseau refusé"));
        }
        self.inner.open_asset(resolved)
    }

    fn is_context_dependent_path(&self, asset_path: &str) -> bool {
        self.inner.is_context_dependent_path(asset_path)
    }

    fn in_memory_identifier<'a>(&self, asset_path: &'a str) -> std::borrow::Cow<'a, str> {
        self.inner.in_memory_identifier(asset_path)
    }

    fn get_modification_timestamp(
        &self,
        asset_path: &str,
        resolved: &openusd::ar::ResolvedPath,
    ) -> Option<std::time::SystemTime> {
        if !self.allowed(&resolved.to_string_lossy()) {
            return None;
        }
        self.inner.get_modification_timestamp(asset_path, resolved)
    }

    fn identity(&self) -> String {
        self.inner.identity()
    }

    fn cache_id(&self) -> Option<u64> {
        self.inner.cache_id()
    }
}

/// Reads a USD stage: the scene, millimetres per stage unit, and whether Y is up.
pub fn read(path: &Path) -> Result<(SceneIn, f64, bool)> {
    let p = path.to_str().context("chemin de fichier invalide")?;
    let resolver = LayerResolver::new(path);
    let stage = usd::Stage::builder().resolver(resolver).open(p).map_err(|e| anyhow!("fichier USD illisible : {e}"))?;
    let mpu = num(stage.stage_metadata("metersPerUnit").ok().flatten()).filter(|x| *x > 0.0).unwrap_or(0.01);
    let up = token(stage.stage_metadata("upAxis").ok().flatten()).unwrap_or_else(|| "Y".into());
    let package = if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("usdz")) {
        crate::zipfile::read(&std::fs::read(path)?).unwrap_or_default()
    } else {
        Vec::new()
    };
    let mut r = Reader {
        stage: &stage,
        scene: SceneIn::default(),
        mats: HashMap::new(),
        protos: HashMap::new(),
        package,
        file: path,
    };
    let roots = stage.root_prims().map_err(|e| anyhow!("{e}"))?;
    for name in roots {
        if let Ok(prim) = stage.prim(format!("/{}", name.as_str()).as_str())
            && let Some(n) = r.node(&prim, 0)
        {
            r.scene.root.children.push(n);
        }
    }
    if r.scene.meshes.is_empty() {
        bail!("aucune maille dans le fichier USD");
    }
    Ok((r.scene, mpu * 1000.0, up != "Z"))
}

/// File › Import of a USD file.
pub fn import(path: &Path, _o: &UsdzImport) -> Result<Model> {
    let (scene, mm, swap) = read(path)?;
    Ok(crate::build::build(&scene, BuildOpts { scale: mm, swap_yz: swap, merge_coplanar: true, orient_faces: false }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use esq_core::model::{Definition, Instance};

    #[test]
    fn usdz_round_trip() {
        let mut m = Model::new();
        let mut e = esq_core::model::Entities::default();
        let v: Vec<_> = [(0.0, 0.0), (1000.0, 0.0), (1000.0, 1000.0), (0.0, 1000.0)]
            .iter()
            .map(|(x, y)| e.add_vertex_raw(DVec3::new(*x, *y, 0.0)))
            .collect();
        let f = e.add_face_raw(v, vec![]).unwrap();
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(2, 2, image::Rgb([90, 60, 30])))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let wood = m.add_material("Bois clair", [90, 60, 30]);
        m.materials[wood].ext.texture = crate::tex::texture_from_bytes("bois.png", png, Some([300.0, 300.0]));
        e.faces[f].front = Some(wood);
        let d = m.defs.insert(Definition {
            name: "Plateau".into(),
            is_group: false,
            description: String::new(),
            entities: e,
            attributes: Default::default(),
            ext: Default::default(),
        });
        m.root.instances.insert(Instance {
            def: d,
            xform: DMat4::from_translation(DVec3::new(0.0, 0.0, 750.0)),
            name: String::new(),
            material: None,
            layer: 0,
            hidden: false,
            locked: false,
            attributes: Default::default(),
            ext: Default::default(),
        });
        let (text, tex) = usda(&m);
        assert!(text.starts_with("#usda 1.0") && text.contains("upAxis = \"Y\""));
        assert!(text.contains("def Xform \"Plateau\""));
        assert_eq!(tex.len(), 1);
        let dir = std::env::temp_dir().join(format!("esq_usd_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("plateau.usdz");
        export_usdz(&m, &p).unwrap();
        let entries = crate::zipfile::read(&std::fs::read(&p).unwrap()).unwrap();
        assert_eq!(entries[0].name, "model.usdc");
        assert!(entries[0].data.starts_with(b"PXR-USDC"));
        let back = import(&p, &UsdzImport::default()).unwrap();
        let mut faces = Vec::new();
        esq_core::walk::visit(&back, |pl| {
            for f in pl.ents.faces.values() {
                let z = pl.xform.transform_point3(pl.ents.verts[f.outer[0]].pos).z;
                faces.push((z, f.front.map(|m| back.materials[m].name.clone()), f.tex.is_some()));
            }
        });
        assert_eq!(faces.len(), 1, "{faces:?}");
        assert!((faces[0].0 - 750.0).abs() < 1e-3, "{faces:?}");
        assert_eq!(faces[0].1.as_deref(), Some("Bois_clair"));
        assert!(faces[0].2);
        let mat = back.materials.values().find(|m| m.name == "Bois_clair").unwrap();
        assert_eq!(mat.ext.texture.as_ref().unwrap().px, [2, 2]);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A USD layer whose sublayer, reference and payload are network paths: openusd's
    /// default resolver looked at each on disk (`exists`, `canonicalize`), and Windows sent the
    /// user's NTLM hash to the server they name. They are refused without any access, and the
    /// local geometry still imports.
    #[test]
    fn network_layers_are_never_opened() {
        let id = std::process::id();
        let (a, b, c) = (
            format!(r"\\esq-test-usd-{id}\partage\couche.usda"),
            format!("//esq-test-usd2-{id}/partage/ref.usda"),
            format!(r"\\?\UNC\esq-test-usd3-{id}\partage\charge.usda"),
        );
        let text = format!(
            "#usda 1.0\n(\n    subLayers = [@{a}@]\n    metersPerUnit = 1\n    upAxis = \"Z\"\n)\n\n\
             def Xform \"Groupe\" (\n    references = @{b}@\n    payload = @{c}@\n)\n{{\n\
             def Mesh \"Carre\"\n{{\n    point3f[] points = [(0, 0, 0), (1, 0, 0), (1, 1, 0), (0, 1, 0)]\n\
             int[] faceVertexCounts = [4]\n    int[] faceVertexIndices = [0, 1, 2, 3]\n}}\n}}\n"
        );
        let dir = std::env::temp_dir().join(format!("esq_usd_unc_{id}"));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("piege.usda");
        std::fs::write(&p, text).unwrap();
        let m = import(&p, &UsdzImport::default()).unwrap();
        let mut n = 0;
        esq_core::walk::visit(&m, |pl| n += pl.ents.faces.len());
        assert_eq!(n, 1);
        let refused = esq_core::untrusted::refused_paths();
        for unc in [&a, &b, &c] {
            assert!(refused.iter().any(|r| r == unc), "{unc} : {refused:?}");
        }
        // A bare name no layer anchored is not looked for in the current folder.
        let r = LayerResolver::new(&p);
        assert!(openusd::ar::Resolver::resolve(&r, "esq_nulle_part.usda").is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
