//! File › Save As / Save a Copy As: the type list of the save dialog. Besides Esquisse's own
//! format, a model is written (natively, `esq_skp::native::write`) for any SketchUp version
//! `Model#save(path, Sketchup::Model::VERSION_…)` knows: the current format (unchanged since
//! SketchUp 2021) and the older ones down to SketchUp 3. SketchUp 2026's own dialog offers the
//! current format only (`docs/skp-format.md`, §8.1).

use std::path::{Path, PathBuf};

use esq_skp::SkpVersion;

use crate::filedlg::FileType;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveFormat {
    Esq,
    /// `None` = the current SketchUp format.
    Skp(Option<SkpVersion>),
}

/// The entries of the type list, in order.
pub fn types() -> Vec<(FileType, SaveFormat)> {
    let mut v = vec![
        (FileType::new("Esquisse", &["esq"]), SaveFormat::Esq),
        (FileType::new("Modèles SketchUp", &["skp"]), SaveFormat::Skp(None)),
    ];
    // 2021 is the current format: it is the line above.
    for ver in SkpVersion::ALL.into_iter().filter(|v| *v != SkpVersion::Su2021) {
        v.push((FileType::new(format!("SketchUp version {}", ver.name()), &["skp"]), SaveFormat::Skp(Some(ver))));
    }
    v
}

/// The format of a file given by its name alone (Save of a document that has a file).
pub fn format_of(path: &Path) -> SaveFormat {
    match path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).as_deref() {
        Some("skp") => SaveFormat::Skp(None),
        _ => SaveFormat::Esq,
    }
}

/// The save dialog; `current` selects the type the document has.
pub fn ask(title: &str, file_name: &str, current: SaveFormat) -> Option<(PathBuf, SaveFormat)> {
    let list = types();
    let initial = list.iter().position(|(_, f)| *f == current).unwrap_or(0);
    let ft: Vec<FileType> = list.iter().map(|(t, _)| t.clone()).collect();
    let (p, i) = crate::filedlg::save(title, file_name, &ft, initial, "models")?;
    Some((p, list[i].1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_view_style_goes_with_the_model() {
        use crate::style::{FaceStyle, ViewStyle};
        let mut m = esq_core::model::Model::new();
        let mut s =
            ViewStyle { faces: FaceStyle::ShadedWithTextures, profiles: false, axes: false, ..Default::default() };
        crate::styles::write_style(&mut m, &s);
        assert_eq!(m.view_ext["ro"]["RenderMode"], serde_json::json!(2));
        assert_eq!(m.view_ext["ro"]["Texture"], serde_json::json!(true));
        assert_eq!(m.view_ext["ro"]["DrawSilhouettes"], serde_json::json!(false));
        // Read back into another view.
        let mut t = ViewStyle::default();
        t.restore(m.view_ext["ro"].as_object().unwrap());
        assert_eq!((t.faces, t.profiles, t.axes), (FaceStyle::ShadedWithTextures, false, false));
        // Hidden line, as SketchUp's templates would have it.
        s.faces = FaceStyle::HiddenLine;
        crate::styles::write_style(&mut m, &s);
        t.restore(m.view_ext["ro"].as_object().unwrap());
        assert_eq!(t.faces, FaceStyle::HiddenLine);
        m.view_ext.insert("ro".into(), serde_json::json!({"RenderMode": 2, "Texture": false, "EdgeDisplayMode": 0}));
        t.restore(m.view_ext["ro"].as_object().unwrap());
        assert_eq!((t.faces, t.edges), (FaceStyle::Shaded, false));
    }

    #[test]
    fn every_older_version_has_its_type() {
        let t = types();
        assert_eq!(t.len(), 2 + 14);
        assert_eq!(t[2].0.title(), "SketchUp version 2020 (*.skp)");
        assert_eq!(t.last().unwrap().1, SaveFormat::Skp(Some(SkpVersion::Su3)));
        // A file name typed as .skp keeps the version chosen in the list.
        let ft: Vec<FileType> = t.iter().map(|(x, _)| x.clone()).collect();
        assert_eq!(crate::filedlg::chosen(PathBuf::from("a.skp"), 5, &ft).1, 5);
        assert_eq!(crate::filedlg::chosen(PathBuf::from("a"), 5, &ft), (PathBuf::from("a.skp"), 5));
        assert_eq!(crate::filedlg::chosen(PathBuf::from("a.esq"), 5, &ft).1, 0);
        assert_eq!(format_of(Path::new("x.SKP")), SaveFormat::Skp(None));
    }
}
