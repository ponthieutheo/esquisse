//! A second reader for the tests: SketchUp's own DWG importer (`Importers\dwgimporter.dll`,
//! built on the Open Design Alliance's libraries), when SketchUp 2026 is installed. Its
//! `ConvertToSketchUp(input, output .skp, progress, options)` reads the DWG strictly (a
//! damaged object makes it refuse the file) and saves a model that the SketchUp SDK
//! (`SketchUpAPI.dll`) reads back: edges, faces, components, tags.
//!
//! The importer throws C++ exceptions on the files it refuses, so it runs in a child
//! process: the test binary itself, on the `child` test, with the paths in the environment.

use std::ffi::{CString, c_char, c_void};
use std::path::{Path, PathBuf};

/// What SketchUp's importer made of a file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Import {
    /// Edges, faces, component instances, guide lines, guide points, arc curves of the model.
    pub model: Counts,
    /// Tags, in SketchUp's order, and whether they are visible.
    pub layers: Vec<(String, bool)>,
    /// Component definitions and what they hold.
    pub defs: Vec<(String, Counts)>,
    /// Bounding boxes of the model and of the definitions, transformations of the model's
    /// component instances (rounded to 1e-6).
    pub geometry: Vec<String>,
}

#[derive(Debug, Default, PartialEq, Clone, Copy)]
pub struct Counts {
    pub edges: usize,
    pub faces: usize,
    pub instances: usize,
    pub guide_lines: usize,
    pub guide_points: usize,
    pub arcs: usize,
}

/// SketchUp's folder: `ESQ_SKETCHUP_DIR`, else where its installer puts it.
fn root() -> Option<PathBuf> {
    let dir = match std::env::var_os("ESQ_SKETCHUP_DIR") {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(std::env::var_os("ProgramFiles")?).join(r"SketchUp\SketchUp 2026\SketchUp"),
    };
    (dir.join("SketchUpAPI.dll").is_file() && dir.join(r"Importers\dwgimporter.dll").is_file()).then_some(dir)
}

/// Imports `data`, a file named `file` (.dwg or .dxf), with SketchUp's importer; `None`
/// when SketchUp is not installed, an error when the importer refuses the file.
pub fn import(data: &[u8], file: &str) -> Option<Result<Import, String>> {
    root()?;
    // One import at a time: two importers at once can make one of them fail.
    static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("esq_teigha_{}", std::process::id()));
    std::fs::create_dir_all(&dir).ok()?;
    let input = dir.join(file);
    let output = dir.join(format!("{file}.skp"));
    std::fs::write(&input, data).ok()?;
    let _ = std::fs::remove_file(&output);
    let out = std::process::Command::new(std::env::current_exe().ok()?)
        .args(["--exact", "dwg::teigha::child", "--nocapture", "--test-threads=1"])
        .env("ESQ_TEIGHA_IN", &input)
        .env("ESQ_TEIGHA_OUT", &output)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let line = text.lines().find_map(|l| l.find("TEIGHA ").map(|i| &l[i + 7..]));
    Some(match line {
        Some(json) => parse(json).ok_or_else(|| format!("réponse illisible : {json}")),
        None => Err(format!(
            "l'importateur de SketchUp refuse le fichier ({}) : {}",
            out.status,
            text.lines().rev().take(5).collect::<Vec<_>>().join(" | ")
        )),
    })
}

fn parse(s: &str) -> Option<Import> {
    let v: serde_json::Value = serde_json::from_str(s).ok()?;
    let counts = |c: &serde_json::Value| -> Option<Counts> {
        let a: Vec<usize> = c.as_array()?.iter().map(|x| x.as_u64().unwrap_or(0) as usize).collect();
        Some(Counts { edges: a[0], faces: a[1], instances: a[2], guide_lines: a[3], guide_points: a[4], arcs: a[5] })
    };
    Some(Import {
        model: counts(&v["model"])?,
        layers: v["layers"]
            .as_array()?
            .iter()
            .map(|x| (x[0].as_str().unwrap_or("").to_string(), x[1].as_bool().unwrap_or(true)))
            .collect(),
        defs: v["defs"].as_array()?.iter().filter_map(|d| Some((d[0].as_str()?.to_string(), counts(&d[1])?))).collect(),
        geometry: v["geometry"].as_array()?.iter().filter_map(|g| Some(g.as_str()?.to_string())).collect(),
    })
}

/// The importer's interface keeps `std::string`s (MSVC layout): a 16-byte buffer or a
/// pointer, the size, the capacity.
#[repr(C)]
struct MsvcString {
    buf: [u8; 16],
    size: usize,
    cap: usize,
}

impl MsvcString {
    fn new(s: &CString) -> MsvcString {
        let b = s.as_bytes();
        let mut m = MsvcString { buf: [0; 16], size: b.len(), cap: 15 };
        if b.len() < 16 {
            m.buf[..b.len()].copy_from_slice(b);
        } else {
            m.buf[..8].copy_from_slice(&(s.as_ptr() as usize).to_le_bytes());
            m.cap = b.len();
        }
        m
    }
}

type Ref = *mut c_void;

/// Runs in the child process: converts, then reads the .skp with the SDK.
fn run(dir: &Path, input: &str, output: &str) -> Result<String, String> {
    use libloading::os::windows::{LOAD_WITH_ALTERED_SEARCH_PATH, Library, Symbol};
    let path = std::env::var("PATH").unwrap_or_default();
    // SAFETY: the child process has a single test thread.
    unsafe { std::env::set_var("PATH", format!("{};{};{path}", dir.display(), dir.join("Importers").display())) };
    // SAFETY: SketchUp's own libraries, loaded with their dependencies from their folders.
    let api = unsafe { Library::load_with_flags(dir.join("SketchUpAPI.dll"), LOAD_WITH_ALTERED_SEARCH_PATH) }
        .map_err(|e| e.to_string())?;
    let imp =
        unsafe { Library::load_with_flags(dir.join(r"Importers\dwgimporter.dll"), LOAD_WITH_ALTERED_SEARCH_PATH) }
            .map_err(|e| e.to_string())?;
    // SAFETY: signatures read in dwgimporter.dll (vtable slot 7 is ConvertToSketchUp) and
    // in the SketchUp C API headers.
    unsafe {
        let get: Symbol<unsafe extern "C" fn() -> *mut c_void> =
            imp.get(b"GetSketchUpModelImporterInterface").map_err(|e| e.to_string())?;
        let iface = get();
        let vtable = *(iface as *const *const usize);
        type Convert = unsafe extern "system" fn(
            *mut c_void,
            *const MsvcString,
            *const MsvcString,
            *mut c_void,
            *const c_void,
        ) -> u8;
        let convert: Convert = std::mem::transmute(*vtable.add(7));
        let (i, o) = (CString::new(input).unwrap(), CString::new(output).unwrap());
        let ok = convert(iface, &MsvcString::new(&i), &MsvcString::new(&o), std::ptr::null_mut(), std::ptr::null());
        if ok == 0 {
            return Err("ConvertToSketchUp a échoué".into());
        }
        std::mem::forget(imp);

        let init: Symbol<unsafe extern "C" fn()> = api.get(b"SUInitialize").map_err(|e| e.to_string())?;
        init();
        let open: Symbol<unsafe extern "C" fn(*mut Ref, *const c_char) -> i32> =
            api.get(b"SUModelCreateFromFile").map_err(|e| e.to_string())?;
        let mut model: Ref = std::ptr::null_mut();
        let r = open(&mut model, o.as_ptr());
        if r != 0 {
            return Err(format!("SUModelCreateFromFile : {r}"));
        }
        let count_fn = |n: &[u8]| -> Symbol<unsafe extern "C" fn(Ref, *mut usize) -> i32> { api.get(n).unwrap() };
        let edges: Symbol<unsafe extern "C" fn(Ref, bool, *mut usize) -> i32> =
            api.get(b"SUEntitiesGetNumEdges").unwrap();
        let counts = |e: Ref| -> Vec<usize> {
            let mut out = Vec::new();
            let mut c = 0usize;
            edges(e, false, &mut c);
            out.push(c);
            for n in [
                &b"SUEntitiesGetNumFaces"[..],
                b"SUEntitiesGetNumInstances",
                b"SUEntitiesGetNumGuideLines",
                b"SUEntitiesGetNumGuidePoints",
                b"SUEntitiesGetNumArcCurves",
            ] {
                let mut c = 0usize;
                count_fn(n)(e, &mut c);
                out.push(c);
            }
            out
        };
        let name_of = |r: Ref, getter: &[u8]| -> String {
            let create: Symbol<unsafe extern "C" fn(*mut Ref) -> i32> = api.get(b"SUStringCreate").unwrap();
            let get_name: Symbol<unsafe extern "C" fn(Ref, *mut Ref) -> i32> = api.get(getter).unwrap();
            let len: Symbol<unsafe extern "C" fn(Ref, *mut usize) -> i32> = api.get(b"SUStringGetUTF8Length").unwrap();
            let utf8: Symbol<unsafe extern "C" fn(Ref, usize, *mut c_char, *mut usize) -> i32> =
                api.get(b"SUStringGetUTF8").unwrap();
            let release: Symbol<unsafe extern "C" fn(*mut Ref) -> i32> = api.get(b"SUStringRelease").unwrap();
            let mut s: Ref = std::ptr::null_mut();
            create(&mut s);
            get_name(r, &mut s);
            let mut l = 0;
            len(s, &mut l);
            let mut b = vec![0u8; l + 1];
            let mut c = 0;
            utf8(s, l + 1, b.as_mut_ptr() as *mut c_char, &mut c);
            release(&mut s);
            b.truncate(c);
            String::from_utf8_lossy(&b).into_owned()
        };
        let model_entities: Symbol<unsafe extern "C" fn(Ref, *mut Ref) -> i32> =
            api.get(b"SUModelGetEntities").unwrap();
        let mut ents: Ref = std::ptr::null_mut();
        model_entities(model, &mut ents);
        let mut n = 0usize;
        count_fn(b"SUModelGetNumLayers")(model, &mut n);
        let get_layers: Symbol<unsafe extern "C" fn(Ref, usize, *mut Ref, *mut usize) -> i32> =
            api.get(b"SUModelGetLayers").unwrap();
        let mut layers = vec![std::ptr::null_mut(); n];
        let mut got = 0;
        get_layers(model, n, layers.as_mut_ptr(), &mut got);
        let visibility: Symbol<unsafe extern "C" fn(Ref, *mut bool) -> i32> = api.get(b"SULayerGetVisibility").unwrap();
        let layer_names: Vec<serde_json::Value> = layers[..got]
            .iter()
            .map(|l| {
                let mut on = true;
                visibility(*l, &mut on);
                serde_json::json!([name_of(*l, b"SULayerGetName"), on])
            })
            .collect();
        let mut nd = 0usize;
        count_fn(b"SUModelGetNumComponentDefinitions")(model, &mut nd);
        let get_defs: Symbol<unsafe extern "C" fn(Ref, usize, *mut Ref, *mut usize) -> i32> =
            api.get(b"SUModelGetComponentDefinitions").unwrap();
        let mut defs = vec![std::ptr::null_mut(); nd];
        let mut got = 0;
        get_defs(model, nd, defs.as_mut_ptr(), &mut got);
        let def_entities: Symbol<unsafe extern "C" fn(Ref, *mut Ref) -> i32> =
            api.get(b"SUComponentDefinitionGetEntities").unwrap();
        let numbers = |xs: &[f64]| -> String {
            xs.iter().map(|x| format!("{:.6}", x + 0.0)).collect::<Vec<_>>().join(" ").replace("-0.000000", "0.000000")
        };
        let bbox: Symbol<unsafe extern "C" fn(Ref, *mut [f64; 6]) -> i32> =
            api.get(b"SUEntitiesGetBoundingBox").unwrap();
        let bounds = |e: Ref| -> String {
            let mut b = [0.0; 6];
            bbox(e, &mut b);
            numbers(&b)
        };
        let mut geometry = vec![format!("modèle {}", bounds(ents))];
        let defs: Vec<serde_json::Value> = defs[..got]
            .iter()
            .map(|d| {
                let mut e: Ref = std::ptr::null_mut();
                def_entities(*d, &mut e);
                let name = name_of(*d, b"SUComponentDefinitionGetName");
                geometry.push(format!("{name} {}", bounds(e)));
                serde_json::json!([name, counts(e)])
            })
            .collect();
        let mut ni = 0usize;
        count_fn(b"SUEntitiesGetNumInstances")(ents, &mut ni);
        let get_instances: Symbol<unsafe extern "C" fn(Ref, usize, *mut Ref, *mut usize) -> i32> =
            api.get(b"SUEntitiesGetInstances").unwrap();
        let transform: Symbol<unsafe extern "C" fn(Ref, *mut [f64; 16]) -> i32> =
            api.get(b"SUComponentInstanceGetTransform").unwrap();
        let mut instances = vec![std::ptr::null_mut(); ni];
        let mut got = 0;
        get_instances(ents, ni, instances.as_mut_ptr(), &mut got);
        let mut xforms: Vec<String> = instances[..got]
            .iter()
            .map(|i| {
                let mut t = [0.0; 16];
                transform(*i, &mut t);
                format!("instance {}", numbers(&t))
            })
            .collect();
        xforms.sort();
        geometry.extend(xforms);
        let json =
            serde_json::json!({ "model": counts(ents), "layers": layer_names, "defs": defs, "geometry": geometry });
        let release: Symbol<unsafe extern "C" fn(*mut Ref) -> i32> = api.get(b"SUModelRelease").unwrap();
        release(&mut model);
        Ok(json.to_string())
    }
}

#[test]
fn child() {
    let (Ok(input), Ok(output)) = (std::env::var("ESQ_TEIGHA_IN"), std::env::var("ESQ_TEIGHA_OUT")) else { return };
    let Some(dir) = root() else { return };
    match run(&dir, &input, &output) {
        Ok(json) => println!("TEIGHA {json}"),
        Err(e) => println!("ERREUR {e}"),
    }
}
