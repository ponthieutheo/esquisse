//! The version maps (`CVersionMap`) SketchUp 2026 writes in the MFC archive of each older
//! version it saves (one per version): every class it
//! knows in that version and the schema of its `Serialize`, in this order. Read from the files
//! of each version the SDK writes (`SUModelSaveToFileWithVersion`); the same for every model.

pub type Map = &'static [(&'static str, u32)];

pub const V3: Map = &[
    ("CArcCurve", 1),
    ("CComponent", 11),
    ("CComponentBehavior", 3),
    ("CComponentDefinition", 10),
    ("CComponentInstance", 3),
    ("CConstructionGeometry", 0),
    ("CConstructionLine", 0),
    ("CConstructionPoint", 0),
    ("CCurve", 3),
    ("CDefinitionList", 0),
    ("CDib", 2),
    ("CDimension", 0),
    ("CDimensionLinear", 4),
    ("CDimensionRadial", 2),
    ("CDimensionStyle", 4),
    ("CDrawingElement", 8),
    ("CEdge", 2),
    ("CEdgeUse", 1),
    ("CEntity", 2),
    ("CFace", 3),
    ("CFontManager", 0),
    ("CGroup", 1),
    ("CImage", 1),
    ("CLayer", 1),
    ("CLayerManager", 4),
    ("CLoop", 1),
    ("CMaterial", 12),
    ("CMaterialManager", 4),
    ("CPolyline3d", 0),
    ("CRelationship", 0),
    ("CRelationshipMap", 0),
    ("CRenderingOptions", 22),
    ("CSectionPlane", 2),
    ("CShadowInfo", 7),
    ("CSkFont", 0),
    ("CSketchCS", 0),
    ("CSketchUpModel", 10),
    ("CSketchUpPage", 1),
    ("CText", 9),
    ("CTextStyle", 4),
    ("CTexture", 4),
    ("CThumbnail", 0),
    ("CVertex", 0),
    ("CViewPage", 6),
    ("CWatermark", 1),
];

/// SketchUp 4 and 5 differ by a few schemas.
const fn v45(instance: u32, de: u32, ro: u32) -> [(&'static str, u32); 51] {
    [
        ("CArcCurve", 1),
        ("CAttribute", 0),
        ("CAttributeContainer", 0),
        ("CAttributeNamed", 0),
        ("CCamera", 4),
        ("CComponent", 11),
        ("CComponentBehavior", 4),
        ("CComponentDefinition", 10),
        ("CComponentInstance", instance),
        ("CConstructionGeometry", 0),
        ("CConstructionLine", 1),
        ("CConstructionPoint", 0),
        ("CCurve", 4),
        ("CDefinitionList", 0),
        ("CDib", 2),
        ("CDimension", 0),
        ("CDimensionLinear", 4),
        ("CDimensionRadial", 2),
        ("CDimensionStyle", 4),
        ("CDrawingElement", de),
        ("CEdge", 2),
        ("CEdgeUse", 1),
        ("CEntity", 3),
        ("CFace", 3),
        ("CFaceTextureCoords", 4),
        ("CFontManager", 0),
        ("CGroup", 1),
        ("CImage", 1),
        ("CLayer", 2),
        ("CLayerManager", 4),
        ("CLoop", 1),
        ("CMaterial", 12),
        ("CMaterialManager", 4),
        ("CPageList", 1),
        ("CPolyline3d", 0),
        ("CRelationship", 0),
        ("CRelationshipMap", 0),
        ("CRenderingOptions", ro),
        ("CSectionPlane", 2),
        ("CShadowInfo", 7),
        ("CSkFont", 0),
        ("CSketchCS", 0),
        ("CSketchUpModel", 12),
        ("CSketchUpPage", 1),
        ("CText", 9),
        ("CTextStyle", 4),
        ("CTexture", 4),
        ("CThumbnail", 1),
        ("CVertex", 0),
        ("CViewPage", 9),
        ("CWatermark", 1),
    ]
}

pub const V4: Map = &v45(3, 8, 25);
pub const V5: Map = &v45(4, 9, 28);

/// The schemas that change from SketchUp 6 to 2013.
#[derive(Clone, Copy)]
struct S6 {
    arc: u32,
    attr_named: u32,
    background: u32,
    behavior: u32,
    ro: u32,
    model: u32,
    page: u32,
}

const fn v6_13(s: S6) -> [(&'static str, u32); 55] {
    [
        ("CArcCurve", s.arc),
        ("CAttribute", 0),
        ("CAttributeContainer", 0),
        ("CAttributeNamed", s.attr_named),
        ("CBackgroundImage", s.background),
        ("CCamera", 5),
        ("CComponent", 11),
        ("CComponentBehavior", s.behavior),
        ("CComponentDefinition", 10),
        ("CComponentInstance", 4),
        ("CConstructionGeometry", 0),
        ("CConstructionLine", 1),
        ("CConstructionPoint", 0),
        ("CCurve", 4),
        ("CDefinitionList", 0),
        ("CDib", 3),
        ("CDimension", 1),
        ("CDimensionLinear", 6),
        ("CDimensionRadial", 2),
        ("CDimensionStyle", 4),
        ("CDrawingElement", 9),
        ("CEdge", 2),
        ("CEdgeUse", 1),
        ("CEntity", 3),
        ("CFace", 3),
        ("CFaceTextureCoords", 4),
        ("CFontManager", 0),
        ("CGroup", 1),
        ("CImage", 1),
        ("CLayer", 2),
        ("CLayerManager", 4),
        ("CLoop", 1),
        ("CMaterial", 12),
        ("CMaterialManager", 4),
        ("CPageList", 1),
        ("CPolyline3d", 0),
        ("CRelationship", 0),
        ("CRelationshipMap", 0),
        ("CRenderingOptions", s.ro),
        ("CSectionPlane", 2),
        ("CShadowInfo", 7),
        ("CSkFont", 1),
        ("CSketchCS", 0),
        ("CSketchUpModel", s.model),
        ("CSketchUpPage", 1),
        ("CSkpStyle", 1),
        ("CSkpStyleManager", 2),
        ("CText", 9),
        ("CTextStyle", 5),
        ("CTexture", 6),
        ("CThumbnail", 1),
        ("CVertex", 0),
        ("CViewPage", s.page),
        ("CWatermark", 1),
        ("CWatermarkManager", 2),
    ]
}

pub const V6: Map = &v6_13(S6 { arc: 1, attr_named: 0, background: 1, behavior: 4, ro: 32, model: 18, page: 11 });
pub const V7: Map = &v6_13(S6 { arc: 1, attr_named: 1, background: 9, behavior: 5, ro: 35, model: 22, page: 11 });
pub const V8: Map = &v6_13(S6 { arc: 1, attr_named: 1, background: 10, behavior: 5, ro: 36, model: 22, page: 12 });
pub const V2013: Map = &v6_13(S6 { arc: 2, attr_named: 1, background: 10, behavior: 5, ro: 36, model: 22, page: 12 });

/// The schemas that change from SketchUp 2014 to 2020.
#[derive(Clone, Copy)]
struct S14 {
    arc: u32,
    definition: u32,
    instance: u32,
    entity: u32,
    layer: u32,
    ro: u32,
    section: u32,
    model: u32,
    style: u32,
    page: u32,
    line_style: u32,
    layer_mgr: u32,
}

/// SketchUp 2014 to 2020: the classes of 2013 and the classification files, then the line
/// styles (2019) and the tag folders (2020).
const fn v14_20(s: S14) -> [(&'static str, u32); 61] {
    [
        ("CArcCurve", s.arc),
        ("CAttribute", 0),
        ("CAttributeContainer", 0),
        ("CAttributeNamed", 1),
        ("CBackgroundImage", 10),
        ("CCamera", 5),
        ("CComponent", 11),
        ("CComponentBehavior", 5),
        ("CComponentDefinition", s.definition),
        ("CComponentInstance", s.instance),
        ("CConstructionGeometry", 0),
        ("CConstructionLine", 1),
        ("CConstructionPoint", 0),
        ("CCurve", 4),
        ("CCustomLineStyle", s.line_style),
        ("CDefinitionList", 0),
        ("CDib", 3),
        ("CDimension", 1),
        ("CDimensionLinear", 6),
        ("CDimensionRadial", 2),
        ("CDimensionStyle", 4),
        ("CDrawingElement", 9),
        ("CEdge", 2),
        ("CEdgeUse", 1),
        ("CEntity", s.entity),
        ("CFace", 3),
        ("CFaceTextureCoords", 4),
        ("CFontManager", 0),
        ("CGroup", 1),
        ("CImage", 1),
        ("CLayer", s.layer),
        ("CLayerGroup", 2),
        ("CLayerManager", s.layer_mgr),
        ("CLineStyleManager", 1),
        ("CLoop", 1),
        ("CMaterial", 12),
        ("CMaterialManager", 4),
        ("CPageList", 1),
        ("CPolyline3d", 0),
        ("CRelationship", 0),
        ("CRelationshipMap", 0),
        ("CRenderingOptions", s.ro),
        ("CSchemaFile", 1),
        ("CSchemaFilterFile", 0),
        ("CSchemaZipFile", 1),
        ("CSectionPlane", s.section),
        ("CShadowInfo", 7),
        ("CSkFont", 1),
        ("CSketchCS", 0),
        ("CSketchUpModel", s.model),
        ("CSketchUpPage", 1),
        ("CSkpStyle", s.style),
        ("CSkpStyleManager", 2),
        ("CText", 9),
        ("CTextStyle", 5),
        ("CTexture", 6),
        ("CThumbnail", 1),
        ("CVertex", 0),
        ("CViewPage", s.page),
        ("CWatermark", 1),
        ("CWatermarkManager", 2),
    ]
}

const S2014: S14 = S14 {
    arc: 2,
    definition: 10,
    instance: 5,
    entity: 3,
    layer: 2,
    ro: 36,
    section: 2,
    model: 24,
    style: 1,
    page: 12,
    line_style: 0,
    layer_mgr: 4,
};

const fn without(m: [(&'static str, u32); 61], drop: [&'static str; 3]) -> ([(&'static str, u32); 61], usize) {
    let mut out = m;
    let mut n = 0;
    let mut i = 0;
    while i < 61 {
        let name = m[i].0;
        let mut keep = true;
        let mut k = 0;
        while k < 3 {
            if eq(name, drop[k]) {
                keep = false;
            }
            k += 1;
        }
        if keep {
            out[n] = m[i];
            n += 1;
        }
        i += 1;
    }
    (out, n)
}

const fn eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

const fn first<const N: usize>(m: ([(&'static str, u32); 61], usize)) -> [(&'static str, u32); N] {
    let mut out = [("", 0); N];
    let mut i = 0;
    while i < N {
        out[i] = m.0[i];
        i += 1;
    }
    assert!(m.1 == N);
    out
}

/// 2014 to 2018: without the line styles and the tag folders.
const OLD3: [&str; 3] = ["CCustomLineStyle", "CLineStyleManager", "CLayerGroup"];

pub const V2014: Map = &first::<58>(without(v14_20(S2014), OLD3));
pub const V2015: Map = &first::<58>(without(v14_20(S14 { arc: 3, ..S2014 }), OLD3));
pub const V2016: Map = &first::<58>(without(v14_20(S14 { arc: 3, model: 25, ..S2014 }), OLD3));
pub const V2017: Map = &first::<58>(without(v14_20(S14 { arc: 3, model: 26, entity: 5, ..S2014 }), OLD3));
const S2018: S14 = S14 { arc: 3, model: 28, entity: 5, definition: 11, instance: 6, ro: 37, section: 3, ..S2014 };
pub const V2018: Map = &first::<58>(without(v14_20(S2018), OLD3));
pub const V2019: Map = &first::<60>(without(
    v14_20(S14 { model: 29, ro: 38, layer: 3, line_style: 2, ..S2018 }),
    ["CLayerGroup", "-", "-"],
));
pub const V2020: Map =
    &v14_20(S14 { model: 29, ro: 39, layer: 3, line_style: 3, page: 13, style: 2, layer_mgr: 7, ..S2018 });

/// The map of a version (3 … 8, 2013 … 2020).
pub fn map(v: crate::SkpVersion) -> Map {
    use crate::SkpVersion::*;
    match v {
        Su3 => V3,
        Su4 => V4,
        Su5 => V5,
        Su6 => V6,
        Su7 => V7,
        Su8 => V8,
        Su2013 => V2013,
        Su2014 => V2014,
        Su2015 => V2015,
        Su2016 => V2016,
        Su2017 => V2017,
        Su2018 => V2018,
        Su2019 => V2019,
        Su2020 | Su2021 => V2020,
    }
}

/// The schema of a class in a map (0 when the map does not name it, as the reader of
/// SketchUp.exe takes it).
pub fn schema(m: Map, class: &str) -> u32 {
    m.iter().find(|(c, _)| *c == class).map_or(0, |(_, s)| *s)
}

/// The schema a class is declared with in the archive (`CRuntimeClass::m_wSchema` of
/// SketchUp 2026, whatever the version written): that of 2020, but for the classes that
/// changed since.
pub fn declared(class: &str) -> u32 {
    match class {
        "CCustomLineStyle" => 4,
        "CLayerGroup" => 3,
        "CViewPage" => 15,
        _ => schema(V2020, class),
    }
}

pub fn has(m: Map, class: &str) -> bool {
    m.iter().any(|(c, _)| *c == class)
}

/// The version code SketchUp's writers test (3000 … 8000, 13000 … 20000).
pub fn code(v: crate::SkpVersion) -> u32 {
    v.major() * 1000
}
