//! File formats: SketchUp's shipped importers/exporters (`resources/fr/*_importer.strings`,
//! `*_exporter.strings`, image importer classes of SketchUp.exe) and the .skp format itself.

use super::Impl::{self, *};
use super::Item;
use crate::commands::Cmd as C;
use esq_formats::ExportFormat as E;

const fn it(id: &'static str, label: &'static str, imp: Impl) -> Item {
    Item { id, label, imp }
}

pub const TABLE: &[Item] = &[
    // --- the native format
    it("skp:open", "Ouvrir un .skp (toutes versions)", Cmd(C::Open)),
    it("skp:save", "Enregistrer en .skp", Cmd(C::Save)),
    it("skp:save-older", "Enregistrer dans une version antérieure de SketchUp", Cmd(C::SaveAs)),
    it(
        "skp:fidelity",
        "Aller-retour .skp sans perte (textures, styles, scènes, attributs, cotes…)",
        Feature("esq-skp: census::sketchup_files_go_through_esquisse_without_loss, tests/fidelity.rs"),
    ),
    it("skp:template", "Modèles types (.skp) de SketchUp", Cmd(C::NewFromTemplate)),
    it("skp:component-file", "Insérer un .skp comme composant", Cmd(C::Import)),
    // --- 3D import
    it(
        "import:3ds",
        "Importer 3DS",
        Feature(
            "esq_formats::threeds::import (test: threeds::tests::export_then_import_keeps_geometry_hierarchy_and_materials)",
        ),
    ),
    it(
        "import:dae",
        "Importer COLLADA (.dae)",
        Feature("esq_formats::dae::import (test: dae::tests::reads_y_up_polylists_and_textures)"),
    ),
    it(
        "import:dem",
        "Importer MNT (.dem, .ddf)",
        Feature("esq_formats::dem::import (test: dem::tests::terrain_with_gradient_texture_and_report)"),
    ),
    it(
        "import:dwg",
        "Importer AutoCAD (.dwg, .dxf)",
        Feature("esq_formats::cad::import (test: cad::tests::reads_layers_faces_lines_circles_and_blocks)"),
    ),
    it(
        "import:glb",
        "Importer glTF (.glb, .gltf)",
        Feature("esq_formats::gltf::import (test: gltf::tests::glb_round_trip)"),
    ),
    it(
        "import:ifc",
        "Importer IFC (.ifc, .ifczip)",
        Feature("esq_formats::ifc::import (test: ifc::tests::reads_extrusions_mapped_items_and_openings)"),
    ),
    it(
        "import:kmz",
        "Importer Google Earth (.kmz)",
        Feature("esq_formats::kmz::import (test: kmz::tests::round_trip_through_google_earth_package)"),
    ),
    it(
        "import:stl",
        "Importer STL",
        Feature("esq_formats::stl::import (test: tests::formats3d::export_selection_only_then_import_with_the_cursor)"),
    ),
    it(
        "import:trb",
        "Importer Trimble (.trb)",
        Feature("esq_formats::trb::import (test: trb::tests::components_in_metres_with_materials_and_tags)"),
    ),
    it(
        "import:usdz",
        "Importer USD (.usdz, .usd)",
        Feature("esq_formats::usd::import (test: usd::tests::usdz_round_trip)"),
    ),
    // --- image import (as image, texture or Match Photo)
    it("import:bmp", "Importer une image BMP", Cmd(C::Import)),
    it("import:jpg", "Importer une image JPEG", Cmd(C::Import)),
    it("import:png", "Importer une image PNG", Cmd(C::Import)),
    it("import:psd", "Importer une image PSD", Cmd(C::Import)),
    it("import:tga", "Importer une image TGA", Cmd(C::Import)),
    it("import:tif", "Importer une image TIFF", Cmd(C::Import)),
    it("import:pdf", "Importer un PDF (comme image)", Cmd(C::Import)),
    // --- 3D export
    it("export:3ds", "Exporter 3DS", Cmd(C::ExportAs(E::ThreeDs))),
    it("export:dae", "Exporter COLLADA", Cmd(C::ExportAs(E::Dae))),
    it("export:dwg3d", "Exporter DWG/DXF 3D", Cmd(C::ExportAs(E::Dwg))),
    it("export:fbx", "Exporter FBX", Cmd(C::ExportAs(E::Fbx))),
    it("export:ifc", "Exporter IFC", Cmd(C::ExportAs(E::Ifc))),
    it("export:kmz", "Exporter KMZ", Cmd(C::ExportAs(E::Kmz))),
    it("export:obj", "Exporter OBJ (+ .mtl et textures)", Cmd(C::ExportAs(E::Obj))),
    it("export:stl", "Exporter STL", Cmd(C::ExportAs(E::Stl))),
    it("export:vrml", "Exporter VRML (.wrl)", Cmd(C::ExportAs(E::Vrml))),
    it("export:xsi", "Exporter Softimage XSI", Cmd(C::ExportAs(E::Xsi))),
    it("export:glb", "Exporter glTF/GLB, USDZ (exportateur SimLab)", Cmd(C::ExportAs(E::Glb))),
    // --- 2D export
    it("export2d:png", "Exporter l'image PNG", Cmd(C::ExportImage)),
    it("export2d:jpg", "Exporter l'image JPEG", Cmd(C::ExportImage)),
    it("export2d:bmp", "Exporter l'image BMP", Cmd(C::ExportImage)),
    it("export2d:tif", "Exporter l'image TIFF", Cmd(C::ExportImage)),
    it("export2d:pdf", "Exporter en PDF vectoriel", Cmd(C::ExportImage)),
    it("export2d:eps", "Exporter en EPS", Cmd(C::ExportImage)),
    it("export2d:dwg", "Exporter DWG/DXF 2D (lignes cachées)", Cmd(C::ExportImage)),
    it("export2d:section", "Exporter la tranche de section (DWG/DXF/SVG)", Cmd(C::ExportSlice)),
    it("export2d:animation", "Exporter l'animation (MP4, WebM, AVI, séquence d'images)", Cmd(C::ExportAnimation)),
    it("export2d:print", "Imprimer / aperçu (échelle, mosaïque)", Cmd(C::Print)),
];
