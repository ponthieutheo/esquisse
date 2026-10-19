//! Images (`SUImageRef`, `Sketchup::Image`). SketchUp's image is a definition of one inch per
//! pixel placed by a transformation (measured: `SUImageCreateFromFile` makes the identity, and
//! the size in inches equals the pixel count), as Esquisse's (`esq_core::image`).

use std::sync::Arc;

use esq_core::meta::{TextureData, base_name};
use esq_core::photo::Blob;

use super::*;

/// The picture of an image: the texture of its definition's face (the original file), or else
/// its pixels as a PNG.
fn picture(a: &Api, img: Ref, file: &str, px: [u32; 2]) -> Option<TextureData> {
    let mut def = Ref::NULL;
    if unsafe { (a.SUImageGetDefinition)(img, &mut def) } == 0 {
        let mut ents = Ref::NULL;
        unsafe { (a.SUComponentDefinitionGetEntities)(def, &mut ents) };
        for f in list(
            |n| unsafe { (a.SUEntitiesGetNumFaces)(ents, n) },
            |n, p, g| unsafe { (a.SUEntitiesGetFaces)(ents, n, p, g) },
        ) {
            let (mut mat, mut tex) = (Ref::NULL, Ref::NULL);
            if unsafe { (a.SUFaceGetFrontMaterial)(f, &mut mat) } == 0
                && unsafe { (a.SUMaterialGetTexture)(mat, &mut tex) } == 0
                && let Some(mut t) = textures::read_texture(a, tex)
            {
                t.px = px;
                if !file.is_empty() {
                    t.filename = file.to_string();
                }
                return Some(t);
            }
        }
    }
    let mut rep = Ref::NULL;
    unsafe { (a.SUImageRepCreate)(&mut rep) };
    let path = std::env::temp_dir().join(format!("esq-skp-{}-img.png", std::process::id()));
    let p = cstr(&path.to_string_lossy());
    let ok = unsafe { (a.SUImageGetImageRep)(img, &mut rep) } == 0
        && unsafe { (a.SUImageRepSaveToFile)(rep, p.as_ptr()) } == 0;
    unsafe { (a.SUImageRepRelease)(&mut rep) };
    let bytes = if ok { std::fs::read(&path).ok() } else { None };
    let _ = std::fs::remove_file(&path);
    Some(TextureData {
        filename: if file.is_empty() { "image.png".into() } else { file.to_string() },
        image: Blob(Arc::new(bytes?)),
        px,
        size: [px[0] as f64 * IN, px[1] as f64 * IN],
        avg: [128, 128, 128],
    })
}

/// The images of a collection, as image instances of `out`.
pub(crate) fn read_images(r: &mut Reader, out: &mut Entities, ents: Ref) -> Result<()> {
    use slotmap::Key;
    let a = r.a;
    for img in list(
        |n| unsafe { (a.SUEntitiesGetNumImages)(ents, n) },
        |n, p, g| unsafe { (a.SUEntitiesGetImages)(ents, n, p, g) },
    ) {
        let name = read_string(a, |s| unsafe { (a.SUImageGetName)(img, s) });
        let file = read_string(a, |s| unsafe { (a.SUImageGetFileName)(img, s) });
        let file = if file.trim().is_empty() { String::new() } else { base_name(&file) };
        let (mut pw, mut ph) = (0usize, 0usize);
        unsafe { (a.SUImageGetPixelDimensions)(img, &mut pw, &mut ph) };
        let Some(tex) = picture(a, img, &file, [pw as u32, ph as u32]) else { continue };
        let stem = Path::new(&file).file_stem().and_then(|s| s.to_str()).unwrap_or("Image").to_string();
        let Some(def) = r.model.add_image_def(&stem, &file, tex) else { continue };
        let mut x = Xf { v: [0.0; 16] };
        unsafe { (a.SUImageGetTransform)(img, &mut x) };
        let el = unsafe { (a.SUImageToDrawingElement)(img) };
        let id = out.instances.insert(Instance {
            def,
            xform: xf_to_mm(&x),
            name,
            material: None,
            layer: r.layer(el),
            hidden: r.hidden(el),
            locked: false,
            attributes: read_attrs(a, unsafe { (a.SUImageToEntity)(img) }),
            ext: Default::default(),
        });
        r.shadow_flags(out, 'i', id.data().as_ffi(), el);
    }
    Ok(())
}

/// Writes an image instance; returns its drawing element.
pub(crate) fn write_image(w: &Writer, target: Ref, inst: &Instance) -> Result<Option<Ref>> {
    let a = w.a;
    let Some(mat) = w.model.image_material(inst.def) else { return Ok(None) };
    let Some(tex) = w.model.materials[mat].ext.texture.as_ref() else { return Ok(None) };
    // The file keeps its name (SketchUp reports it as the image's file name).
    let dir = std::env::temp_dir().join(format!("esq-skp-{}-i{}", std::process::id(), w.images.get()));
    w.images.set(w.images.get() + 1);
    std::fs::create_dir_all(&dir)?;
    let mut name = base_name(&tex.filename);
    let ext = textures::image_ext(&tex.image.0, &name);
    if !name.to_ascii_lowercase().ends_with(&format!(".{ext}")) {
        name = format!("{}.{ext}", Path::new(&name).file_stem().and_then(|s| s.to_str()).unwrap_or("image"));
    }
    let path = dir.join(&name);
    std::fs::write(&path, tex.image.0.as_slice())?;
    let p = cstr(&path.to_string_lossy());
    let mut img = Ref::NULL;
    let res = su!(a.SUImageCreateFromFile(&mut img, p.as_ptr()));
    let _ = std::fs::remove_dir_all(&dir);
    res?;
    su!(a.SUEntitiesAddImage(target, img))?;
    su!(a.SUImageSetTransform(img, &xf_from_mm(&inst.xform)))?;
    if !inst.name.is_empty() {
        let n = cstr(&inst.name);
        su!(a.SUImageSetName(img, n.as_ptr()))?;
    }
    write_attrs(a, unsafe { (a.SUImageToEntity)(img) }, &inst.attributes)?;
    Ok(Some(unsafe { (a.SUImageToDrawingElement)(img) }))
}
