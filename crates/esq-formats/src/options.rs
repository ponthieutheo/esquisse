//! The options of SketchUp's import and export dialogs, format by format: same settings,
//! same French labels (from the `*_importer.strings` / `*_exporter.strings` of SketchUp
//! 2026), same defaults. The dialogs are drawn by the application from [`Field`] lists.

use serde::{Deserialize, Serialize};

use crate::units::LenUnit;

/// A choice among labelled values (combo box or radio buttons).
pub trait Pick {
    fn labels(&self) -> Vec<&'static str>;
    fn index(&self) -> usize;
    fn set_index(&mut self, i: usize);
}

/// One line of an options dialog.
pub enum Field<'a> {
    /// A titled group box.
    Group(&'static str),
    Check(&'static str, &'a mut bool),
    /// Drop-down list.
    Combo(&'static str, &'a mut dyn Pick),
    /// Radio buttons.
    Radio(&'static str, &'a mut dyn Pick),
    /// A whole number in a range.
    Int(&'static str, &'a mut u32, u32, u32),
    /// A remark under the previous control.
    Note(&'static str),
    /// A read-only value computed from the others.
    Value(&'static str, String),
}

macro_rules! pick_enum {
    ($name:ident { $($var:ident => $label:expr),* $(,)? }) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
        pub enum $name { #[default] $($var),* }
        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$var),*];
            pub fn label(self) -> &'static str { match self { $($name::$var => $label),* } }
        }
        impl Pick for $name {
            fn labels(&self) -> Vec<&'static str> { Self::ALL.iter().map(|x| x.label()).collect() }
            fn index(&self) -> usize { Self::ALL.iter().position(|x| x == self).unwrap_or(0) }
            fn set_index(&mut self, i: usize) { if let Some(x) = Self::ALL.get(i) { *self = *x; } }
        }
    };
}

impl Pick for LenUnit {
    fn labels(&self) -> Vec<&'static str> {
        LenUnit::ALL.iter().map(|x| x.label()).collect()
    }
    fn index(&self) -> usize {
        LenUnit::ALL.iter().position(|x| x == self).unwrap_or(0)
    }
    fn set_index(&mut self, i: usize) {
        if let Some(x) = LenUnit::ALL.get(i) {
            *self = *x;
        }
    }
}

// skp2stl.dll fills the list in this order; ExportUnits = 0 (model units).
pick_enum!(StlUnit {
    Model => "Unités du modèle",
    Meters => "Mètres",
    Centimeters => "Centimètres",
    Millimeters => "Millimètres",
    Feet => "Pieds",
    Inches => "Pouces",
});

impl StlUnit {
    pub fn len_unit(self) -> LenUnit {
        match self {
            StlUnit::Model => LenUnit::Model,
            StlUnit::Inches => LenUnit::Inches,
            StlUnit::Feet => LenUnit::Feet,
            StlUnit::Millimeters => LenUnit::Millimeters,
            StlUnit::Centimeters => LenUnit::Centimeters,
            StlUnit::Meters => LenUnit::Meters,
        }
    }
}

// The units of an STL file being imported (the file does not say).
pick_enum!(StlImportUnit {
    Meters => "Mètres",
    Centimeters => "Centimètres",
    Millimeters => "Millimètres",
    Feet => "Pieds",
    Inches => "Pouces",
});

impl StlImportUnit {
    pub fn mm(self) -> f64 {
        match self {
            StlImportUnit::Inches => 25.4,
            StlImportUnit::Feet => 304.8,
            StlImportUnit::Millimeters => 1.0,
            StlImportUnit::Centimeters => 10.0,
            StlImportUnit::Meters => 1000.0,
        }
    }
}

// The list is "Ascii", "Binaire"; ExportFormat = 1 (binary) by default.
pick_enum!(StlFormat { Ascii => "Ascii", Binary => "Binaire" });

pick_enum!(Hierarchy3ds {
    FullHierarchy => "Hiérarchie complète",
    ByTag => "Par étiquette",
    ByMaterial => "Par matière",
    SingleObject => "Objet unique",
});

pick_enum!(TwoSidedAs { Materials => "Matières", Geometry => "Géométrie" });

pick_enum!(Favor3ds {
    PreserveTexCoords => "Maintien des coordonnées de texture",
    WeldVertices => "Soudure de sommets",
});

pick_enum!(AcadVersion {
    R12 => "Version 12",
    R13 => "Version 13",
    R14 => "Version 14",
    V2000 => "AutoCAD 2000",
    V2004 => "AutoCAD 2004",
    V2007 => "AutoCAD 2007",
    V2010 => "AutoCAD 2010",
    V2013 => "AutoCAD 2013",
    V2018 => "AutoCAD 2018",
});

impl AcadVersion {
    /// The release written: `ExportAcadVersion` is the index of this list in `skp2acad`.
    pub fn dwg(self) -> esq_core::dwg::DwgVersion {
        esq_core::dwg::DwgVersion::from_code(self.index()).unwrap_or_default()
    }
}

pick_enum!(IfcVersion { Ifc2x3 => "IFC 2x3", Ifc4 => "IFC 4" });

pick_enum!(IfcRepresentation { Tessellation => "Tessellation", Body => "Corps" });

pick_enum!(IfcTags {
    IfcClasses => "Utiliser les classes IFC",
    IfcLayers => "Utiliser les calques IFC",
    NoTags => "Ne pas créer d'étiquettes",
});

/// Options of one format's dialog.
pub trait FormatOptions {
    /// Title of the dialog ("Options d'exportation OBJ"…).
    fn title(&self) -> &'static str;
    fn fields(&mut self) -> Vec<Field<'_>>;
}

// -------------------------------------------------------------------------------------
// Export

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ObjExport {
    pub selection_only: bool,
    pub triangulate: bool,
    pub two_sided: bool,
    pub edges: bool,
    pub texture_maps: bool,
    pub swap_yz: bool,
    pub units: LenUnit,
}

impl Default for ObjExport {
    fn default() -> Self {
        ObjExport {
            selection_only: false,
            triangulate: false,
            two_sided: false,
            edges: false,
            texture_maps: true,
            swap_yz: true,
            units: LenUnit::Model,
        }
    }
}

impl FormatOptions for ObjExport {
    fn title(&self) -> &'static str {
        "Options d'exportation OBJ"
    }
    fn fields(&mut self) -> Vec<Field<'_>> {
        vec![
            Field::Group("Géométrie"),
            Field::Check("Exporter uniquement la sélection", &mut self.selection_only),
            Field::Check("Trianguler toutes les faces", &mut self.triangulate),
            Field::Check("Exporter les faces recto verso", &mut self.two_sided),
            Field::Check("Exporter les arêtes", &mut self.edges),
            Field::Group("Matières"),
            Field::Check("Exporter les placages de texture", &mut self.texture_maps),
            Field::Group("Échelle"),
            Field::Check("Permuter les coordonnées YZ (Z vertical)", &mut self.swap_yz),
            Field::Combo("Unités", &mut self.units),
        ]
    }
}

/// XSI has the same dialog as OBJ.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct XsiExport(pub ObjExport);

impl Default for XsiExport {
    fn default() -> Self {
        XsiExport(ObjExport { swap_yz: false, ..ObjExport::default() })
    }
}

impl FormatOptions for XsiExport {
    fn title(&self) -> &'static str {
        "Options d'exportation XSI"
    }
    fn fields(&mut self) -> Vec<Field<'_>> {
        self.0.fields()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FbxExport {
    pub selection_only: bool,
    pub triangulate: bool,
    pub two_sided: bool,
    pub separate_disconnected: bool,
    pub texture_maps: bool,
    pub swap_yz: bool,
    pub units: LenUnit,
}

impl Default for FbxExport {
    fn default() -> Self {
        FbxExport {
            selection_only: false,
            triangulate: false,
            two_sided: false,
            separate_disconnected: false,
            texture_maps: true,
            swap_yz: true,
            units: LenUnit::Model,
        }
    }
}

impl FormatOptions for FbxExport {
    fn title(&self) -> &'static str {
        "Options d'exportation FBX"
    }
    fn fields(&mut self) -> Vec<Field<'_>> {
        vec![
            Field::Group("Géométrie"),
            Field::Check("Exporter uniquement la sélection", &mut self.selection_only),
            Field::Check("Trianguler toutes les faces", &mut self.triangulate),
            Field::Check("Exporter les faces recto verso", &mut self.two_sided),
            Field::Check("Séparer les faces déconnectées", &mut self.separate_disconnected),
            Field::Group("Matières"),
            Field::Check("Exporter les placages de texture", &mut self.texture_maps),
            Field::Group("Échelle"),
            Field::Check("Permuter les coordonnées YZ (Z vertical)", &mut self.swap_yz),
            Field::Combo("Unités", &mut self.units),
        ]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StlExport {
    pub selection_only: bool,
    pub units: StlUnit,
    pub swap_yz: bool,
    pub format: StlFormat,
}

impl Default for StlExport {
    fn default() -> Self {
        StlExport { selection_only: false, units: StlUnit::Model, swap_yz: false, format: StlFormat::Binary }
    }
}

impl FormatOptions for StlExport {
    fn title(&self) -> &'static str {
        "Options d'exportation de STL"
    }
    fn fields(&mut self) -> Vec<Field<'_>> {
        vec![
            Field::Group("Géométrie"),
            Field::Check("Exporter uniquement la sélection actuelle", &mut self.selection_only),
            Field::Group("Échelle"),
            Field::Combo("Unités STL", &mut self.units),
            Field::Check("Permuter les coordonnées YZ (Z vertical)", &mut self.swap_yz),
            Field::Group("Format de fichier"),
            Field::Combo("Format de fichier", &mut self.format),
        ]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThreeDsExport {
    pub hierarchy: Hierarchy3ds,
    pub selection_only: bool,
    pub two_sided: bool,
    pub two_sided_as: TwoSidedAs,
    pub standalone_edges: bool,
    pub texture_maps: bool,
    pub favor: Favor3ds,
    pub cameras: bool,
    pub units: LenUnit,
}

impl Default for ThreeDsExport {
    fn default() -> Self {
        ThreeDsExport {
            hierarchy: Hierarchy3ds::FullHierarchy,
            selection_only: false,
            two_sided: false,
            two_sided_as: TwoSidedAs::Materials,
            standalone_edges: true,
            texture_maps: true,
            favor: Favor3ds::PreserveTexCoords,
            cameras: true,
            units: LenUnit::Model,
        }
    }
}

impl FormatOptions for ThreeDsExport {
    fn title(&self) -> &'static str {
        "Options d'exportation de fichier 3DS"
    }
    fn fields(&mut self) -> Vec<Field<'_>> {
        vec![
            Field::Group("Géométrie"),
            Field::Combo("Exporter", &mut self.hierarchy),
            Field::Check("Exporter uniquement la sélection", &mut self.selection_only),
            Field::Check("Exporter les faces recto verso", &mut self.two_sided),
            Field::Radio("Comme", &mut self.two_sided_as),
            Field::Check("Exporter les arêtes indépendantes", &mut self.standalone_edges),
            Field::Group("Matières"),
            Field::Check("Exporter les placages de texture", &mut self.texture_maps),
            Field::Radio("Favoriser", &mut self.favor),
            Field::Group("Caméras"),
            Field::Check("Générer des caméras à partir des pages", &mut self.cameras),
            Field::Group("Échelle"),
            Field::Combo("Unités :", &mut self.units),
        ]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DaeExport {
    pub two_sided: bool,
    pub edges: bool,
    pub triangulate: bool,
    pub selection_only: bool,
    pub hidden: bool,
    pub preserve_hierarchies: bool,
    pub texture_maps: bool,
    pub credits: bool,
}

impl Default for DaeExport {
    fn default() -> Self {
        DaeExport {
            two_sided: false,
            edges: false,
            triangulate: true,
            selection_only: false,
            hidden: false,
            preserve_hierarchies: true,
            texture_maps: true,
            credits: false,
        }
    }
}

impl FormatOptions for DaeExport {
    fn title(&self) -> &'static str {
        "Options d'exportation DAE"
    }
    fn fields(&mut self) -> Vec<Field<'_>> {
        vec![
            Field::Group("Géométrie"),
            Field::Check("Exporter les faces recto verso", &mut self.two_sided),
            Field::Check("Exporter les arêtes", &mut self.edges),
            Field::Check("Trianguler toutes les faces", &mut self.triangulate),
            Field::Check("Exporter uniquement l'ensemble sélectionné", &mut self.selection_only),
            Field::Check("Exporter la géométrie cachée", &mut self.hidden),
            Field::Check("Conserver les hiérarchies de composants", &mut self.preserve_hierarchies),
            Field::Group("Matières"),
            Field::Check("Exporter les placages de texture", &mut self.texture_maps),
            Field::Group("Crédits"),
            Field::Check("Conserver les crédits", &mut self.credits),
        ]
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct KmzExport {
    pub hidden: bool,
    pub credits: bool,
}

impl FormatOptions for KmzExport {
    fn title(&self) -> &'static str {
        "Options d'exportation KMZ"
    }
    fn fields(&mut self) -> Vec<Field<'_>> {
        vec![
            Field::Group("Géométrie"),
            Field::Check("Exporter la géométrie cachée", &mut self.hidden),
            Field::Group("Crédits"),
            Field::Check("Conserver les crédits", &mut self.credits),
        ]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VrmlExport {
    pub texture_maps: bool,
    pub ignore_back_material: bool,
    pub edges: bool,
    pub vrml_orientation: bool,
    pub cameras: bool,
    pub allow_mirrored: bool,
    pub material_overrides: bool,
}

impl Default for VrmlExport {
    fn default() -> Self {
        VrmlExport {
            texture_maps: true,
            ignore_back_material: false,
            edges: true,
            vrml_orientation: false,
            cameras: true,
            allow_mirrored: false,
            material_overrides: true,
        }
    }
}

impl FormatOptions for VrmlExport {
    fn title(&self) -> &'static str {
        "Options d'exportation de fichier VRML"
    }
    fn fields(&mut self) -> Vec<Field<'_>> {
        vec![
            Field::Group("Apparence"),
            Field::Check("Exporter des placages de texture", &mut self.texture_maps),
            Field::Check("Ignorer la matière du verso des faces", &mut self.ignore_back_material),
            Field::Check("Exporter les arêtes", &mut self.edges),
            Field::Check("Utiliser l'orientation standard VRML", &mut self.vrml_orientation),
            Field::Check("Générer des caméras", &mut self.cameras),
            Field::Check("Autoriser les composants créés par symétrie", &mut self.allow_mirrored),
            Field::Check("Vérifier s'il y a des substitutions de matières", &mut self.material_overrides),
        ]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AcadExport {
    pub version: AcadVersion,
    pub faces: bool,
    pub edges: bool,
    pub construction: bool,
    pub dimensions: bool,
    pub text: bool,
    pub materials: bool,
}

impl Default for AcadExport {
    fn default() -> Self {
        AcadExport {
            version: AcadVersion::V2000,
            faces: true,
            edges: true,
            construction: false,
            dimensions: true,
            text: true,
            materials: true,
        }
    }
}

impl FormatOptions for AcadExport {
    fn title(&self) -> &'static str {
        "Options d'exportation DWG/DXF"
    }
    fn fields(&mut self) -> Vec<Field<'_>> {
        vec![
            Field::Combo("Version AutoCAD", &mut self.version),
            Field::Group("Exporter:"),
            Field::Check("Faces", &mut self.faces),
            Field::Check("Arêtes", &mut self.edges),
            Field::Check("Géométrie de construction", &mut self.construction),
            Field::Check("Cotations", &mut self.dimensions),
            Field::Check("Texte", &mut self.text),
            Field::Check("Matières", &mut self.materials),
        ]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IfcExport {
    pub version: IfcVersion,
    pub standard_hierarchy: bool,
    pub selection_only: bool,
    pub hidden: bool,
    pub representation: IfcRepresentation,
}

impl Default for IfcExport {
    fn default() -> Self {
        IfcExport {
            version: IfcVersion::Ifc4,
            standard_hierarchy: false,
            selection_only: false,
            hidden: false,
            representation: IfcRepresentation::Tessellation,
        }
    }
}

impl FormatOptions for IfcExport {
    fn title(&self) -> &'static str {
        "Options d’exportation"
    }
    fn fields(&mut self) -> Vec<Field<'_>> {
        vec![
            Field::Group("Version"),
            Field::Combo("Version", &mut self.version),
            Field::Group("Exporter"),
            Field::Check("Hiérarchie spatiale standard IFC", &mut self.standard_hierarchy),
            Field::Group("Géométrie"),
            Field::Check("Exporter uniquement l'ensemble de sélections", &mut self.selection_only),
            Field::Check("Exporter la géométrie cachée", &mut self.hidden),
            Field::Group("Représentation"),
            Field::Radio("Représentation", &mut self.representation),
        ]
    }
}

// -------------------------------------------------------------------------------------
// Import

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StlImport {
    pub merge_coplanar: bool,
    pub preserve_origin: bool,
    pub units: StlImportUnit,
    pub swap_yz: bool,
}

impl Default for StlImport {
    fn default() -> Self {
        StlImport { merge_coplanar: false, preserve_origin: true, units: StlImportUnit::Millimeters, swap_yz: false }
    }
}

impl FormatOptions for StlImport {
    fn title(&self) -> &'static str {
        "Options d'importation de STL"
    }
    fn fields(&mut self) -> Vec<Field<'_>> {
        vec![
            Field::Group("Géométrie"),
            Field::Check("Fusionner les faces coplanaires", &mut self.merge_coplanar),
            Field::Check("Préserver l’origine du dessin", &mut self.preserve_origin),
            Field::Group("Échelle"),
            Field::Combo("Unités Stl :", &mut self.units),
            Field::Check("Permuter les coordonnées YZ (Z vertical)", &mut self.swap_yz),
        ]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThreeDsImport {
    pub merge_coplanar: bool,
    pub units: LenUnit,
}

impl Default for ThreeDsImport {
    fn default() -> Self {
        ThreeDsImport { merge_coplanar: false, units: LenUnit::Model }
    }
}

impl FormatOptions for ThreeDsImport {
    fn title(&self) -> &'static str {
        "Options d'importation de fichier 3DS"
    }
    fn fields(&mut self) -> Vec<Field<'_>> {
        vec![
            Field::Group("Géométrie"),
            Field::Check("Fusionner les faces coplanaires", &mut self.merge_coplanar),
            Field::Group("Échelle"),
            Field::Combo("Unités :", &mut self.units),
        ]
    }
}

/// DAE and KMZ have the same dialog.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DaeImport {
    pub validate: bool,
    pub merge_coplanar: bool,
}

impl Default for DaeImport {
    fn default() -> Self {
        DaeImport { validate: true, merge_coplanar: true }
    }
}

impl FormatOptions for DaeImport {
    fn title(&self) -> &'static str {
        "Options d'importation de fichier DAE"
    }
    fn fields(&mut self) -> Vec<Field<'_>> {
        vec![
            Field::Check("Valider le fichier DAE", &mut self.validate),
            Field::Check("Fusionner les faces coplanaires", &mut self.merge_coplanar),
        ]
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct KmzImport(pub DaeImport);

impl FormatOptions for KmzImport {
    fn title(&self) -> &'static str {
        "Options d’importation KMZ"
    }
    fn fields(&mut self) -> Vec<Field<'_>> {
        vec![
            Field::Check("Valider le fichier KMZ", &mut self.0.validate),
            Field::Check("Fusionner les faces coplanaires", &mut self.0.merge_coplanar),
        ]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DemImport {
    /// Most points kept for the triangulated irregular network.
    pub max_points: u32,
    pub gradient_texture: bool,
}

impl Default for DemImport {
    fn default() -> Self {
        DemImport { max_points: 250, gradient_texture: false }
    }
}

impl FormatOptions for DemImport {
    fn title(&self) -> &'static str {
        "Options d'importation de fichier DEM"
    }
    fn fields(&mut self) -> Vec<Field<'_>> {
        // What the triangulation of that many points gives (2 n − 6).
        let faces = (2 * self.max_points as i64 - 6).max(0).to_string();
        vec![
            Field::Group("TIN"),
            Field::Int("Points :", &mut self.max_points, 4, 1_000_000),
            Field::Value("Faces :", faces),
            Field::Group("Couleur"),
            Field::Check("Générer un dégradé de texture", &mut self.gradient_texture),
        ]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DwgImport {
    pub merge_coplanar: bool,
    pub orient_faces: bool,
    pub flatten: bool,
    pub group_tag_elements: bool,
    pub break_edges: bool,
    pub materials: bool,
    pub preserve_origin: bool,
    pub units: LenUnit,
}

impl Default for DwgImport {
    fn default() -> Self {
        DwgImport {
            merge_coplanar: false,
            orient_faces: false,
            flatten: false,
            group_tag_elements: false,
            break_edges: false,
            materials: true,
            preserve_origin: false,
            units: LenUnit::Model,
        }
    }
}

impl FormatOptions for DwgImport {
    fn title(&self) -> &'static str {
        "Options d'importation de fichier AutoCAD DWG/DXF"
    }
    fn fields(&mut self) -> Vec<Field<'_>> {
        vec![
            Field::Group("Géométrie"),
            Field::Check("Fusionner les faces coplanaires", &mut self.merge_coplanar),
            Field::Check("Orienter les faces de façon cohérente", &mut self.orient_faces),
            Field::Check("Importer un dessin au trait aplati", &mut self.flatten),
            Field::Check("Importer des couches en tant que groupes", &mut self.group_tag_elements),
            Field::Check("Rompre les arêtes", &mut self.break_edges),
            Field::Check("Importer des matières", &mut self.materials),
            Field::Group("Position"),
            Field::Check("Préserver l'origine du dessin", &mut self.preserve_origin),
            Field::Note(
                "Attention : des résultats non désirés peuvent se produire lors de l'utilisation de cette option",
            ),
            Field::Group("Échelle"),
            Field::Combo("Unités :", &mut self.units),
        ]
    }
}

/// glTF and USDZ (SimLab importers): "Préserver l'origine du dessin" is ticked by default.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OriginImport {
    pub preserve_origin: bool,
}

impl Default for OriginImport {
    fn default() -> Self {
        OriginImport { preserve_origin: true }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GlbImport(pub OriginImport);

impl FormatOptions for GlbImport {
    fn title(&self) -> &'static str {
        "Options d’importation de fichier GLB"
    }
    fn fields(&mut self) -> Vec<Field<'_>> {
        vec![Field::Group("Échelle"), Field::Check("Préserver l’origine du dessin", &mut self.0.preserve_origin)]
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UsdzImport(pub OriginImport);

impl FormatOptions for UsdzImport {
    fn title(&self) -> &'static str {
        "Options d'importation de fichier USDZ"
    }
    fn fields(&mut self) -> Vec<Field<'_>> {
        vec![Field::Group("Échelle"), Field::Check("Préserver l’origine du dessin", &mut self.0.preserve_origin)]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IfcImport {
    pub preserve_origin: bool,
    pub tags: IfcTags,
}

impl Default for IfcImport {
    fn default() -> Self {
        IfcImport { preserve_origin: true, tags: IfcTags::IfcClasses }
    }
}

impl FormatOptions for IfcImport {
    fn title(&self) -> &'static str {
        "Options d'importation d'IFC"
    }
    fn fields(&mut self) -> Vec<Field<'_>> {
        vec![
            Field::Group("Position"),
            Field::Check("Préserver l'origine du dessin", &mut self.preserve_origin),
            Field::Combo("Étiquettes", &mut self.tags),
        ]
    }
}

/// Every option of every format, as saved between sessions.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AllOptions {
    pub obj: ObjExport,
    pub xsi: XsiExport,
    pub fbx: FbxExport,
    pub stl: StlExport,
    pub tds: ThreeDsExport,
    pub dae: DaeExport,
    pub kmz: KmzExport,
    pub vrml: VrmlExport,
    pub acad: AcadExport,
    pub ifc: IfcExport,
    pub stl_in: StlImport,
    pub tds_in: ThreeDsImport,
    pub dae_in: DaeImport,
    pub kmz_in: KmzImport,
    pub dem_in: DemImport,
    pub dwg_in: DwgImport,
    pub glb_in: GlbImport,
    pub usdz_in: UsdzImport,
    pub ifc_in: IfcImport,
}

impl AllOptions {
    /// The options dialog of an import format (none for TrimBIM).
    pub fn import_options(&mut self, f: crate::ImportFormat) -> Option<&mut dyn FormatOptions> {
        use crate::ImportFormat as I;
        Some(match f {
            I::Stl => &mut self.stl_in,
            I::ThreeDs => &mut self.tds_in,
            I::Dae => &mut self.dae_in,
            I::Kmz => &mut self.kmz_in,
            I::Dem => &mut self.dem_in,
            I::Dwg => &mut self.dwg_in,
            I::Glb => &mut self.glb_in,
            I::Usdz => &mut self.usdz_in,
            I::Ifc => &mut self.ifc_in,
            I::Trb => return None,
        })
    }

    /// The options dialog of an export format (none for the SimLab glTF / USDZ exporter).
    pub fn export_options(&mut self, f: crate::ExportFormat) -> Option<&mut dyn FormatOptions> {
        use crate::ExportFormat as E;
        Some(match f {
            E::Obj => &mut self.obj,
            E::Xsi => &mut self.xsi,
            E::Fbx => &mut self.fbx,
            E::Stl => &mut self.stl,
            E::ThreeDs => &mut self.tds,
            E::Dae => &mut self.dae,
            E::Kmz => &mut self.kmz,
            E::Vrml => &mut self.vrml,
            E::Dwg | E::Dxf => &mut self.acad,
            E::Ifc => &mut self.ifc,
            E::Glb | E::Usdz => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialogs_list_sketchups_settings() {
        let mut o = AllOptions::default();
        let count = |f: Vec<Field>| f.iter().filter(|x| !matches!(x, Field::Group(_) | Field::Note(_))).count();
        assert_eq!(count(o.obj.fields()), 7);
        assert_eq!(count(o.stl.fields()), 4);
        assert_eq!(count(o.tds.fields()), 9);
        assert_eq!(count(o.dae.fields()), 8);
        assert_eq!(count(o.kmz.fields()), 2);
        assert_eq!(count(o.fbx.fields()), 7);
        assert_eq!(count(o.vrml.fields()), 7);
        assert_eq!(count(o.stl_in.fields()), 4);
        assert_eq!(count(o.dwg_in.fields()), 8);
        let json = serde_json::to_string(&o).unwrap();
        let back: AllOptions = serde_json::from_str(&json).unwrap();
        assert_eq!(back, o);
        // Old or partial files keep the defaults for what they lack.
        let partial: AllOptions = serde_json::from_str(r#"{"obj":{"edges":true}}"#).unwrap();
        assert!(partial.obj.edges && partial.obj.texture_maps && !partial.obj.triangulate);
        let mut u = LenUnit::Model;
        u.set_index(7);
        assert_eq!(u, LenUnit::Meters);
    }
}
