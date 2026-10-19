//! AutoCAD DWG files in every release SketchUp's AutoCAD exporters offer (`skp2acad`:
//! "Version 12" to "AutoCAD 2018"), written from the Open Design Alliance's public *Open
//! Design Specification for .dwg files* (v5.4.1):
//!
//! - releases 13, 14 and 2000 (`AC1012`, `AC1014`, `AC1015`): the file header and its
//!   section locators, the header variables, the classes, the object stream, the object
//!   map, the "unknown" section, the template and an empty preview ([`r13`]);
//! - 2004, 2010, 2013 and 2018 (`AC1018`, `AC1024`, `AC1027`, `AC1032`): the encrypted
//!   file header, sections cut into LZ77-compressed pages, the section and page maps
//!   ([`r2004`]);
//! - 2007 (`AC1021`): Reed-Solomon coded pages, 64-bit CRCs and the randomised file
//!   header of chapter 5 ([`r2007`]);
//! - release 12 (`AC1009`), which the ODA specification does not cover, in the pre-R13
//!   format ([`r12`]).
//!
//! The objects are the same in every release ([`objects`]): what differs is how they are
//! coded (strings in UTF-16 in a separate stream from 2007, handles in their own stream,
//! the 2010 object type, colours with a method byte from 2004…) and which of them exist
//! (layouts and plot styles from 2000, no lightweight polylines before release 14, no
//! infinite lines in release 12). Up to release 14, layer and block names are in capitals
//! ([`DwgVersion::table_name`]).
//!
//! [`write_dxf`] writes the same drawing as DXF of the same release, with the handles and
//! objects of its DWG ([`dxf_out`]).
//!
//! The drawing is described by [`DwgDoc`] — layers, block definitions and model-space
//! entities in world coordinates — whatever made it: the 2D export converts its
//! [`crate::vector2d::CadDrawing`], the 3D export its own document. What every DWG needs
//! besides is written with AutoCAD's defaults: the table control objects, layer "0", the
//! Standard text and dimension styles, the ByBlock / ByLayer / Continuous linetypes, the
//! `ACAD` application, the `*Active` viewport, the named object dictionary and the model
//! and paper space blocks.

use anyhow::Result;
use glam::{DVec2, DVec3};

mod bits;
mod dxf_out;
mod lz77;
mod objects;
mod r12;
mod r13;
mod r2004;
mod r2007;
mod rs;

pub use bits::crc16;

/// A release of the DWG format, in the order of SketchUp's list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DwgVersion {
    R12,
    R13,
    R14,
    #[default]
    R2000,
    R2004,
    R2007,
    R2010,
    R2013,
    R2018,
}

impl DwgVersion {
    pub const ALL: [DwgVersion; 9] = [
        DwgVersion::R12,
        DwgVersion::R13,
        DwgVersion::R14,
        DwgVersion::R2000,
        DwgVersion::R2004,
        DwgVersion::R2007,
        DwgVersion::R2010,
        DwgVersion::R2013,
        DwgVersion::R2018,
    ];

    /// The labels of SketchUp's list, in its order.
    pub const LABELS: [&'static str; 9] = {
        let mut out = [""; 9];
        let mut i = 0;
        while i < 9 {
            out[i] = DwgVersion::ALL[i].label();
            i += 1;
        }
        out
    };

    /// The `ExportAcadVersion` value of SketchUp's exporters (`skp2acad.dll` maps 0–8
    /// to the database versions of Release 12 … AutoCAD 2018), the index of the list.
    pub fn code(self) -> usize {
        DwgVersion::ALL.iter().position(|v| *v == self).unwrap_or(3)
    }

    /// The version of an `ExportAcadVersion` value.
    pub fn from_code(code: usize) -> Option<DwgVersion> {
        DwgVersion::ALL.get(code).copied()
    }

    /// The label of SketchUp's list, in French.
    pub const fn label(self) -> &'static str {
        match self {
            DwgVersion::R12 => "Version 12",
            DwgVersion::R13 => "Version 13",
            DwgVersion::R14 => "Version 14",
            DwgVersion::R2000 => "AutoCAD 2000",
            DwgVersion::R2004 => "AutoCAD 2004",
            DwgVersion::R2007 => "AutoCAD 2007",
            DwgVersion::R2010 => "AutoCAD 2010",
            DwgVersion::R2013 => "AutoCAD 2013",
            DwgVersion::R2018 => "AutoCAD 2018",
        }
    }

    /// The 6 bytes a file of this version starts with (`$ACADVER` in DXF).
    pub const fn magic(self) -> &'static str {
        match self {
            DwgVersion::R12 => "AC1009",
            DwgVersion::R13 => "AC1012",
            DwgVersion::R14 => "AC1014",
            DwgVersion::R2000 => "AC1015",
            DwgVersion::R2004 => "AC1018",
            DwgVersion::R2007 => "AC1021",
            DwgVersion::R2010 => "AC1024",
            DwgVersion::R2013 => "AC1027",
            DwgVersion::R2018 => "AC1032",
        }
    }

    /// The version number of the drawing database (AuxHeader, classes, file header byte
    /// 0x11): 19 for R13 … 33 for 2018 (spec chapter 27).
    fn number(self) -> u8 {
        match self {
            DwgVersion::R12 => 16,
            DwgVersion::R13 => 19,
            DwgVersion::R14 => 21,
            DwgVersion::R2000 => 23,
            DwgVersion::R2004 => 25,
            DwgVersion::R2007 => 27,
            DwgVersion::R2010 => 29,
            DwgVersion::R2013 => 31,
            DwgVersion::R2018 => 33,
        }
    }

    /// The maintenance release written at 0x0B of the file. From 2010 a value above 3
    /// means an extra long after the size of the header and classes sections (chapters 9
    /// and 10); the ODA writes 8 for 2013 (chapter 7).
    fn maintenance(self) -> u8 {
        match self {
            DwgVersion::R2007 => 0x19,
            DwgVersion::R2010 => 6,
            DwgVersion::R2013 => 8,
            _ => 0,
        }
    }

    /// The name a layer or block named `name` gets in this release: up to release 14, in
    /// capitals with `_` for what those releases do not allow (as SketchUp's exporter,
    /// "Fenêtre" is "FEN_TRE"); from 2000, `name` itself.
    pub fn table_name(self, name: &str) -> String {
        objects::Names::new(self, &[]).add(name)
    }

    /// Whether the header and classes sections have a second long after their size.
    fn size_high(self) -> bool {
        (matches!(self, DwgVersion::R2010 | DwgVersion::R2013) && self.maintenance() > 3) || self >= DwgVersion::R2018
    }
}

/// Colour of an entity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DwgColor {
    #[default]
    ByLayer,
    ByBlock,
    /// AutoCAD Color Index, 1–255.
    Aci(u8),
    /// A true colour (from 2004), with the colour index that earlier releases write.
    Rgb {
        rgb: [u8; 3],
        aci: u8,
    },
}

impl DwgColor {
    fn index(self) -> i16 {
        match self {
            DwgColor::ByLayer => 256,
            DwgColor::ByBlock => 0,
            DwgColor::Aci(i) | DwgColor::Rgb { aci: i, .. } => i16::from(i.max(1)),
        }
    }

    /// The true colour as 0xRRGGBB.
    fn rgb(self) -> Option<u32> {
        match self {
            DwgColor::Rgb { rgb: [r, g, b], .. } => Some(u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)),
            _ => None,
        }
    }
}

/// A layer.
#[derive(Clone, Debug, PartialEq)]
pub struct DwgLayer {
    pub name: String,
    /// AutoCAD Color Index, 1–255.
    pub color: u8,
    /// Layers that are off are not displayed.
    pub visible: bool,
}

impl DwgLayer {
    pub fn new(name: impl Into<String>, color: u8) -> DwgLayer {
        DwgLayer { name: name.into(), color, visible: true }
    }
}

/// What an entity is. Points are in world coordinates.
#[derive(Clone, Debug, PartialEq)]
pub enum DwgGeom {
    /// LINE; `thickness` extrudes it along Z.
    Line {
        a: DVec3,
        b: DVec3,
        thickness: f64,
    },
    /// LWPOLYLINE in a plane parallel to XY at `elevation`, of constant `width` (a 2D
    /// POLYLINE in release 13, which has no lightweight polylines).
    LwPolyline {
        pts: Vec<DVec2>,
        closed: bool,
        width: f64,
        elevation: f64,
    },
    /// POLYLINE (3D) with its VERTEX_3D and SEQEND.
    Polyline3d {
        pts: Vec<DVec3>,
        closed: bool,
    },
    /// 3DFACE; a triangle repeats its third corner. Bit `i` of `hidden` hides edge `i`.
    Face3d {
        corners: [DVec3; 4],
        hidden: u8,
    },
    /// POLYLINE (polyface mesh): vertices, then faces of 1-based vertex indices (0 for an
    /// unused fourth one; a negative index hides the edge that starts at that vertex).
    PolyFace {
        verts: Vec<DVec3>,
        faces: Vec<[i32; 4]>,
    },
    /// SOLID: a filled quadrilateral parallel to XY, corners in AutoCAD's order (1, 2, 4, 3
    /// around), a triangle repeating its third corner.
    Solid {
        corners: [DVec2; 4],
        elevation: f64,
    },
    /// TEXT parallel to XY: `pos` is the start of the baseline; `align` the justification
    /// point when `halign` / `valign` (DXF groups 72 / 73) are not both 0.
    Text {
        pos: DVec3,
        align: DVec3,
        height: f64,
        rotation: f64,
        halign: u8,
        valign: u8,
        text: String,
    },
    /// ARC: angles in radians, counter-clockwise about `normal` from the X axis of its
    /// arbitrary-axis plane.
    Arc {
        center: DVec3,
        normal: DVec3,
        radius: f64,
        start: f64,
        end: f64,
    },
    Circle {
        center: DVec3,
        normal: DVec3,
        radius: f64,
    },
    Point {
        p: DVec3,
    },
    /// XLINE: an infinite line.
    XLine {
        p: DVec3,
        dir: DVec3,
    },
    /// INSERT of the block named `block`: `pos` in world coordinates, scales, rotation (radians)
    /// about `normal` in the plane of `normal`.
    Insert {
        block: String,
        pos: DVec3,
        scale: DVec3,
        rotation: f64,
        normal: DVec3,
    },
}

/// An entity on a layer.
#[derive(Clone, Debug, PartialEq)]
pub struct DwgEntity {
    pub geom: DwgGeom,
    pub layer: String,
    pub color: DwgColor,
}

impl DwgEntity {
    pub fn new(geom: DwgGeom, layer: impl Into<String>) -> DwgEntity {
        DwgEntity { geom, layer: layer.into(), color: DwgColor::ByLayer }
    }
}

/// A block definition.
#[derive(Clone, Debug, PartialEq)]
pub struct DwgBlock {
    pub name: String,
    pub base: DVec3,
    pub entities: Vec<DwgEntity>,
}

/// A drawing. Layers used by entities but not listed are created white; layer "0" always
/// exists. Blocks must come before the blocks that insert them.
#[derive(Clone, Debug, PartialEq)]
pub struct DwgDoc {
    pub layers: Vec<DwgLayer>,
    pub blocks: Vec<DwgBlock>,
    /// Model space.
    pub entities: Vec<DwgEntity>,
    /// `INSUNITS`: 0 none, 1 inches, 2 feet, 4 millimetres, 5 centimetres, 6 metres…
    pub insunits: i16,
    /// Font of the Standard text style: "txt" (AutoCAD's), "arial.ttf"…
    pub font: String,
}

impl Default for DwgDoc {
    fn default() -> Self {
        DwgDoc { layers: Vec::new(), blocks: Vec::new(), entities: Vec::new(), insunits: 4, font: "txt".into() }
    }
}

impl DwgDoc {
    fn metric(&self) -> bool {
        matches!(self.insunits, 4..=6 | 12..=20)
    }

    /// The font of the Standard text style in release `v`: release 12 has no TrueType
    /// fonts, AutoCAD's "txt" stands in for them.
    fn font(&self, v: DwgVersion) -> &str {
        if v == DwgVersion::R12 && self.font.to_lowercase().ends_with(".ttf") { "txt" } else { &self.font }
    }
}

/// Writes `doc` as a DWG file of `version`.
pub fn write(doc: &DwgDoc, version: DwgVersion) -> Result<Vec<u8>> {
    if version == DwgVersion::R12 {
        return r12::write(doc);
    }
    let built = objects::build(doc, version)?;
    Ok(match version {
        DwgVersion::R13 | DwgVersion::R14 | DwgVersion::R2000 => r13::write(&built),
        DwgVersion::R2007 => r2007::write(&built),
        _ => r2004::write(&built),
    })
}

/// Writes `doc` as a DXF file of `version`, with the objects and handles of its DWG.
pub fn write_dxf(doc: &DwgDoc, version: DwgVersion) -> Result<Vec<u8>> {
    dxf_out::write(doc, version)
}

#[cfg(test)]
#[cfg(windows)]
pub(crate) mod teigha;
#[cfg(test)]
pub(crate) mod tests;
