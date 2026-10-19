//! Materials beyond a colour (textures, colorization, PBR maps) and the positioning of
//! textures on faces.
//!
//! Measured with the SDK: `SUTextureCreateFromFile` and `SUTextureSetDimensions` take the size
//! of one tile in inches, `SUTextureGetDimensions` returns its inverse (tiles per inch).
//! `SUUVHelperGetFrontUVQ` returns homogeneous coordinates, linear in the point (q ≠ 1 for a
//! distorted texture), in tiles.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use esq_core::glam::{DVec2, DVec3};
use esq_core::meta::{FaceTex, MaterialExt, TextureData, UvMap, base_name, face_axes};
use esq_core::photo::Blob;

use super::*;

/// Files handed to and from the SDK go through this folder of the temporary directory.
fn scratch_file(ext: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("esq-skp-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir.join(format!("t{}.{ext}", N.fetch_add(1, Ordering::Relaxed)))
}

/// The extension that matches the bytes of an image file.
pub(crate) fn image_ext(bytes: &[u8], name: &str) -> String {
    if bytes.starts_with(b"\x89PNG") {
        "png".into()
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        "jpg".into()
    } else if bytes.starts_with(b"BM") {
        "bmp".into()
    } else if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        "tif".into()
    } else if bytes.starts_with(b"8BPS") {
        "psd".into()
    } else {
        let e = Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or("png").to_ascii_lowercase();
        if e.chars().all(|c| c.is_ascii_alphanumeric()) && !e.is_empty() { e } else { "png".into() }
    }
}

/// A texture of the file: its original image (as SketchUp keeps it), name, pixels and size.
pub(crate) fn read_texture(a: &Api, tex: Ref) -> Option<TextureData> {
    if tex.is_null() {
        return None;
    }
    let filename = base_name(&read_string(a, |s| unsafe { (a.SUTextureGetFileName)(tex, s) }));
    let (mut w, mut h, mut s, mut t) = (0usize, 0usize, 0.0, 0.0);
    unsafe { (a.SUTextureGetDimensions)(tex, &mut w, &mut h, &mut s, &mut t) };
    // The original file as the model keeps it: `SUTextureWriteOriginalToFile` converts the image
    // to the format of the path's extension when it knows that format (a PNG named `x.jpg`
    // becomes a new JPEG, a TIFF named `clock.gif` a GIF), and writes the stored bytes for an
    // extension it does not know.
    let mut path = scratch_file("bin");
    let p = cstr(&path.to_string_lossy());
    if unsafe { (a.SUTextureWriteOriginalToFile)(tex, p.as_ptr()) } != 0 {
        path = scratch_file("png");
        let p = cstr(&path.to_string_lossy());
        if unsafe { (a.SUTextureWriteToFile)(tex, p.as_ptr()) } != 0 {
            return None;
        }
    }
    let bytes = std::fs::read(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    if bytes.is_empty() {
        return None;
    }
    let mut avg = Color::default();
    unsafe { (a.SUTextureGetAverageColor)(tex, &mut avg) };
    let size = |k: f64| if k > 0.0 { IN / k } else { IN };
    Some(TextureData {
        filename,
        image: Blob(Arc::new(bytes)),
        px: [w as u32, h as u32],
        size: [size(s), size(t)],
        avg: [avg.r, avg.g, avg.b],
    })
}

/// A new texture (to attach to a material) from Esquisse's.
pub(crate) fn write_texture(a: &Api, t: &TextureData) -> Result<Ref> {
    let name = base_name(&t.filename);
    let path = scratch_file(&image_ext(&t.image.0, &name));
    std::fs::write(&path, t.image.0.as_slice())?;
    let p = cstr(&path.to_string_lossy());
    let mut r = Ref::NULL;
    let (w, h) = (t.size[0].max(1e-6) / IN, t.size[1].max(1e-6) / IN);
    let res = su!(a.SUTextureCreateFromFile(&mut r, p.as_ptr(), w, h));
    let _ = std::fs::remove_file(&path);
    res?;
    let n = cstr(&name);
    su!(a.SUTextureSetFileName(r, n.as_ptr()))?;
    Ok(r)
}

/// `SUMaterialType`.
const COLORED: i32 = 0;
const TEXTURED: i32 = 1;
const COLORIZED: i32 = 2;

/// Texture, colorization, opacity use and PBR settings of a material.
pub(crate) fn read_material_ext(a: &Api, mat: Ref, opacity: &mut f32) -> MaterialExt {
    let mut ext = MaterialExt::default();
    let mut tex = Ref::NULL;
    if unsafe { (a.SUMaterialGetTexture)(mat, &mut tex) } == 0 {
        ext.texture = read_texture(a, tex);
    }
    let mut ty = COLORED;
    unsafe { (a.SUMaterialGetType)(mat, &mut ty) };
    if ty == COLORIZED {
        let mut ct = 0;
        unsafe { (a.SUMaterialGetColorizeType)(mat, &mut ct) };
        ext.colorize_type = ct.clamp(0, 1) as u8;
        let (mut hh, mut ss, mut ll) = (0.0, 0.0, 0.0);
        unsafe { (a.SUMaterialGetColorizeDeltas)(mat, &mut hh, &mut ss, &mut ll) };
        ext.colorize_deltas = [hh, ss, ll];
    }
    let mut use_opacity = true;
    unsafe { (a.SUMaterialGetUseOpacity)(mat, &mut use_opacity) };
    if !use_opacity {
        *opacity = 1.0;
    }
    let mut wf = 0;
    unsafe { (a.SUMaterialGetWorkflow)(mat, &mut wf) };
    ext.workflow = wf.clamp(0, 1) as u8;
    let p = &mut ext.pbr;
    let get_tex = |f: unsafe extern "C" fn(Ref, *mut Ref) -> R| {
        let mut t = Ref::NULL;
        if unsafe { f(mat, &mut t) } == 0 { read_texture(a, t) } else { None }
    };
    unsafe {
        (a.SUMaterialIsMetalnessEnabled)(mat, &mut p.metalness_enabled);
        (a.SUMaterialGetMetallicFactor)(mat, &mut p.metallic_factor);
        (a.SUMaterialIsRoughnessEnabled)(mat, &mut p.roughness_enabled);
        (a.SUMaterialGetRoughnessFactor)(mat, &mut p.roughness_factor);
        (a.SUMaterialIsRoughnessTextureInverted)(mat, &mut p.roughness_inverted);
        (a.SUMaterialIsNormalEnabled)(mat, &mut p.normal_enabled);
        (a.SUMaterialGetNormalScale)(mat, &mut p.normal_scale);
        let mut style = 0;
        (a.SUMaterialGetNormalStyle)(mat, &mut style);
        p.normal_style = style.clamp(0, 1) as u8;
        (a.SUMaterialIsAOEnabled)(mat, &mut p.ao_enabled);
        (a.SUMaterialGetAOStrength)(mat, &mut p.ao_strength);
    }
    p.metallic_texture = get_tex(a.SUMaterialGetMetallicTexture);
    p.roughness_texture = get_tex(a.SUMaterialGetRoughnessTexture);
    p.normal_texture = get_tex(a.SUMaterialGetNormalTexture);
    p.ao_texture = get_tex(a.SUMaterialGetAOTexture);
    ext
}

/// Writes what [`read_material_ext`] reads.
pub(crate) fn write_material_ext(a: &Api, mat: Ref, ext: &MaterialExt, opacity: f32) -> Result<()> {
    su!(a.SUMaterialSetUseOpacity(mat, opacity < 1.0))?;
    if let Some(t) = &ext.texture {
        let tr = write_texture(a, t)?;
        su!(a.SUMaterialSetTexture(mat, tr))?;
    }
    let p = &ext.pbr;
    if ext.workflow == 1 || p.metalness_enabled || p.roughness_enabled || p.normal_enabled || p.ao_enabled {
        let set_tex = |f: unsafe extern "C" fn(Ref, Ref) -> R, t: &Option<TextureData>| -> Result<()> {
            if let Some(t) = t {
                let tr = write_texture(a, t)?;
                check(unsafe { f(mat, tr) }, "SUMaterialSet…Texture")?;
            }
            Ok(())
        };
        // Maps first: normal and occlusion maps cannot be turned on without their image.
        set_tex(a.SUMaterialSetMetallicTexture, &p.metallic_texture)?;
        set_tex(a.SUMaterialSetRoughnessTexture, &p.roughness_texture)?;
        set_tex(a.SUMaterialSetNormalTexture, &p.normal_texture)?;
        set_tex(a.SUMaterialSetAOTexture, &p.ao_texture)?;
        su!(a.SUMaterialSetMetalnessEnabled(mat, p.metalness_enabled))?;
        su!(a.SUMaterialSetMetallicFactor(mat, p.metallic_factor))?;
        su!(a.SUMaterialSetRoughnessEnabled(mat, p.roughness_enabled))?;
        su!(a.SUMaterialSetRoughnessFactor(mat, p.roughness_factor))?;
        su!(a.SUMaterialSetRoughnessTextureInverted(mat, p.roughness_inverted))?;
        if p.normal_texture.is_some() {
            su!(a.SUMaterialSetNormalEnabled(mat, p.normal_enabled))?;
            su!(a.SUMaterialSetNormalScale(mat, p.normal_scale))?;
            su!(a.SUMaterialSetNormalStyle(mat, p.normal_style as i32))?;
        }
        if p.ao_texture.is_some() {
            su!(a.SUMaterialSetAOEnabled(mat, p.ao_enabled))?;
            su!(a.SUMaterialSetAOStrength(mat, p.ao_strength))?;
        }
    }
    Ok(())
}

/// Plain texture, or colorized by the material's colour (`SUMaterialType`).
pub(crate) fn write_material_type(a: &Api, mat: Ref, ext: &MaterialExt) -> Result<()> {
    if ext.texture.is_none() {
        return Ok(());
    }
    if ext.colorize_deltas != [0.0; 3] {
        su!(a.SUMaterialSetType(mat, COLORIZED))?;
        su!(a.SUMaterialSetColorizeType(mat, ext.colorize_type as i32))?;
    } else {
        su!(a.SUMaterialSetType(mat, TEXTURED))?;
    }
    Ok(())
}

/// How a textured face is mapped, when it is not SketchUp's default mapping: the map on
/// each side (millimetres, in the face's collection), and the projection of a projected
/// texture. `points` are the face's points (millimetres).
pub(crate) fn read_face_tex(a: &Api, f: Ref, sides: [bool; 2], points: &[DVec3]) -> Option<FaceTex> {
    let p0 = *points.first()?;
    let mut out = FaceTex::default();
    let mut uvh = Ref::NULL;
    let mut any = false;
    for (i, front) in [true, false].into_iter().enumerate() {
        if !sides[i] {
            continue;
        }
        let (mut positioned, mut projected) = (false, false);
        unsafe {
            (a.SUFaceIsTexturePositioned)(f, front, &mut positioned);
            (a.SUFaceIsTextureProjected)(f, front, &mut projected);
        }
        if !positioned && !projected {
            continue;
        }
        if uvh.is_null() && unsafe { (a.SUFaceGetUVHelper)(f, true, true, Ref::NULL, &mut uvh) } != 0 {
            return None;
        }
        // The helper's u, v, q are affine functions of the point, off the face too (a
        // projected texture keeps its coordinates along the projection): four points that are
        // not on a plane give them exactly.
        const L: f64 = 1000.0;
        let sample = |p: DVec3| {
            let pt = P3 { x: p.x / IN, y: p.y / IN, z: p.z / IN };
            let mut q = Uvq::default();
            unsafe {
                if front {
                    (a.SUUVHelperGetFrontUVQ)(uvh, &pt, &mut q);
                } else {
                    (a.SUUVHelperGetBackUVQ)(uvh, &pt, &mut q);
                }
            }
            DVec3::new(q.u, q.v, q.q)
        };
        let s = [sample(p0), sample(p0 + DVec3::X * L), sample(p0 + DVec3::Y * L), sample(p0 + DVec3::Z * L)];
        let w = if s[0].z.abs() > 1e-12 { s[0].z } else { 1.0 };
        let s = s.map(|x| x / w);
        let row = |c: [f64; 4]| {
            let g = DVec3::new(c[1] - c[0], c[2] - c[0], c[3] - c[0]) / L;
            [g.x, g.y, g.z, c[0] - g.dot(p0)]
        };
        let affine = s[1..].iter().all(|x| (x.z - 1.0).abs() < 1e-9);
        let map = UvMap {
            u: row(s.map(|x| x.x)),
            v: row(s.map(|x| x.y)),
            q: if affine { [0.0, 0.0, 0.0, 1.0] } else { row(s.map(|x| x.z)) },
        };
        if front {
            out.front = Some(map);
        } else {
            out.back = Some(map);
        }
        if projected {
            let mut d = P3::default();
            if unsafe { (a.SUFaceGetTextureProjection)(f, front, &mut d) } == 0 {
                let d = DVec3::new(d.x, d.y, d.z);
                out.projection[i] = (d.length() > 1e-12).then(|| d.normalize());
            }
        }
        any = true;
    }
    if !uvh.is_null() {
        unsafe { (a.SUUVHelperRelease)(&mut uvh) };
    }
    any.then_some(out)
}

/// The material input of one side of a face: its position when Esquisse has one, through
/// four points of the face's plane (or the pins it was positioned with).
pub(crate) fn position_input(
    mat: Ref,
    tex: Option<&FaceTex>,
    side: usize,
    points: &[DVec3],
    normal: DVec3,
) -> Option<MaterialPositionInput> {
    let t = tex?;
    let map = if side == 0 { t.front? } else { t.back? };
    let p0 = *points.first()?;
    let pins = &t.pins[side];
    let pairs: Vec<(DVec3, DVec2)> = if pins.len() == 4 && pins.iter().all(|(p, uv)| (map.at(*p) - *uv).length() < 1e-6)
    {
        pins.clone()
    } else {
        let (ex, ey) = face_axes(normal);
        // Large enough to be precise, on the face's scale.
        let size = points.iter().map(|p| p.distance(p0)).fold(0.0, f64::max).max(100.0);
        [p0, p0 + ex * size, p0 + ex * size + ey * size, p0 + ey * size].map(|p| (p, map.at(p))).to_vec()
    };
    let mut input = MaterialPositionInput {
        num_uv_coords: 4,
        uv_coords: [P2::default(); 4],
        points: [P3::default(); 4],
        material: mat,
        projection: P3::default(),
    };
    for (k, (p, uv)) in pairs.iter().enumerate().take(4) {
        input.points[k] = P3 { x: p.x / IN, y: p.y / IN, z: p.z / IN };
        input.uv_coords[k] = P2 { x: uv.x, y: uv.y };
    }
    if let Some(d) = t.projection[side] {
        input.projection = P3 { x: d.x, y: d.y, z: d.z };
    }
    Some(input)
}
