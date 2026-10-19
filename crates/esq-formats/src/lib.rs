//! The 3D file formats SketchUp 2026 imports and exports (its `Importers\` and `Exporters\`
//! plug-ins), rewritten in Rust: same formats, same options, same defaults.
//!
//! Importers turn a file into a [`Model`] that the application inserts as a component;
//! exporters write a model (or the selection, see [`scene::selection_model`]).

use std::path::Path;

use anyhow::{Result, bail};
use esq_core::model::{Model, SceneCamera, Units};

pub mod acad;
pub mod acad_dwg;
pub mod build;
pub mod cad;
pub mod dae;
pub mod dem;
pub mod fbx;
pub mod gltf;
pub mod ifc;
pub mod iso8211;
pub mod kmz;
pub mod obj;
pub mod options;
pub mod report;
pub mod scene;
pub mod step;
pub mod stl;
pub mod tex;
pub mod threeds;
pub mod trb;
pub mod units;
pub mod usd;
pub mod vrml;
pub mod xml;
pub mod xsi;
pub mod zipfile;

pub use options::AllOptions;

/// The 3D formats of File › Import.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImportFormat {
    ThreeDs,
    Dae,
    Dem,
    Dwg,
    Glb,
    Ifc,
    Kmz,
    Stl,
    Trb,
    Usdz,
}

impl ImportFormat {
    pub const ALL: [ImportFormat; 10] = [
        ImportFormat::ThreeDs,
        ImportFormat::Dwg,
        ImportFormat::Dae,
        ImportFormat::Dem,
        ImportFormat::Kmz,
        ImportFormat::Glb,
        ImportFormat::Ifc,
        ImportFormat::Stl,
        ImportFormat::Trb,
        ImportFormat::Usdz,
    ];

    /// Label of the file type, as in SketchUp's import dialog.
    pub fn label(self) -> &'static str {
        match self {
            ImportFormat::ThreeDs => "Fichier 3DS (*.3ds)",
            ImportFormat::Dae => "Fichiers COLLADA (*.dae)",
            ImportFormat::Dem => "DEM (*.dem, *.ddf)",
            ImportFormat::Dwg => "Fichiers AutoCAD (*.dwg, *.dxf)",
            ImportFormat::Glb => "Fichier binaire glTF (*.glb)",
            ImportFormat::Ifc => "Fichiers IFC (*.ifc, *.ifcZIP)",
            ImportFormat::Kmz => "Fichiers Google Earth (*.kmz)",
            ImportFormat::Stl => "Fichiers STereoLithography (*.stl)",
            ImportFormat::Trb => "Fichier TrimBIM (*.trb)",
            ImportFormat::Usdz => "Fichier USDZ (*.usdz)",
        }
    }

    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            ImportFormat::ThreeDs => &["3ds"],
            ImportFormat::Dae => &["dae"],
            ImportFormat::Dem => &["dem", "ddf"],
            ImportFormat::Dwg => &["dwg", "dxf"],
            ImportFormat::Glb => &["glb", "gltf"],
            ImportFormat::Ifc => &["ifc", "ifczip"],
            ImportFormat::Kmz => &["kmz"],
            ImportFormat::Stl => &["stl"],
            ImportFormat::Trb => &["trb"],
            ImportFormat::Usdz => &["usdz", "usd", "usda", "usdc"],
        }
    }

    pub fn from_path(p: &Path) -> Option<ImportFormat> {
        let ext = p.extension()?.to_str()?.to_lowercase();
        Self::ALL.into_iter().find(|f| f.extensions().contains(&ext.as_str()))
    }

    /// Whether the dialog has an "Options…" button.
    pub fn has_options(self) -> bool {
        !matches!(self, ImportFormat::Trb)
    }

    pub fn implemented(self) -> bool {
        true
    }
}

/// The 3D formats of File › Export › 3D Model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExportFormat {
    ThreeDs,
    Dwg,
    Dxf,
    Dae,
    Fbx,
    Ifc,
    Kmz,
    Obj,
    Stl,
    Vrml,
    Xsi,
    Glb,
    Usdz,
}

impl ExportFormat {
    pub const ALL: [ExportFormat; 13] = [
        ExportFormat::ThreeDs,
        ExportFormat::Dwg,
        ExportFormat::Dxf,
        ExportFormat::Dae,
        ExportFormat::Fbx,
        ExportFormat::Ifc,
        ExportFormat::Kmz,
        ExportFormat::Obj,
        ExportFormat::Stl,
        ExportFormat::Vrml,
        ExportFormat::Xsi,
        ExportFormat::Glb,
        ExportFormat::Usdz,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ExportFormat::ThreeDs => "Fichier 3DS (*.3ds)",
            ExportFormat::Dwg => "Fichier AutoCAD DWG (*.dwg)",
            ExportFormat::Dxf => "Fichier AutoCAD DXF (*.dxf)",
            ExportFormat::Dae => "Fichier COLLADA (*.dae)",
            ExportFormat::Fbx => "Fichier FBX (*.fbx)",
            ExportFormat::Ifc => "IFC Fichier (*.ifc)",
            ExportFormat::Kmz => "Fichier Google Earth (*.kmz)",
            ExportFormat::Obj => "Fichier OBJ (*.obj)",
            ExportFormat::Stl => "Fichier STereoLithography (*.stl)",
            ExportFormat::Vrml => "Fichier VRML (*.wrl)",
            ExportFormat::Xsi => "Fichier XSI (*.xsi)",
            ExportFormat::Glb => "Fichier binaire glTF (*.glb)",
            ExportFormat::Usdz => "Fichier USDZ (*.usdz)",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::ThreeDs => "3ds",
            ExportFormat::Dwg => "dwg",
            ExportFormat::Dxf => "dxf",
            ExportFormat::Dae => "dae",
            ExportFormat::Fbx => "fbx",
            ExportFormat::Ifc => "ifc",
            ExportFormat::Kmz => "kmz",
            ExportFormat::Obj => "obj",
            ExportFormat::Stl => "stl",
            ExportFormat::Vrml => "wrl",
            ExportFormat::Xsi => "xsi",
            ExportFormat::Glb => "glb",
            ExportFormat::Usdz => "usdz",
        }
    }

    pub fn from_path(p: &Path) -> Option<ExportFormat> {
        let ext = p.extension()?.to_str()?.to_lowercase();
        Self::ALL.into_iter().find(|f| f.extension() == ext)
    }

    /// Whether the dialog has an "Options…" button (the SimLab exporters have none).
    pub fn has_options(self) -> bool {
        !matches!(self, ExportFormat::Glb | ExportFormat::Usdz)
    }

    /// "Exporter uniquement la sélection" (or its variants) is ticked.
    pub fn selection_only(self, o: &AllOptions) -> bool {
        match self {
            ExportFormat::ThreeDs => o.tds.selection_only,
            ExportFormat::Dae => o.dae.selection_only,
            ExportFormat::Fbx => o.fbx.selection_only,
            ExportFormat::Ifc => o.ifc.selection_only,
            ExportFormat::Obj => o.obj.selection_only,
            ExportFormat::Stl => o.stl.selection_only,
            ExportFormat::Xsi => o.xsi.0.selection_only,
            _ => false,
        }
    }

    pub fn implemented(self) -> bool {
        true
    }
}

/// What an import gives the application.
pub struct Imported {
    pub model: Model,
    /// "Résultats de l'importation" text, for the importers that show one.
    pub report: Option<String>,
    /// Place the content where it is in the file instead of with the cursor.
    pub preserve_origin: bool,
}

/// File › Import of a 3D file; `units` are the units of the model it goes into ("Unités du
/// modèle").
pub fn import(path: &Path, o: &AllOptions, units: Units) -> Result<Imported> {
    let Some(fmt) = ImportFormat::from_path(path) else { bail!("format de fichier non pris en charge") };
    let bytes = std::fs::read(path)?;
    let (model, report, preserve_origin) = match fmt {
        ImportFormat::Stl => (stl::import(&bytes, &o.stl_in)?, None, o.stl_in.preserve_origin),
        ImportFormat::ThreeDs => {
            let (m, r) = threeds::import(&bytes, Some(path), &o.tds_in, units)?;
            (m, Some(r), false)
        }
        ImportFormat::Dae => {
            let text = String::from_utf8_lossy(&bytes);
            (dae::import(&text, Some(path), &o.dae_in)?, None, false)
        }
        ImportFormat::Kmz => (kmz::import(&bytes, &o.kmz_in)?.0, None, false),
        ImportFormat::Glb => (gltf::import(&bytes, Some(path), &o.glb_in)?, None, o.glb_in.0.preserve_origin),
        ImportFormat::Usdz => (usd::import(path, &o.usdz_in)?, None, o.usdz_in.0.preserve_origin),
        ImportFormat::Dwg => {
            let (m, r) = cad::import(&bytes, &o.dwg_in, units)?;
            (m, Some(r), o.dwg_in.preserve_origin)
        }
        ImportFormat::Ifc => {
            let (m, r) = ifc::import(&bytes, &o.ifc_in)?;
            (m, Some(r), o.ifc_in.preserve_origin)
        }
        ImportFormat::Dem => {
            let (m, r) = dem::import(path, &o.dem_in)?;
            (m, Some(r), false)
        }
        ImportFormat::Trb => (trb::import(&bytes)?, Some(trb::report(true)), false),
    };
    Ok(Imported { model, report, preserve_origin })
}

/// What the exporters may need besides the model.
#[derive(Clone, Debug, Default)]
pub struct ExportCx {
    /// The current point of view (the cameras of 3DS and VRML).
    pub view: Option<SceneCamera>,
    /// Name of the model (file name without extension).
    pub name: String,
}

/// File › Export › 3D Model. Returns the "Résultats de l'exportation" text, if the format
/// shows one.
pub fn export(model: &Model, path: &Path, fmt: ExportFormat, o: &AllOptions, cx: &ExportCx) -> Result<Option<String>> {
    match fmt {
        ExportFormat::Stl => {
            std::fs::write(path, stl::export(model, &cx.name, &o.stl))?;
            Ok(None)
        }
        ExportFormat::Obj => obj::export(model, path, &o.obj).map(Some),
        ExportFormat::ThreeDs => threeds::export(model, path, &o.tds).map(Some),
        ExportFormat::Dae => dae::export(model, path, &o.dae, cx).map(|_| None),
        ExportFormat::Kmz => kmz::export(model, path, &o.kmz, cx).map(|_| None),
        ExportFormat::Vrml => vrml::export(model, path, &o.vrml, cx).map(Some),
        ExportFormat::Xsi => xsi::export(model, path, &o.xsi).map(Some),
        ExportFormat::Fbx => fbx::export(model, path, &o.fbx).map(Some),
        ExportFormat::Glb => gltf::export(model, path).map(|_| None),
        ExportFormat::Usdz => usd::export_usdz(model, path).map(|_| None),
        ExportFormat::Dxf => acad::export_dxf(model, path, &o.acad).map(|_| None),
        ExportFormat::Dwg => acad_dwg::export_dwg(model, path, &o.acad).map(|_| None),
        ExportFormat::Ifc => ifc::export(model, path, &o.ifc).map(Some),
    }
}
