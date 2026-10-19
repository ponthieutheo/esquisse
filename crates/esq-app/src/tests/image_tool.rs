//! File › Import of a picture and the Image tool (`CImageTool`), driven like the mouse does.

use eframe::egui::Modifiers;
use esq_core::glam::{DVec2, DVec3};
use esq_core::model::EntityRef;

use super::Harness;
use crate::imgfile::LoadedImage;
use crate::tools::{ImageTool, ToolId};

/// A 200 × 100 pixels picture.
fn picture() -> LoadedImage {
    let bytes = crate::photo_img::tests::png_bytes(200, 100);
    let mut img = crate::imgfile::from_bytes("plan.png", bytes).unwrap();
    img.path = "C:\\plans\\plan.png".into();
    img
}

fn only_image(h: &Harness) -> esq_core::image::ImageView {
    let ents = &h.doc.model.root;
    let (id, _) = ents.instances.iter().find(|(_, i)| h.doc.model.is_image_def(i.def)).expect("an image");
    h.doc.model.image_of(None, id).unwrap()
}

#[test]
fn two_clicks_place_the_image_keeping_its_proportions() {
    let mut h = Harness::new();
    h.tool = Box::new(ImageTool::new(picture(), false));
    assert_eq!(h.tool.id(), ToolId::Image);
    h.hover(DVec3::new(100.0, 50.0, 0.0));
    assert!(h.doc.preview.is_some(), "the picture follows the cursor");
    assert!(h.doc.model.root.instances.is_empty());
    assert_eq!(h.tool.vcb_label(), "Dimensions");
    h.click(DVec3::new(0.0, 0.0, 0.0));
    assert_eq!(h.tool.vcb_label(), "Hauteur");
    h.hover(DVec3::new(1000.0, 200.0, 0.0));
    assert_eq!(h.tool.vcb_label(), "Largeur", "the width follows the cursor");
    h.click(DVec3::new(1000.0, 200.0, 0.0));
    let v = only_image(&h);
    assert!((v.width - 1000.0).abs() < 1.0 && (v.height - 500.0).abs() < 0.5, "{v:?}");
    assert!(v.origin.distance(DVec3::ZERO) < 1.0, "{v:?}");
    assert!(v.normal.distance(DVec3::Z) < 1e-9 && v.xaxis.distance(DVec3::X) < 1e-9);
    assert_eq!((v.px, v.path.as_str()), ([200, 100], "C:\\plans\\plan.png"));
    assert_eq!(h.tool.switch_to(), Some(ToolId::Select), "the tool gives way to Select");
    let id = h.doc.model.root.instances.keys().next().unwrap();
    assert_eq!(h.doc.sel, vec![EntityRef::Instance(id)], "the new image is selected");
    assert_eq!(h.doc.undo_label(), Some("Importer graphique 2D"));
    // The picture is the texture of the image's own material.
    let mat = &h.doc.model.materials[v.material.unwrap()];
    assert!(mat.ext.owner_image && mat.ext.texture.as_ref().unwrap().px == [200, 100]);
}

#[test]
fn shift_frees_the_proportions_and_ctrl_grows_from_the_centre() {
    let mut h = Harness::new();
    h.tool = Box::new(ImageTool::new(picture(), false));
    h.click(DVec3::new(500.0, 500.0, 0.0));
    h.mods = Modifiers::SHIFT;
    h.hover(DVec3::new(1300.0, 800.0, 0.0));
    h.click(DVec3::new(1300.0, 800.0, 0.0));
    let v = only_image(&h);
    assert!((v.width - 800.0).abs() < 1.0 && (v.height - 300.0).abs() < 1.0, "{v:?}");

    let mut h = Harness::new();
    h.tool = Box::new(ImageTool::new(picture(), false));
    h.click(DVec3::new(500.0, 500.0, 0.0));
    h.mods = Modifiers::CTRL;
    h.click(DVec3::new(900.0, 600.0, 0.0));
    let v = only_image(&h);
    assert!((v.width - 800.0).abs() < 1.0 && (v.height - 400.0).abs() < 1.0, "{v:?}");
    assert!(v.origin.distance(DVec3::new(100.0, 300.0, 0.0)) < 1.0, "centred on the first point: {v:?}");
}

#[test]
fn sizes_can_be_typed() {
    let mut h = Harness::new();
    h.tool = Box::new(ImageTool::new(picture(), false));
    h.hover(DVec3::new(0.0, 0.0, 0.0));
    // Before the first click, both values are needed.
    assert!(h.with(|t, cx| t.vcb(cx, "600")).is_err());
    h.click(DVec3::new(0.0, 0.0, 0.0));
    h.hover(DVec3::new(300.0, 900.0, 0.0));
    assert_eq!(h.tool.vcb_label(), "Hauteur");
    h.type_vcb("400");
    let v = only_image(&h);
    assert!((v.width - 800.0).abs() < 1e-6 && (v.height - 400.0).abs() < 1e-6, "{v:?}");

    let mut h = Harness::new();
    h.tool = Box::new(ImageTool::new(picture(), false));
    h.click(DVec3::new(0.0, 0.0, 0.0));
    h.type_vcb("1500;600");
    let v = only_image(&h);
    assert!((v.width - 1500.0).abs() < 1e-6 && (v.height - 600.0).abs() < 1e-6, "{v:?}");
    assert!(h.with(|t, cx| t.vcb(cx, "0;5")).is_err());
}

#[test]
fn a_double_click_drops_the_image_at_its_first_size() {
    let mut h = Harness::new();
    h.tool = Box::new(ImageTool::new(picture(), false));
    let p = DVec3::new(300.0, 100.0, 0.0);
    h.hover(p);
    let m = h.px(p);
    h.with(|t, cx| t.double_click(cx, m));
    let v = only_image(&h);
    // Half the smaller side of the view (400 px) at the distance of the point.
    let want = (400.0 * h.cam.pixel_size_at(p)).round();
    assert!((v.width - want).abs() < 1.0 && (v.height - want / 2.0).abs() < 1.0, "{v:?} vs {want}");
    // A second import of the same file starts at the size of the first one, even far away.
    h.tool = Box::new(ImageTool::new(picture(), false));
    let q = DVec3::new(-2000.0, 0.0, 0.0);
    h.hover(q);
    let m = h.px(q);
    h.with(|t, cx| t.double_click(cx, m));
    let ents = &h.doc.model.root;
    let sizes: Vec<f64> =
        ents.instances.keys().filter_map(|i| h.doc.model.image_of(None, i)).map(|v| v.width).collect();
    assert_eq!(sizes.len(), 2);
    assert!((sizes[0] - sizes[1]).abs() < 1e-6, "{sizes:?}");
}

#[test]
fn as_a_texture_the_picture_paints_the_face() {
    let mut h = Harness::new();
    h.tool(ToolId::Rect);
    h.click(DVec3::ZERO);
    h.hover(DVec3::new(900.0, 500.0, 0.0));
    h.type_vcb("2000;1000");
    let f = h.doc.ents().faces.keys().next().unwrap();
    h.tool = Box::new(ImageTool::new(picture(), true));
    // Off the face: nothing happens.
    h.click(DVec3::new(-500.0, -500.0, 0.0));
    assert!(h.tool.switch_to().is_none());
    h.click(DVec3::new(100.0, 100.0, 0.0));
    h.click(DVec3::new(500.0, 300.0, 0.0));
    assert_eq!(h.tool.switch_to(), Some(ToolId::Select));
    assert!(h.doc.model.root.instances.is_empty(), "no Image entity");
    let face = &h.doc.ents().faces[f];
    // The face is seen from above: its side facing up is painted.
    let up_is_front = face.normal.z > 0.0;
    let (mat, map) = if up_is_front {
        (face.front.unwrap(), face.tex.as_ref().and_then(|t| t.front).unwrap())
    } else {
        (face.back.unwrap(), face.tex.as_ref().and_then(|t| t.back).unwrap())
    };
    let m = &h.doc.model.materials[mat];
    assert!(!m.ext.owner_image);
    let t = m.ext.texture.as_ref().unwrap();
    assert!((t.size[0] - 400.0).abs() < 1.0 && (t.size[1] - 200.0).abs() < 1.0, "{:?}", t.size);
    // The first point is the corner of a tile; the second one is one tile further.
    assert!((map.at(DVec3::new(100.0, 100.0, 0.0)) - DVec2::ZERO).length() < 1e-3);
    assert!(
        (map.at(DVec3::new(500.0, 300.0, 0.0)) - DVec2::ONE).length() < 1e-2,
        "{}",
        map.at(DVec3::new(500.0, 300.0, 0.0))
    );
}
