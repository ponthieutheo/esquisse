//! Google Earth (.kmz): a ZIP holding `doc.kml` (where the model stands on Earth) and
//! `models/untitled.dae` with its textures, as SketchUp's KMZ exporter writes it; the importer
//! reads every `<Model>` of the KML and its COLLADA file from the package.

use std::path::Path;

use anyhow::{Context, Result, bail};
use esq_core::glam::{DMat4, DVec3};
use esq_core::model::Model;

use crate::build::{BuildOpts, NodeIn, SceneIn};
use crate::dae::ImageSource;
use crate::options::{DaeExport, KmzExport, KmzImport};
use crate::xml::{Xml, num};
use crate::zipfile::{self, Entry};

/// Mean Earth radius (m), for placing several models relative to each other.
const EARTH_R: f64 = 6_371_000.0;

/// File › Export › 3D Model › KMZ.
pub fn export(model: &Model, path: &Path, o: &KmzExport, cx: &crate::ExportCx) -> Result<()> {
    let dae_opts = DaeExport { hidden: o.hidden, credits: o.credits, selection_only: false, ..DaeExport::default() };
    // Textures go next to the document, inside the package.
    let (doc, extras) = crate::dae::write(model, &dae_opts, cx, "untitled");
    let s = &model.shadow;
    let name = if cx.name.is_empty() { "SketchUp".to_string() } else { cx.name.clone() };
    let mut k = Xml::new("<?xml version=\"1.0\" encoding=\"UTF-8\"?>", "\t");
    k.open("kml", &[("xmlns", "http://www.opengis.net/kml/2.2"), ("xmlns:gx", "http://www.google.com/kml/ext/2.2")]);
    k.open("Folder", &[]);
    k.text("name", &[], &name);
    k.text("description", &[], "Created with <a href=\"http://sketchup.com\">SketchUp</a>");
    k.text("visibility", &[], "1");
    k.open("Placemark", &[]);
    k.text("name", &[], "Model");
    k.open("LookAt", &[]);
    k.text("longitude", &[], &num(s.longitude));
    k.text("latitude", &[], &num(s.latitude));
    k.text("altitude", &[], "0");
    k.text("heading", &[], &num(s.north_angle));
    k.text("tilt", &[], "60");
    let size = crate::build::bounds(model).map_or(100.0, |(lo, hi)| (hi - lo).length() / 1000.0);
    k.text("range", &[], &num((size * 2.0).max(10.0)));
    k.text("altitudeMode", &[], "relativeToGround");
    k.close();
    k.open("Model", &[("id", "model")]);
    k.text("altitudeMode", &[], "relativeToGround");
    k.open("Location", &[]);
    k.text("longitude", &[], &num(s.longitude));
    k.text("latitude", &[], &num(s.latitude));
    k.text("altitude", &[], "0");
    k.close();
    k.open("Orientation", &[]);
    k.text("heading", &[], &num(s.north_angle));
    k.text("tilt", &[], "0");
    k.text("roll", &[], "0");
    k.close();
    k.open("Scale", &[]);
    for a in ["x", "y", "z"] {
        k.text(a, &[], "1");
    }
    k.close();
    k.open("Link", &[]);
    k.text("href", &[], "models/untitled.dae");
    k.close();
    k.close();
    k.close();
    let kml = k.finish();
    let mut z = zipfile::Writer::new();
    z.add("doc.kml", kml.as_bytes(), true, 0);
    z.add("models/untitled.dae", doc.as_bytes(), true, 0);
    for e in extras {
        z.add(&format!("models/{}", e.path), &e.data, false, 0);
    }
    std::fs::write(path, z.finish())?;
    Ok(())
}

/// Images of the package, found by their bare name.
struct InZip<'a>(&'a [Entry]);

impl ImageSource for InZip<'_> {
    fn image(&self, reference: &str) -> Option<(String, Vec<u8>)> {
        let name = crate::tex::safe_name(reference)?.to_lowercase();
        let e = self.0.iter().find(|e| e.name.rsplit('/').next().is_some_and(|n| n.to_lowercase() == name))?;
        Some((name, e.data.clone()))
    }
}

fn child<'a, 'i>(n: roxmltree::Node<'a, 'i>, tag: &str) -> Option<roxmltree::Node<'a, 'i>> {
    n.children().find(|c| c.is_element() && c.tag_name().name() == tag)
}

fn num_of(n: Option<roxmltree::Node>, tag: &str) -> f64 {
    n.and_then(|n| child(n, tag)).and_then(|c| c.text()).and_then(|t| t.trim().parse().ok()).unwrap_or(0.0)
}

/// File › Import of a KMZ file: every model of the KML, placed relative to the first one.
pub fn import(bytes: &[u8], o: &KmzImport) -> Result<(Model, Option<(f64, f64)>)> {
    let entries = zipfile::read(bytes).context("fichier KMZ illisible")?;
    let kml = entries
        .iter()
        .find(|e| e.name.to_lowercase().ends_with(".kml") && !e.name.contains('/'))
        .or_else(|| entries.iter().find(|e| e.name.to_lowercase().ends_with(".kml")));
    let mut models: Vec<(String, DVec3, f64, DVec3)> = Vec::new();
    if let Some(kml) = kml {
        let text = String::from_utf8_lossy(&kml.data).into_owned();
        let doc = roxmltree::Document::parse(&text).context("doc.kml illisible")?;
        for m in doc.descendants().filter(|n| n.is_element() && n.tag_name().name() == "Model") {
            let Some(href) = child(m, "Link").and_then(|l| child(l, "href")).and_then(|h| h.text()) else { continue };
            let loc = child(m, "Location");
            let pos = DVec3::new(num_of(loc, "longitude"), num_of(loc, "latitude"), num_of(loc, "altitude"));
            let heading = num_of(child(m, "Orientation"), "heading");
            let sc = child(m, "Scale");
            let mut scale = DVec3::new(num_of(sc, "x"), num_of(sc, "y"), num_of(sc, "z"));
            if scale == DVec3::ZERO {
                scale = DVec3::ONE;
            }
            models.push((href.trim().to_string(), pos, heading, scale));
        }
    }
    if models.is_empty() {
        // A package without KML: its COLLADA files.
        for e in entries.iter().filter(|e| e.name.to_lowercase().ends_with(".dae")) {
            models.push((e.name.clone(), DVec3::ZERO, 0.0, DVec3::ONE));
        }
    }
    if models.is_empty() {
        bail!("aucun modèle dans le fichier KMZ");
    }
    let images = InZip(&entries);
    let mut scene = SceneIn::default();
    let origin = models[0].1;
    for (href, pos, heading, scale) in &models {
        // The link is a path inside the package; only its name is trusted.
        let want = crate::tex::safe_name(href).unwrap_or_default().to_lowercase();
        let Some(e) = entries.iter().find(|e| e.name.rsplit('/').next().is_some_and(|n| n.to_lowercase() == want))
        else {
            continue;
        };
        let text = String::from_utf8_lossy(&e.data).into_owned();
        let (mut sub, mm, swap) = match crate::dae::read(&text, &images, o.0.validate) {
            Ok(x) => x,
            Err(err) if o.0.validate => {
                bail!("Impossible de valider le fichier .kmz que vous tentez d'importer. ({err:#})")
            }
            Err(err) => return Err(err),
        };
        // Where this model stands relative to the first (metres east, north, up).
        let lat0 = origin.y.to_radians();
        let east = (pos.x - origin.x).to_radians() * EARTH_R * lat0.cos();
        let north = (pos.y - origin.y).to_radians() * EARTH_R;
        let up = pos.z - origin.z;
        sub.rescale(mm);
        let place = DMat4::from_translation(DVec3::new(east, north, up) * 1000.0)
            * DMat4::from_rotation_z(-heading.to_radians())
            * DMat4::from_scale(*scale)
            * if swap { crate::build::y_up_to_z_up() } else { DMat4::IDENTITY };
        // Merge the sub-scene into the KMZ scene.
        let base_mesh = scene.meshes.len();
        let base_mat = scene.materials.len() as u32;
        let base_def = scene.defs.len();
        scene.materials.extend(sub.materials);
        for mut m in sub.meshes {
            for p in &mut m.polys {
                p.mat = p.mat.map(|x| x + base_mat);
                p.back = p.back.map(|x| x + base_mat);
            }
            scene.meshes.push(m);
        }
        fn shift(n: &mut NodeIn, bm: usize, bd: usize, bmat: u32) {
            n.meshes.iter_mut().for_each(|m| *m += bm);
            n.def = n.def.map(|d| d + bd);
            n.material = n.material.map(|x| x + bmat);
            n.children.iter_mut().for_each(|c| shift(c, bm, bd, bmat));
        }
        let mut root = sub.root;
        shift(&mut root, base_mesh, base_def, base_mat);
        for mut d in sub.defs {
            d.meshes.iter_mut().for_each(|m| *m += base_mesh);
            d.children.iter_mut().for_each(|c| shift(c, base_mesh, base_def, base_mat));
            scene.defs.push(d);
        }
        root.xform = place * root.xform;
        root.kind = crate::build::NodeKind::Flatten;
        scene.root.children.push(root);
    }
    let model = crate::build::build(
        &scene,
        BuildOpts { scale: 1.0, swap_yz: false, merge_coplanar: o.0.merge_coplanar, orient_faces: false },
    );
    Ok((model, Some((origin.y, origin.x))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn textured_square() -> Model {
        let mut m = Model::new();
        let v: Vec<_> = [(0.0, 0.0), (2000.0, 0.0), (2000.0, 1000.0), (0.0, 1000.0)]
            .iter()
            .map(|(x, y)| m.root.add_vertex_raw(DVec3::new(*x, *y, 0.0)))
            .collect();
        let f = m.root.add_face_raw(v, vec![]).unwrap();
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(4, 4, image::Rgb([200, 100, 50])))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let mat = m.add_material("Brique", [200, 100, 50]);
        m.materials[mat].ext.texture = crate::tex::texture_from_bytes("brique.png", png, Some([500.0, 500.0]));
        m.root.faces[f].front = Some(mat);
        m.shadow.latitude = 48.8584;
        m.shadow.longitude = 2.2945;
        m
    }

    #[test]
    fn round_trip_through_google_earth_package() {
        let dir = std::env::temp_dir().join(format!("esq_kmz_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("tour.kmz");
        let m = textured_square();
        export(&m, &p, &KmzExport::default(), &crate::ExportCx { view: None, name: "tour".into() }).unwrap();
        let bytes = std::fs::read(&p).unwrap();
        let entries = zipfile::read(&bytes).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"doc.kml") && names.contains(&"models/untitled.dae"), "{names:?}");
        assert!(names.contains(&"models/untitled/brique.png"), "{names:?}");
        let kml = String::from_utf8(zipfile::find(&entries, "doc.kml").unwrap().data.clone()).unwrap();
        assert!(kml.contains("<latitude>48.8584</latitude>") && kml.contains("<href>models/untitled.dae</href>"));
        let (back, at) = import(&bytes, &KmzImport::default()).unwrap();
        assert_eq!(at, Some((48.8584, 2.2945)));
        let mut faces = Vec::new();
        esq_core::walk::visit(&back, |pl| faces.extend(pl.ents.faces.values().cloned()));
        assert_eq!(faces.len(), 1);
        let mat = &back.materials[faces[0].front.unwrap()];
        assert_eq!(mat.ext.texture.as_ref().unwrap().px, [4, 4]);
        assert!(faces[0].tex.as_ref().and_then(|t| t.front).is_some());
        std::fs::remove_dir_all(&dir).ok();
    }
}
