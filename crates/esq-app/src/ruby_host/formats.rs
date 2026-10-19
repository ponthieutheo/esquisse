//! The option hashes of `Model#export` and `Model#import`, read as SketchUp.exe reads them.
//!
//! SketchUp (for exports and imports) walks a fixed table of
//! Ruby keys, in the order of their names, and looks each one up in the hash **as a Symbol**
//! (a String key is never seen). A found value becomes a typed option under SketchUp's own
//! name (`:triangulated_faces` → `ExportTriangulatedFaces`): `true`/`false` a boolean, an
//! Integer an integer, a Float a number, and a String an integer through SketchUp's tables of
//! names ("by_layer", "acad_2013", "mm"…, the same tables whatever the key). The exporter then
//! copies, over its saved options, those it knows *with the type it expects*: an unknown key,
//! `nil`, or a value of the wrong type changes nothing and raises nothing.

use esq_core::model::Units;
use esq_core::slice_export::SliceOptions;
use esq_core::vector2d::CadWide;
use esq_formats::options::{
    AcadVersion, Favor3ds, Hierarchy3ds, IfcRepresentation, IfcTags, IfcVersion, StlFormat, StlImportUnit, StlUnit,
    TwoSidedAs,
};
use esq_formats::units::LenUnit;
use esq_formats::{AllOptions, ExportFormat, ImportFormat};
use serde_json::{Map, Value as J};

use crate::export_vec::{DrawUnit, VecOptions};
use crate::export2d::RasterOptions;

/// A typed option (SketchUp's `SUTypedValue`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Typed {
    Bool(bool),
    Int(i32),
    Num(f64),
}

/// Ruby key → exporter option (the 45 pairs).
const EXPORT_KEYS: &[(&str, &str)] = &[
    ("acad_version", "ExportAcadVersion"),
    ("allow_mirrored_components", "ExportAllowMirroredComponents"),
    ("author_attribution", "ExportAuthorAttribution"),
    ("camera_lookat", "ExportCameraLookat"),
    ("cameras", "ExportCameras"),
    ("construction_geometry", "ExportConstructionGeometry"),
    ("dimensions", "ExportDimensions"),
    ("doublesided_faces", "ExportDoubleSidedFaces"),
    ("edges", "ExportEdges"),
    ("extension_automatic", "ExtensionAutomatic"),
    ("extension_length", "ExtensionLength"),
    ("faces", "ExportFaces"),
    ("faces_flag", "ExportFacesFlag"),
    ("format", "ExportFormat"),
    ("full_scale", "FullScale"),
    ("geometry", "ExportGeometry"),
    ("height", "Height"),
    ("hidden_geometry", "ExportHiddenGeometry"),
    ("ifc_version", "IfcVersion"),
    ("in_drawing", "InDrawing"),
    ("in_model", "InModel"),
    ("material_overrides", "ExportMaterialOverrides"),
    ("preserve_instancing", "ExportPreserveHierarchies"),
    ("preserve_texture_coords", "PreserveTextureCoords"),
    ("profile_automatic", "ProfileAutomatic"),
    ("profile_on_layer", "ProfileOnLayer"),
    ("profile_type", "ProfileType"),
    ("profile_width", "ProfileWidth"),
    ("section_automatic", "SectionAutomatic"),
    ("section_line_type", "ExportSectionLineType"),
    ("section_on_layer", "SectionOnLayer"),
    ("section_width", "SectionWidth"),
    ("selectionset_only", "ExportSelectionSetOnly"),
    ("separate_disconnected_faces", "ExportSeparateDisconnectedFaces"),
    ("show_extensions", "ShowExtensions"),
    ("standard_ifc_hierarchy", "StandardIfcHierarchy"),
    ("swap_yz", "SwapYZ"),
    ("tessellated_geometry", "TessellatedGeometry"),
    ("text", "ExportText"),
    ("texture_maps", "ExportTextureMaps"),
    ("triangulated_faces", "ExportTriangulatedFaces"),
    ("type", "ExportType"),
    ("units", "ExportUnits"),
    ("use_vrml_orientation", "ExportUseVRMLOrientation"),
    ("width", "Width"),
];

/// Ruby key → importer option (the 12 pairs, misspelling included).
const IMPORT_KEYS: &[(&str, &str)] = &[
    ("generate_texture", "ImportGenerateTexture"),
    ("import_materials", "ImportMaterials"),
    ("max_points", "ImportMaxPoints"),
    ("merge_coplanar_faces", "ImportMergeCoplanarFaces"),
    ("merge_coplaner_faces", "ImportMergeCoplanarFaces"),
    ("orient_faces", "ImportOrientFaces"),
    ("preserve_origin", "ImportPreserveOrigin"),
    ("swap_yz", "SwapYZ"),
    ("tags_handling", "ImportTagsOption"),
    ("units", "ImportUnits"),
    ("validate_dae", "ImportValidateDAE"),
    ("validate_kmz", "ImportValidateKmz"),
];

/// Units of the exporters and importers other than STL, in `LenUnit` order.
const UNITS: &[&str] = &["model", "inch", "foot", "yard", "mile", "mm", "cm", "m", "km"];
/// Units of the STL exporter and importer, in `StlUnit` order.
const STL_UNITS: &[&str] = &["model", "m", "cm", "mm", "feet", "inch"];

/// The tables a String value of an export goes through, in SketchUp's order: export type,
/// 3DS hierarchy, 3DS two-sided faces, AutoCAD version, IFC version, section type, line types.
const EXPORT_NAMES: &[&[&str]] = &[
    &["3d", "2d", "section"],
    &["full_hierarchy", "by_layer", "by_material", "single_object"],
    &["not_two_sided", "two_sided_as_material", "two_sided_as_geometry"],
    &["acad_12", "acad_13", "acad_14", "acad_2000", "acad_2004", "acad_2007", "acad_2010", "acad_2013"],
    &["IFC 2x3", "IFC 4"],
    &["true_section", "screen_projection"],
    &["none", "polyline_with_width", "wide_line_entities"],
];

fn index(table: &[&str], s: &str) -> Option<i32> {
    table.iter().position(|x| *x == s).map(|i| i as i32)
}

/// A String value: every table is tried and the last match wins (as SketchUp stores each).
fn export_string(s: &str, stl: bool) -> Option<i32> {
    let mut found = None;
    for t in EXPORT_NAMES {
        found = index(t, s).or(found);
    }
    if stl {
        found = index(STL_UNITS, s).or(found);
        // "none" and "ascii" are the ASCII format, "binary" the binary one.
        found = match s {
            "none" | "ascii" => Some(0),
            "binary" => Some(1),
            _ => found,
        };
    } else {
        found = index(UNITS, s).or(found);
    }
    found
}

fn import_string(s: &str, stl: bool) -> Option<i32> {
    if stl {
        let mut found = index(STL_UNITS, s);
        found = match s {
            "none" | "ascii" => Some(0),
            "binary" => Some(1),
            _ => found,
        };
        found
    } else {
        index(UNITS, s).or(index(&["use_ifc_classes", "use_ifc_layers", "do_not_create_tags"], s))
    }
}

fn typed(v: &J, string: impl Fn(&str) -> Option<i32>) -> Option<Typed> {
    match v {
        J::Bool(b) => Some(Typed::Bool(*b)),
        J::Number(n) if n.is_i64() || n.is_u64() => Some(Typed::Int(n.as_i64().unwrap_or(i64::MAX) as i32)),
        J::Number(n) => n.as_f64().map(Typed::Num),
        J::String(s) => string(s).map(Typed::Int),
        _ => None,
    }
}

/// The typed options a hash gives, by SketchUp name, in the order SketchUp applies them.
fn read(
    h: &Map<String, J>,
    keys: &[(&str, &'static str)],
    string: impl Fn(&str) -> Option<i32>,
) -> Vec<(&'static str, Typed)> {
    let mut keys = keys.to_vec();
    keys.sort_by_key(|(k, _)| *k);
    keys.into_iter().filter_map(|(k, su)| h.get(k).and_then(|v| typed(v, &string)).map(|t| (su, t))).collect()
}

fn pick<T: Copy>(all: &[T], i: i32) -> Option<T> {
    usize::try_from(i).ok().and_then(|i| all.get(i)).copied()
}

/// What an export asks besides the exporter's own options.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ExportRequest {
    /// `:selectionset_only => true`: only the selection is written. SketchUp decides this
    /// itself from the hash, before the exporter, for every format; a
    /// script that does not say so exports the whole model, whatever the saved option.
    pub selection_only: bool,
    /// `:type` of the AutoCAD exporter: 0 3D model, 1 2D graphic, 2 section slice.
    pub acad_type: i32,
}

/// Lays the hash `h` of `Model#export` over the options `o` of the format `f`.
pub fn apply_export(o: &mut AllOptions, f: ExportFormat, h: &Map<String, J>) -> ExportRequest {
    use ExportFormat as E;
    use Typed::{Bool, Int};
    let mut req = ExportRequest::default();
    for (key, v) in read(h, EXPORT_KEYS, |s| export_string(s, f == E::Stl)) {
        if let ("ExportSelectionSetOnly", Bool(b)) = (key, v) {
            req.selection_only = b;
        }
        match (f, key, v) {
            (E::Fbx, _, _) => {
                // As OBJ without edges, plus "Séparer les faces déconnectées".
                let x = &mut o.fbx;
                match (key, v) {
                    ("ExportSelectionSetOnly", Bool(b)) => x.selection_only = b,
                    ("ExportTriangulatedFaces", Bool(b)) => x.triangulate = b,
                    ("ExportDoubleSidedFaces", Bool(b)) => x.two_sided = b,
                    ("ExportSeparateDisconnectedFaces", Bool(b)) => x.separate_disconnected = b,
                    ("ExportTextureMaps", Bool(b)) => x.texture_maps = b,
                    ("SwapYZ", Bool(b)) => x.swap_yz = b,
                    ("ExportUnits", Int(i)) => x.units = pick(&LenUnit::ALL, i).unwrap_or(x.units),
                    _ => {}
                }
            }
            (E::Obj | E::Xsi, _, _) => {
                let x = if f == E::Obj { &mut o.obj } else { &mut o.xsi.0 };
                match (key, v) {
                    ("ExportSelectionSetOnly", Bool(b)) => x.selection_only = b,
                    ("ExportTriangulatedFaces", Bool(b)) => x.triangulate = b,
                    ("ExportDoubleSidedFaces", Bool(b)) => x.two_sided = b,
                    ("ExportEdges", Bool(b)) => x.edges = b,
                    ("ExportTextureMaps", Bool(b)) => x.texture_maps = b,
                    ("SwapYZ", Bool(b)) => x.swap_yz = b,
                    ("ExportUnits", Int(i)) => x.units = pick(&LenUnit::ALL, i).unwrap_or(x.units),
                    _ => {}
                }
            }
            (E::ThreeDs, _, _) => {
                let x = &mut o.tds;
                match (key, v) {
                    ("ExportGeometry", Int(i)) => x.hierarchy = pick(Hierarchy3ds::ALL, i).unwrap_or(x.hierarchy),
                    ("ExportSelectionSetOnly", Bool(b)) => x.selection_only = b,
                    ("ExportDoubleSidedFaces", Bool(b)) => x.two_sided = b,
                    // What the exporter reads: 0 one-sided, 1 two-sided as materials, 2 as geometry.
                    ("ExportFaces", Int(i @ 0..=2)) => {
                        x.two_sided = i > 0;
                        if i > 0 {
                            x.two_sided_as = if i == 1 { TwoSidedAs::Materials } else { TwoSidedAs::Geometry };
                        }
                    }
                    ("ExportEdges", Bool(b)) => x.standalone_edges = b,
                    ("ExportTextureMaps", Bool(b)) => x.texture_maps = b,
                    ("PreserveTextureCoords", Bool(b)) => {
                        x.favor = if b { Favor3ds::PreserveTexCoords } else { Favor3ds::WeldVertices }
                    }
                    ("ExportCameras", Bool(b)) => x.cameras = b,
                    ("ExportUnits", Int(i)) => x.units = pick(&LenUnit::ALL, i).unwrap_or(x.units),
                    _ => {}
                }
            }
            (E::Dae, _, _) => {
                let x = &mut o.dae;
                match (key, v) {
                    ("ExportTriangulatedFaces", Bool(b)) => x.triangulate = b,
                    ("ExportDoubleSidedFaces", Bool(b)) => x.two_sided = b,
                    ("ExportEdges", Bool(b)) => x.edges = b,
                    ("ExportAuthorAttribution", Bool(b)) => x.credits = b,
                    ("ExportTextureMaps", Bool(b)) => x.texture_maps = b,
                    ("ExportSelectionSetOnly", Bool(b)) => x.selection_only = b,
                    ("ExportPreserveHierarchies", Bool(b)) => x.preserve_hierarchies = b,
                    ("ExportHiddenGeometry", Bool(b)) => x.hidden = b,
                    _ => {}
                }
            }
            (E::Kmz, "ExportHiddenGeometry", Bool(b)) => o.kmz.hidden = b,
            (E::Kmz, "ExportAuthorAttribution", Bool(b)) => o.kmz.credits = b,
            (E::Vrml, _, _) => {
                let x = &mut o.vrml;
                match (key, v) {
                    // The dialog's "Ignorer la matière du verso des faces" is its opposite.
                    ("ExportDoubleSidedFaces", Bool(b)) => x.ignore_back_material = !b,
                    ("ExportCameras", Bool(b)) => x.cameras = b,
                    ("ExportUseVRMLOrientation", Bool(b)) => x.vrml_orientation = b,
                    ("ExportEdges", Bool(b)) => x.edges = b,
                    ("ExportTextureMaps", Bool(b)) => x.texture_maps = b,
                    ("ExportAllowMirroredComponents", Bool(b)) => x.allow_mirrored = b,
                    ("ExportMaterialOverrides", Bool(b)) => x.material_overrides = b,
                    _ => {}
                }
            }
            (E::Dwg | E::Dxf, _, _) => {
                let x = &mut o.acad;
                match (key, v) {
                    ("ExportAcadVersion", Int(i)) => x.version = pick(AcadVersion::ALL, i).unwrap_or(x.version),
                    ("ExportFacesFlag", Bool(b)) => x.faces = b,
                    ("ExportEdges", Bool(b)) => x.edges = b,
                    ("ExportConstructionGeometry", Bool(b)) => x.construction = b,
                    ("ExportDimensions", Bool(b)) => x.dimensions = b,
                    ("ExportText", Bool(b)) => x.text = b,
                    ("ExportType", Int(i @ 0..=2)) => req.acad_type = i,
                    _ => {}
                }
            }
            (E::Ifc, _, _) => {
                let x = &mut o.ifc;
                match (key, v) {
                    ("IfcVersion", Int(i)) => x.version = pick(IfcVersion::ALL, i).unwrap_or(x.version),
                    ("StandardIfcHierarchy", Bool(b)) => x.standard_hierarchy = b,
                    ("ExportSelectionSetOnly", Bool(b)) => x.selection_only = b,
                    ("ExportHiddenGeometry", Bool(b)) => x.hidden = b,
                    ("TessellatedGeometry", Bool(b)) => {
                        x.representation = if b { IfcRepresentation::Tessellation } else { IfcRepresentation::Body }
                    }
                    _ => {}
                }
            }
            (E::Stl, _, _) => {
                let x = &mut o.stl;
                match (key, v) {
                    ("ExportSelectionSetOnly", Bool(b)) => x.selection_only = b,
                    ("ExportUnits", Int(i)) => x.units = pick(StlUnit::ALL, i).unwrap_or(x.units),
                    ("SwapYZ", Bool(b)) => x.swap_yz = b,
                    ("ExportFormat", Int(i)) => x.format = pick(StlFormat::ALL, i).unwrap_or(x.format),
                    _ => {}
                }
            }
            // The SimLab glTF / USDZ exporters have no options.
            _ => {}
        }
    }
    req
}

const INCH: f64 = 25.4;

/// A length of SketchUp's 2D exports (inches, a Float), in millimetres.
fn inches(v: &J) -> Option<f64> {
    v.as_f64().filter(|_| v.is_f64()).filter(|x| *x > 0.0).map(|x| x * INCH)
}

/// The keys of the PDF export (for `com.sketchup.exporters.pdf` only): they
/// set the vector exporter's own settings (the globals behind `PDFOptions` and its dialog).
/// Booleans must be true/false, units Integers (0 inches, 1 feet, 2 mm, 3 cm, 4 m, 5 points,
/// as `Length::Inches`…), lengths Floats in inches; anything else is left alone. `aspect` is
/// the view's width over its height (`:window_height` gives the width that keeps it).
/// `:map_fonts`, `:extension_units` and `:height_units` only change what the dialog shows.
pub fn apply_pdf(o: &mut VecOptions, h: &Map<String, J>, aspect: f64) {
    let flag = |k: &str| h.get(k).and_then(J::as_bool);
    let unit =
        |k: &str| h.get(k).filter(|v| v.is_i64()).and_then(J::as_i64).and_then(|i| pick(&DrawUnit::ALL, i as i32));
    for (k, f) in [
        ("output_profile_lines", &mut o.profiles),
        ("output_section_lines", &mut o.sections),
        ("edge_extensions", &mut o.extensions),
        ("match_screen_profiles", &mut o.profile_auto),
        ("match_screen_section", &mut o.section_auto),
        ("match_screen_extensions", &mut o.extension_auto),
        ("full_scale", &mut o.full_scale),
    ] {
        if let Some(b) = flag(k) {
            *f = b;
        }
    }
    for (k, u) in
        [("drawing_units", &mut o.drawing_unit), ("model_units", &mut o.model_unit), ("width_units", &mut o.size_unit)]
    {
        if let Some(x) = unit(k) {
            *u = x;
        }
    }
    let len = |k: &str| h.get(k).and_then(inches);
    // Line widths are at least 0.01 inch.
    let width = |k: &str| h.get(k).filter(|v| v.is_f64()).and_then(J::as_f64).map(|x| x.max(0.01) * INCH);
    if let Some(x) = len("extension_length") {
        o.extension_mm = x;
    }
    if let Some(x) = width("line_width") {
        o.profile_mm = x;
    }
    if let Some(x) = width("line_width_section") {
        o.section_mm = x;
    }
    if let Some(x) = len("length_in_drawing") {
        o.drawing_mm = x;
    }
    if let Some(x) = len("length_in_model") {
        o.model_mm = x;
    }
    if let Some(x) = len("window_height") {
        o.width_mm = x * aspect;
    }
}

/// The keys of the AutoCAD 2D export (`:type => "2d"`): the generic table of `Model#export`,
/// as `skp2acad`'s "Acad HLR Exporter" reads them:
/// booleans, Integers (version, line types), Floats in inches (sizes, widths, lengths).
pub fn apply_cad2d(o: &mut VecOptions, h: &Map<String, J>, aspect: f64) {
    use Typed::{Bool, Int, Num};
    for (key, v) in read(h, EXPORT_KEYS, |s| export_string(s, false)) {
        let mm = |x: f64| (x > 0.0).then_some(x * INCH);
        match (key, v) {
            ("FullScale", Bool(b)) => o.full_scale = b,
            ("InDrawing", Num(x)) => o.drawing_mm = mm(x).unwrap_or(o.drawing_mm),
            ("InModel", Num(x)) => o.model_mm = mm(x).unwrap_or(o.model_mm),
            // Height first (keys in name order): a width given too wins.
            ("Height", Num(x)) => o.width_mm = mm(x).map_or(o.width_mm, |y| y * aspect),
            ("Width", Num(x)) => o.width_mm = mm(x).unwrap_or(o.width_mm),
            ("ProfileAutomatic", Bool(b)) => o.profile_auto = b,
            ("ProfileOnLayer", Bool(b)) => o.profile_layer = b,
            ("ProfileType", Int(i)) => o.profile_cad = pick(&CAD_WIDE, i).unwrap_or(o.profile_cad),
            ("ProfileWidth", Num(x)) => o.profile_mm = mm(x).unwrap_or(o.profile_mm),
            ("SectionAutomatic", Bool(b)) => o.section_auto = b,
            ("ExportSectionLineType", Int(i)) => o.section_cad = pick(&CAD_WIDE, i).unwrap_or(o.section_cad),
            ("SectionWidth", Num(x)) => o.section_mm = mm(x).unwrap_or(o.section_mm),
            ("ShowExtensions", Bool(b)) => o.extensions = b,
            ("ExtensionAutomatic", Bool(b)) => o.extension_auto = b,
            ("ExtensionLength", Num(x)) => o.extension_mm = mm(x).unwrap_or(o.extension_mm),
            // The list of the 3D exporter: Version 12 (0) to AutoCAD 2018 (8).
            ("ExportAcadVersion", Int(i @ 0..=8)) => o.acad_version = i as usize,
            _ => {}
        }
    }
}

/// The keys of the section slice (`:type => "section"`): the generic table, as `skp2acad`'s
/// "Acad Section Exporter" reads it (the keys of the hidden-line exporter's
/// base, then `ExportSectionType`, which no Ruby key names: the projection
/// stays the saved one). Floats in inches; `aspect`: width over height of the drawing.
pub fn apply_slice(o: &mut SliceOptions, h: &Map<String, J>, aspect: f64) {
    use Typed::{Bool, Int, Num};
    for (key, v) in read(h, EXPORT_KEYS, |s| export_string(s, false)) {
        let mm = |x: f64| (x > 0.0).then_some(x * INCH);
        match (key, v) {
            ("FullScale", Bool(b)) => o.full_scale = b,
            ("InDrawing", Num(x)) => o.drawing_mm = mm(x).unwrap_or(o.drawing_mm),
            ("InModel", Num(x)) => o.model_mm = mm(x).unwrap_or(o.model_mm),
            ("Height", Num(x)) => o.width_mm = mm(x).map_or(o.width_mm, |y| y * aspect),
            ("Width", Num(x)) => o.width_mm = mm(x).unwrap_or(o.width_mm),
            ("SectionAutomatic", Bool(b)) => o.line_auto = b,
            ("ExportSectionLineType", Int(i)) => o.line_type = pick(&CAD_WIDE, i).unwrap_or(o.line_type),
            ("SectionWidth", Num(x)) => o.line_width = mm(x).unwrap_or(o.line_width),
            ("ExportAcadVersion", Int(i @ 0..=8)) => o.acad_version = i as usize,
            _ => {}
        }
    }
}

/// "none", "polyline_with_width", "wide_line_entities".
const CAD_WIDE: [CadWide; 3] = [CadWide::None, CadWide::Polyline, CadWide::WideLine];

/// The keys of an image export: `:width` and `:height` (Integers, pixels) of the generic table;
/// the view's proportions are kept (the height follows a given width, as in SketchUp's
/// dialog). `:antialias`, `:compression` and `:transparent` are not keys of `Model#export`
/// (only of `View#write_image`): SketchUp's table does not have them.
pub fn apply_raster(o: &mut RasterOptions, h: &Map<String, J>, view: [u32; 2]) {
    let px = |k: &str| h.get(k).filter(|v| v.is_i64()).and_then(J::as_i64).filter(|x| *x > 0).map(|x| x as u32);
    if let Some(hgt) = px("height") {
        o.use_view_size = false;
        o.width = crate::export2d::width_for_height(view, hgt);
    }
    if let Some(w) = px("width") {
        o.use_view_size = false;
        o.width = w;
    }
    if !o.use_view_size {
        o.height = crate::export2d::height_for_width(view, o.width);
    }
}

/// Lays the hash `h` of `Model#import` over the options `o` of the format `f`; `units` are
/// the model's ("model" units of an STL file).
pub fn apply_import(o: &mut AllOptions, f: ImportFormat, h: &Map<String, J>, units: Units) {
    use ImportFormat as I;
    use Typed::{Bool, Int};
    for (key, v) in read(h, IMPORT_KEYS, |s| import_string(s, f == I::Stl)) {
        match (f, key, v) {
            (I::Stl, "ImportMergeCoplanarFaces", Bool(b)) => o.stl_in.merge_coplanar = b,
            (I::Stl, "ImportPreserveOrigin", Bool(b)) => o.stl_in.preserve_origin = b,
            (I::Stl, "SwapYZ", Bool(b)) => o.stl_in.swap_yz = b,
            // 1 m, 2 cm, 3 mm, 4 ft, 5 in; 0 the model's units.
            (I::Stl, "ImportUnits", Int(i)) => {
                let u = match i {
                    0 => match units {
                        Units::Millimeters => Some(StlImportUnit::Millimeters),
                        Units::Centimeters => Some(StlImportUnit::Centimeters),
                        Units::Meters => Some(StlImportUnit::Meters),
                        Units::Inches => Some(StlImportUnit::Inches),
                        Units::Feet => Some(StlImportUnit::Feet),
                        Units::Yards => None,
                    },
                    i => pick(StlImportUnit::ALL, i - 1),
                };
                o.stl_in.units = u.unwrap_or(o.stl_in.units);
            }
            (I::ThreeDs, "ImportMergeCoplanarFaces", Bool(b)) => o.tds_in.merge_coplanar = b,
            (I::ThreeDs, "ImportUnits", Int(i)) => o.tds_in.units = pick(&LenUnit::ALL, i).unwrap_or(o.tds_in.units),
            (I::Dae, "ImportMergeCoplanarFaces", Bool(b)) => o.dae_in.merge_coplanar = b,
            (I::Dae, "ImportValidateDAE", Bool(b)) => o.dae_in.validate = b,
            (I::Kmz, "ImportMergeCoplanarFaces", Bool(b)) => o.kmz_in.0.merge_coplanar = b,
            (I::Kmz, "ImportValidateKmz", Bool(b)) => o.kmz_in.0.validate = b,
            (I::Dem, "ImportGenerateTexture", Bool(b)) => o.dem_in.gradient_texture = b,
            (I::Dem, "ImportMaxPoints", Int(i)) if i >= 4 => o.dem_in.max_points = i as u32,
            (I::Dwg, "ImportMergeCoplanarFaces", Bool(b)) => o.dwg_in.merge_coplanar = b,
            (I::Dwg, "ImportOrientFaces", Bool(b)) => o.dwg_in.orient_faces = b,
            (I::Dwg, "ImportMaterials", Bool(b)) => o.dwg_in.materials = b,
            (I::Dwg, "ImportPreserveOrigin", Bool(b)) => o.dwg_in.preserve_origin = b,
            (I::Dwg, "ImportUnits", Int(i)) => o.dwg_in.units = pick(&LenUnit::ALL, i).unwrap_or(o.dwg_in.units),
            (I::Glb, "ImportPreserveOrigin", Bool(b)) => o.glb_in.0.preserve_origin = b,
            (I::Usdz, "ImportPreserveOrigin", Bool(b)) => o.usdz_in.0.preserve_origin = b,
            (I::Ifc, "ImportPreserveOrigin", Bool(b)) => o.ifc_in.preserve_origin = b,
            (I::Ifc, "ImportTagsOption", Int(i)) => o.ifc_in.tags = pick(IfcTags::ALL, i).unwrap_or(o.ifc_in.tags),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hash(v: J) -> Map<String, J> {
        v.as_object().cloned().unwrap()
    }

    #[test]
    fn keys_types_and_names_as_sketchup_reads_them() {
        let mut o = AllOptions::default();
        let r = apply_export(
            &mut o,
            ExportFormat::Obj,
            &hash(json!({
                "triangulated_faces": true, "edges": 1, "swap_yz": false, "units": "m",
                "texture_maps": "yes", "colour": true, "selectionset_only": true,
            })),
        );
        assert!(o.obj.triangulate && !o.obj.swap_yz && o.obj.units == LenUnit::Meters);
        // An Integer for a boolean option, a String no table knows, an unknown key: nothing.
        assert!(!o.obj.edges && o.obj.texture_maps);
        assert!(r.selection_only);
        // STL has its own names for units and formats.
        apply_export(&mut o, ExportFormat::Stl, &hash(json!({"units": "inch", "format": "ascii", "swap_yz": true})));
        assert_eq!((o.stl.units, o.stl.format, o.stl.swap_yz), (StlUnit::Inches, StlFormat::Ascii, true));
        apply_export(&mut o, ExportFormat::Stl, &hash(json!({"units": "m", "format": 1})));
        assert_eq!((o.stl.units, o.stl.format), (StlUnit::Meters, StlFormat::Binary));
        // 3DS: names of the hierarchy and of the two-sided faces; "faces" wins over "doublesided_faces".
        apply_export(
            &mut o,
            ExportFormat::ThreeDs,
            &hash(json!({"geometry": "by_material", "faces": "two_sided_as_geometry", "doublesided_faces": false,
                          "preserve_texture_coords": false, "units": 5})),
        );
        let t = &o.tds;
        assert_eq!((t.hierarchy, t.two_sided, t.two_sided_as), (Hierarchy3ds::ByMaterial, true, TwoSidedAs::Geometry));
        assert_eq!((t.favor, t.units), (Favor3ds::WeldVertices, LenUnit::Millimeters));
        // AutoCAD: the names stop at 2013, an Integer reaches 2018.
        apply_export(&mut o, ExportFormat::Dwg, &hash(json!({"acad_version": "acad_2013", "faces_flag": false})));
        assert_eq!((o.acad.version, o.acad.faces), (AcadVersion::V2013, false));
        apply_export(&mut o, ExportFormat::Dxf, &hash(json!({"acad_version": 8})));
        assert_eq!(o.acad.version, AcadVersion::V2018);
        let r = apply_export(&mut o, ExportFormat::Dxf, &hash(json!({"type": "2d"})));
        assert_eq!(r.acad_type, 1);
        // VRML: "doublesided_faces" is the opposite of "Ignorer la matière du verso".
        apply_export(&mut o, ExportFormat::Vrml, &hash(json!({"doublesided_faces": false})));
        assert!(o.vrml.ignore_back_material);
        apply_export(
            &mut o,
            ExportFormat::Ifc,
            &hash(json!({"ifc_version": "IFC 2x3", "tessellated_geometry": false})),
        );
        assert_eq!((o.ifc.version, o.ifc.representation), (IfcVersion::Ifc2x3, IfcRepresentation::Body));
    }

    #[test]
    fn import_keys() {
        let mut o = AllOptions::default();
        let h = hash(json!({"merge_coplaner_faces": true, "units": "cm", "preserve_origin": false, "swap_yz": true}));
        apply_import(&mut o, ImportFormat::Stl, &h, Units::Meters);
        assert!(o.stl_in.merge_coplanar && !o.stl_in.preserve_origin && o.stl_in.swap_yz);
        assert_eq!(o.stl_in.units, StlImportUnit::Centimeters);
        apply_import(&mut o, ImportFormat::Stl, &hash(json!({"units": "model"})), Units::Inches);
        assert_eq!(o.stl_in.units, StlImportUnit::Inches);
        apply_import(&mut o, ImportFormat::ThreeDs, &hash(json!({"units": "cm"})), Units::Meters);
        assert_eq!(o.tds_in.units, LenUnit::Centimeters);
        apply_import(&mut o, ImportFormat::Ifc, &hash(json!({"tags_handling": "use_ifc_layers"})), Units::Meters);
        assert_eq!(o.ifc_in.tags, IfcTags::IfcLayers);
        apply_import(
            &mut o,
            ImportFormat::Dem,
            &hash(json!({"max_points": 1000, "generate_texture": true})),
            Units::Meters,
        );
        assert_eq!((o.dem_in.max_points, o.dem_in.gradient_texture), (1000, true));
        // Other formats' options are left alone.
        assert_eq!(o.dae_in, AllOptions::default().dae_in);
    }
}
