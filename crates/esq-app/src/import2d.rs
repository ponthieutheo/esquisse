//! File › Import of a picture (SketchUp's `CAllImageImporter` and the image importers): the
//! file's type is chosen in the open dialog, then "Utiliser image comme" (SketchUp's
//! `ImportImageAsType` preference, kept between sessions):
//!
//! - Image: the Image tool places it (`tools/image.rs`);
//! - Texture: the Image tool paints it on a face;
//! - Nouvelle photo adaptée: Camera › Match New Photo with that picture.

use std::path::{Path, PathBuf};

use eframe::egui;
use serde_json::{Value, json};

use crate::app::EsquisseApp;
use crate::imgfile::LoadedImage;
use crate::tools::{ImageTool, MatchPhotoTool, ToolId};
use crate::ui::centered;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageUse {
    Image,
    Texture,
    MatchPhoto,
}

impl ImageUse {
    pub const ALL: [ImageUse; 3] = [ImageUse::Image, ImageUse::Texture, ImageUse::MatchPhoto];

    pub fn label(self) -> &'static str {
        match self {
            ImageUse::Image => "Image",
            ImageUse::Texture => "Texture",
            ImageUse::MatchPhoto => "Nouvelle photo adaptée",
        }
    }

    fn code(self) -> u64 {
        self as u64
    }

    fn of_code(c: u64) -> ImageUse {
        ImageUse::ALL.get(c as usize).copied().unwrap_or(ImageUse::Image)
    }

    /// Position in "Utiliser image comme :" (SketchUp's `ImportImageAsType`: 0, 1, 2).
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn of_index(i: usize) -> ImageUse {
        ImageUse::of_code(i as u64)
    }
}

/// The picture types of SketchUp's import dialog, after its 3D formats: all of them, each
/// one, then PDF.
pub const PICTURE_TYPES: [(&str, &[&str]); 8] = [
    ("Tous les types d'images pris en charge (*.bmp, *.jpg, *.png, *.psd, *.tif, *.tga)", &crate::imgfile::IMAGE_EXTS),
    ("Image JPEG (*.jpg, *.jpeg)", &["jpg", "jpeg"]),
    ("Image PNG (*.png)", &["png"]),
    ("Photoshop (*.psd)", &["psd"]),
    ("Image TIFF (*.tif, *.tiff)", &["tif", "tiff"]),
    ("Fichier Targa (*.tga)", &["tga"]),
    ("Bitmap Windows (*.bmp)", &["bmp"]),
    ("Fichier PDF (*.pdf)", &["pdf"]),
];

fn prefs_path() -> Option<PathBuf> {
    crate::prefs::dir().map(|d| d.join("import.json"))
}

/// The last "Use Image As".
pub fn load_prefs() -> ImageUse {
    let v: Value = prefs_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null);
    ImageUse::of_code(v.get("image_as").and_then(Value::as_u64).unwrap_or(0))
}

pub fn save_prefs(use_as: ImageUse) {
    if let Some(p) = prefs_path() {
        let _ = std::fs::write(p, json!({"image_as": use_as.code()}).to_string());
    }
}

/// The "Use Image As" window of a picture that is being imported.
pub struct ImportImageDialog {
    pub image: LoadedImage,
    pub use_as: ImageUse,
}

/// Loads the picture and asks how to use it. Returns false when `path` is no picture.
pub fn start(app: &mut EsquisseApp, ctx: &egui::Context, path: &Path) -> bool {
    if !crate::imgfile::is_image(path) {
        return false;
    }
    match crate::imgfile::load(path) {
        Ok(image) => {
            app.ui_state.extra.import_image = Some(ImportImageDialog { image, use_as: load_prefs() });
        }
        Err(e) => app.say(ctx, e),
    }
    true
}

/// Loads the picture chosen in SketchUp's import dialog and uses it as `use_as` says (the
/// choice made in the dialog's "Utiliser image comme :").
pub fn import_as(app: &mut EsquisseApp, ctx: &egui::Context, path: &Path, use_as: ImageUse) {
    let r = crate::imgfile::load(path).and_then(|image| apply(app, image, use_as));
    if let Err(e) = r {
        app.say(ctx, e);
    }
}

/// Starts what "Use Image As" asks for.
pub fn apply(app: &mut EsquisseApp, image: LoadedImage, use_as: ImageUse) -> Result<(), String> {
    match use_as {
        ImageUse::Image | ImageUse::Texture => {
            app.set_tool(ToolId::Select);
            app.tool = Box::new(ImageTool::new(image, use_as == ImageUse::Texture));
        }
        ImageUse::MatchPhoto => {
            let t = MatchPhotoTool::from_bytes(&image.name, image.bytes)?;
            app.set_tool(ToolId::Select);
            app.remember_view();
            app.tool = Box::new(t);
        }
    }
    Ok(())
}

pub fn show(app: &mut EsquisseApp, ctx: &egui::Context) {
    let Some(mut dlg) = app.ui_state.extra.import_image.take() else { return };
    let mut action = None;
    centered("Importer").show(ctx, |ui| {
        ui.label(format!("{} — {} × {} pixels", dlg.image.name, dlg.image.px[0], dlg.image.px[1]));
        ui.separator();
        ui.label("Utiliser image comme :");
        for u in ImageUse::ALL {
            ui.radio_value(&mut dlg.use_as, u, u.label());
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("Importer").clicked() {
                action = Some(true);
            }
            if ui.button("Annuler").clicked() {
                action = Some(false);
            }
        });
    });
    match action {
        None => app.ui_state.extra.import_image = Some(dlg),
        Some(false) => {}
        Some(true) => {
            save_prefs(dlg.use_as);
            if let Err(e) = apply(app, dlg.image, dlg.use_as) {
                app.say(ctx, e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dialog_lists_every_image_type() {
        use crate::formats3d::ImportKind;
        let list = ImportKind::list();
        let all = ImportKind::All.extensions();
        for e in ["skp", "esq", "bmp", "jpg", "png", "psd", "tif", "tga", "pdf"] {
            assert!(all.contains(&e), "{e}");
        }
        let psd = list.iter().find(|k| k.label() == "Photoshop (*.psd)").expect("PSD");
        assert_eq!(psd.extensions(), ["psd"]);
        assert!(crate::imgfile::is_image(Path::new("a.PSD")) && crate::imgfile::is_image(Path::new("plan.pdf")));
        assert_eq!(ImageUse::of_code(2), ImageUse::MatchPhoto);
        assert_eq!(ImageUse::of_code(9), ImageUse::Image);
    }
}
