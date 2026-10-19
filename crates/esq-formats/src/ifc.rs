//! IFC (Industry Foundation Classes, .ifc and .ifczip).
//!
//! Export as SketchUp's IFC exporter does it: IFC 2x3 or IFC 4, one product per group or
//! component, of the class its definition is classified as ("AppliedSchemaTypes"), groups
//! classified IfcSite / IfcBuilding / IfcBuildingStorey / IfcSpace forming the spatial
//! structure, optionally completed into the standard hierarchy; tessellated or
//! boundary-representation bodies with their colours.
//!
//! Import: placements, extruded and revolved profiles, faceted breps, face sets, mapped
//! items (shared components), openings cut with the solid tools, colours, tags made from
//! the IFC classes or the IFC layers; each product becomes a component keeping its class.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::f64::consts::TAU;
use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Result, bail};
use esq_core::glam::{DMat3, DMat4, DQuat, DVec3};
use esq_core::math::newell_normal;
use esq_core::model::{AttrValue, DefId, Definition, Entities, Face, Instance, MaterialId, Model};
use esq_core::solid::SolidOp;

use crate::build::{BuildOpts, DefIn, MeshIn, NodeIn, PolyIn, SceneIn};
use crate::options::{IfcExport, IfcImport, IfcRepresentation, IfcTags, IfcVersion};
use crate::step::{self, P, StepFile, encode, real, refs};

/// The attribute dictionary SketchUp keeps classifications in, by schema name.
pub const APPLIED: &str = "AppliedSchemaTypes";

fn schema_key(v: IfcVersion) -> &'static str {
    match v {
        IfcVersion::Ifc2x3 => "IFC 2x3",
        IfcVersion::Ifc4 => "IFC 4",
    }
}

// ------------------------------------------------------------------------------------------
// GUIDs

const GUID_CHARS: &[u8; 64] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$";

/// IFC GlobalIds: 128 bits written as 22 characters.
struct Guids(u64);

impl Guids {
    fn new() -> Guids {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64);
        Guids(t ^ 0x5851_F42D_4C95_7F2D)
    }

    fn next_u64(&mut self) -> u64 {
        // splitmix64
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// The next GlobalId, quoted for the file.
    fn next(&mut self) -> String {
        let n = ((self.next_u64() as u128) << 64) | self.next_u64() as u128;
        let mut s = String::with_capacity(24);
        s.push('\'');
        // The first character holds the top 2 bits, then 21 characters of 6 bits.
        s.push(GUID_CHARS[(n >> 126) as usize] as char);
        for i in (0..21).rev() {
            s.push(GUID_CHARS[((n >> (i * 6)) & 63) as usize] as char);
        }
        s.push('\'');
        s
    }
}

// ------------------------------------------------------------------------------------------
// Export

/// Where a product goes in the spatial structure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Site,
    Building,
    Storey,
    Space,
    Element,
}

/// An IFC class with its number of attributes in IFC 2x3 and IFC 4 (`None` when the class
/// does not exist in that schema) and, for IFC 2x3, the position of a required enumeration.
struct Class {
    name: &'static str,
    kind: Kind,
    n2x3: Option<usize>,
    n4: Option<usize>,
    enum2x3: Option<usize>,
}

const fn c(name: &'static str, kind: Kind, n2x3: Option<usize>, n4: Option<usize>, enum2x3: Option<usize>) -> Class {
    Class { name, kind, n2x3, n4, enum2x3 }
}

const CLASSES: &[Class] = &[
    c("IfcBuildingElementProxy", Kind::Element, Some(9), Some(9), None),
    c("IfcSite", Kind::Site, Some(14), Some(14), None),
    c("IfcBuilding", Kind::Building, Some(12), Some(12), None),
    c("IfcBuildingStorey", Kind::Storey, Some(10), Some(10), None),
    c("IfcSpace", Kind::Space, Some(11), Some(11), Some(9)),
    c("IfcWall", Kind::Element, Some(8), Some(9), None),
    c("IfcWallStandardCase", Kind::Element, Some(8), Some(9), None),
    c("IfcSlab", Kind::Element, Some(9), Some(9), None),
    c("IfcBeam", Kind::Element, Some(8), Some(9), None),
    c("IfcColumn", Kind::Element, Some(8), Some(9), None),
    c("IfcMember", Kind::Element, Some(8), Some(9), None),
    c("IfcPlate", Kind::Element, Some(8), Some(9), None),
    c("IfcRoof", Kind::Element, Some(9), Some(9), Some(8)),
    c("IfcStair", Kind::Element, Some(9), Some(9), Some(8)),
    c("IfcStairFlight", Kind::Element, Some(12), Some(13), None),
    c("IfcRamp", Kind::Element, Some(9), Some(9), Some(8)),
    c("IfcRampFlight", Kind::Element, Some(8), Some(9), None),
    c("IfcRailing", Kind::Element, Some(9), Some(9), None),
    c("IfcCovering", Kind::Element, Some(9), Some(9), None),
    c("IfcCurtainWall", Kind::Element, Some(8), Some(9), None),
    c("IfcFooting", Kind::Element, Some(9), Some(9), Some(8)),
    c("IfcPile", Kind::Element, Some(10), Some(10), Some(8)),
    c("IfcDoor", Kind::Element, Some(10), Some(13), None),
    c("IfcWindow", Kind::Element, Some(10), Some(13), None),
    c("IfcFurnishingElement", Kind::Element, Some(8), Some(8), None),
    c("IfcFurniture", Kind::Element, None, Some(9), None),
    c("IfcFlowTerminal", Kind::Element, Some(8), Some(8), None),
    c("IfcFlowSegment", Kind::Element, Some(8), Some(8), None),
    c("IfcFlowFitting", Kind::Element, Some(8), Some(8), None),
    c("IfcChimney", Kind::Element, None, Some(9), None),
    c("IfcShadingDevice", Kind::Element, None, Some(9), None),
    c("IfcBuildingElementPart", Kind::Element, None, Some(9), None),
    c("IfcDiscreteAccessory", Kind::Element, Some(8), Some(9), None),
    c("IfcTransportElement", Kind::Element, Some(11), Some(9), None),
    c("IfcGeographicElement", Kind::Element, None, Some(9), None),
    c("IfcElementAssembly", Kind::Element, Some(10), Some(10), Some(9)),
];

/// The classification of a definition for a schema (the other schema's when it has none).
fn classification(model: &Model, def: DefId, version: IfcVersion) -> Option<String> {
    let applied = model.defs.get(def)?.attributes.get(APPLIED)?;
    let other = match version {
        IfcVersion::Ifc2x3 => IfcVersion::Ifc4,
        IfcVersion::Ifc4 => IfcVersion::Ifc2x3,
    };
    [version, other].iter().find_map(|v| match applied.get(schema_key(*v))? {
        AttrValue::Text(t) if !t.trim().is_empty() => Some(t.trim().to_string()),
        _ => None,
    })
}

struct Exp<'a> {
    model: &'a Model,
    o: &'a IfcExport,
    w: step::Writer,
    guid: Guids,
    owner: u64,
    body_ctx: u64,
    styles: HashMap<Option<MaterialId>, u64>,
    counts: BTreeMap<&'static str, usize>,
    replaced: usize,
    origin: u64,
    z: u64,
    x: u64,
}

/// A product written: its id and kind.
type Made = (u64, Kind);

impl Exp<'_> {
    fn point(&mut self, p: DVec3) -> u64 {
        self.w.add("IFCCARTESIANPOINT", &format!("({},{},{})", real(p.x), real(p.y), real(p.z)))
    }

    fn dir(&mut self, d: DVec3) -> u64 {
        self.w.add("IFCDIRECTION", &format!("({},{},{})", real(d.x), real(d.y), real(d.z)))
    }

    /// An IfcAxis2Placement3D of a rigid transform.
    fn axis(&mut self, m: &DMat4) -> u64 {
        if *m == DMat4::IDENTITY {
            return self.w.add("IFCAXIS2PLACEMENT3D", &format!("#{},#{},#{}", self.origin, self.z, self.x));
        }
        let o = self.point(m.w_axis.truncate());
        let z = m.z_axis.truncate().normalize_or(DVec3::Z);
        let x = m.x_axis.truncate().normalize_or(DVec3::X);
        let (zi, xi) = (self.dir(z), self.dir(x));
        self.w.add("IFCAXIS2PLACEMENT3D", &format!("#{o},#{zi},#{xi}"))
    }

    fn placement(&mut self, parent: Option<u64>, m: &DMat4) -> u64 {
        let a = self.axis(m);
        let rel = parent.map_or("$".into(), |p| format!("#{p}"));
        self.w.add("IFCLOCALPLACEMENT", &format!("{rel},#{a}"))
    }

    /// The surface style of a material (its colour and transparency).
    fn style(&mut self, m: Option<MaterialId>) -> u64 {
        if let Some(&s) = self.styles.get(&m) {
            return s;
        }
        let (c, a, name) = match m.and_then(|m| self.model.materials.get(m)) {
            Some(x) => (x.color, x.opacity as f64, x.name.clone()),
            None => (crate::obj::FRONT_COLOR, 1.0, "Default".to_string()),
        };
        let ch = |v: u8| real(v as f64 / 255.0);
        let col = self.w.add("IFCCOLOURRGB", &format!("$,{},{},{}", ch(c[0]), ch(c[1]), ch(c[2])));
        let t = real(1.0 - a);
        let shading = match self.o.version {
            IfcVersion::Ifc4 => self.w.add("IFCSURFACESTYLESHADING", &format!("#{col},{t}")),
            IfcVersion::Ifc2x3 => {
                self.w.add("IFCSURFACESTYLERENDERING", &format!("#{col},{t},$,$,$,$,$,$,.NOTDEFINED."))
            }
        };
        let s = self.w.add("IFCSURFACESTYLE", &format!("{},.BOTH.,(#{shading})", encode(&name)));
        self.styles.insert(m, s);
        s
    }

    fn styled(&mut self, item: u64, m: Option<MaterialId>) {
        let s = self.style(m);
        let styles = match self.o.version {
            IfcVersion::Ifc4 => s,
            IfcVersion::Ifc2x3 => self.w.add("IFCPRESENTATIONSTYLEASSIGNMENT", &format!("(#{s})")),
        };
        self.w.add("IFCSTYLEDITEM", &format!("#{item},(#{styles}),$"));
    }

    fn tessellated(&self) -> bool {
        self.o.representation == IfcRepresentation::Tessellation && self.o.version == IfcVersion::Ifc4
    }

    /// The shape of a collection's loose faces (one item per material), in its coordinates.
    fn shape(&mut self, ents: &Entities, inherited: Option<MaterialId>) -> Option<u64> {
        let model = self.model;
        let hidden = self.o.hidden;
        let mut by_mat: BTreeMap<Option<MaterialId>, Vec<&Face>> = BTreeMap::new();
        for f in ents.faces.values() {
            if hidden || (!f.hidden && model.layer_shown(f.layer)) {
                by_mat.entry(f.front.or(inherited)).or_default().push(f);
            }
        }
        if by_mat.is_empty() {
            return None;
        }
        let tess = self.tessellated();
        // A single closed volume is a boundary representation; anything else a surface model.
        let brep = !tess && by_mat.len() == 1 && esq_core::solid::is_solid(ents);
        let mut items = Vec::new();
        for (mat, faces) in by_mat {
            let item = if tess { self.face_set(ents, &faces) } else { self.faceted(ents, &faces, brep) };
            self.styled(item, mat);
            items.push(item);
        }
        let kind = if tess {
            "Tessellation"
        } else if brep {
            "Brep"
        } else {
            "SurfaceModel"
        };
        let rep = self.w.add("IFCSHAPEREPRESENTATION", &format!("#{},'Body','{kind}',{}", self.body_ctx, refs(&items)));
        Some(self.w.add("IFCPRODUCTDEFINITIONSHAPE", &format!("$,$,(#{rep})")))
    }

    /// An IfcTriangulatedFaceSet (IFC 4).
    fn face_set(&mut self, ents: &Entities, faces: &[&Face]) -> u64 {
        let mut pts: Vec<DVec3> = Vec::new();
        let mut index: HashMap<[u64; 3], usize> = HashMap::new();
        let mut tris: Vec<[usize; 3]> = Vec::new();
        for f in faces {
            let outer: Vec<DVec3> = f.outer.iter().map(|v| ents.verts[*v].pos).collect();
            let holes: Vec<Vec<DVec3>> =
                f.holes.iter().map(|h| h.iter().map(|v| ents.verts[*v].pos).collect()).collect();
            let ids: Vec<usize> = outer
                .iter()
                .chain(holes.iter().flatten())
                .map(|q| {
                    *index.entry(q.to_array().map(f64::to_bits)).or_insert_with(|| {
                        pts.push(*q);
                        pts.len()
                    })
                })
                .collect();
            for t in crate::scene::triangles(&outer, &holes, f.normal) {
                tris.push([ids[t[0]], ids[t[1]], ids[t[2]]]);
            }
        }
        let list: Vec<String> = pts.iter().map(|p| format!("({},{},{})", real(p.x), real(p.y), real(p.z))).collect();
        let pl = self.w.add("IFCCARTESIANPOINTLIST3D", &format!("({})", list.join(",")));
        let idx: Vec<String> = tris.iter().map(|t| format!("({},{},{})", t[0], t[1], t[2])).collect();
        self.w.add("IFCTRIANGULATEDFACESET", &format!("#{pl},$,$,({}),$", idx.join(",")))
    }

    /// An IfcFacetedBrep (closed) or IfcShellBasedSurfaceModel (open): one IfcFace per face.
    fn faceted(&mut self, ents: &Entities, faces: &[&Face], closed: bool) -> u64 {
        let mut pt_ids: HashMap<[u64; 3], u64> = HashMap::new();
        let mut face_ids = Vec::new();
        for f in faces {
            let mut bounds = Vec::new();
            for (k, l) in f.loops().enumerate() {
                let mut ids = Vec::with_capacity(l.len());
                for v in l {
                    let q = ents.verts[*v].pos;
                    let key = q.to_array().map(f64::to_bits);
                    let i = match pt_ids.get(&key) {
                        Some(&i) => i,
                        None => {
                            let i = self.point(q);
                            pt_ids.insert(key, i);
                            i
                        }
                    };
                    ids.push(i);
                }
                let lp = self.w.add("IFCPOLYLOOP", &refs(&ids));
                let ty = if k == 0 { "IFCFACEOUTERBOUND" } else { "IFCFACEBOUND" };
                bounds.push(self.w.add(ty, &format!("#{lp},.T.")));
            }
            face_ids.push(self.w.add("IFCFACE", &refs(&bounds)));
        }
        if closed {
            let shell = self.w.add("IFCCLOSEDSHELL", &refs(&face_ids));
            self.w.add("IFCFACETEDBREP", &format!("#{shell}"))
        } else {
            let shell = self.w.add("IFCOPENSHELL", &refs(&face_ids));
            self.w.add("IFCSHELLBASEDSURFACEMODEL", &format!("(#{shell})"))
        }
    }

    /// A product of a class (IfcBuildingElementProxy when the class is not one of the
    /// schema).
    fn product(&mut self, class: &str, name: &str, place: u64, shape: Option<u64>) -> Made {
        let v = self.o.version;
        let n = |c: &Class| if v == IfcVersion::Ifc4 { c.n4 } else { c.n2x3 };
        let found = CLASSES.iter().find(|c| c.name.eq_ignore_ascii_case(class) && n(c).is_some());
        let class = match found {
            Some(c) => c,
            None => {
                self.replaced += 1;
                &CLASSES[0]
            }
        };
        let count = n(class).unwrap_or(9);
        let guid = self.guid.next();
        let shape = shape.map_or("$".into(), |s| format!("#{s}"));
        let mut args = format!("{guid},#{},{},$,$,#{place},{shape}", self.owner, encode(name));
        for i in 7..count {
            let required_enum = v == IfcVersion::Ifc2x3 && class.enum2x3 == Some(i);
            let composition = class.kind != Kind::Element && i == 8;
            args.push_str(if composition {
                ",.ELEMENT."
            } else if required_enum {
                ",.NOTDEFINED."
            } else {
                ",$"
            });
        }
        *self.counts.entry(class.name).or_default() += 1;
        (self.w.add(&class.name.to_uppercase(), &args), class.kind)
    }

    /// A relation between a product and its children: spatial children are aggregated,
    /// elements contained in a spatial parent or aggregated into an element.
    fn relate(&mut self, parent: Made, children: &[Made]) {
        let spatial: Vec<u64> = children.iter().filter(|c| c.1 != Kind::Element).map(|c| c.0).collect();
        let elements: Vec<u64> = children.iter().filter(|c| c.1 == Kind::Element).map(|c| c.0).collect();
        let owner = self.owner;
        if !spatial.is_empty() {
            let g = self.guid.next();
            self.w.add("IFCRELAGGREGATES", &format!("{g},#{owner},$,$,#{},{}", parent.0, refs(&spatial)));
        }
        if !elements.is_empty() {
            let g = self.guid.next();
            if parent.1 == Kind::Element {
                self.w.add("IFCRELAGGREGATES", &format!("{g},#{owner},$,$,#{},{}", parent.0, refs(&elements)));
            } else {
                let args = format!("{g},#{owner},$,$,{},#{}", refs(&elements), parent.0);
                self.w.add("IFCRELCONTAINEDINSPATIALSTRUCTURE", &args);
            }
        }
    }

    /// Products of the groups and components of a collection, with their own children.
    fn products(
        &mut self,
        ents: &Entities,
        parent_place: u64,
        inherited: Option<MaterialId>,
        depth: usize,
    ) -> Vec<Made> {
        let model = self.model;
        let mut out = Vec::new();
        if depth > 64 {
            return out;
        }
        for inst in ents.instances.values() {
            let Some(d) = model.defs.get(inst.def) else { continue };
            let shown = self.o.hidden || (!inst.hidden && model.layer_shown(inst.layer));
            if !shown || d.ext.image.is_some() {
                continue;
            }
            let inh = inst.material.or(inherited);
            // The placement is the rigid part of the transform; scaling goes into the
            // coordinates.
            let (s, r, t) = inst.xform.to_scale_rotation_translation();
            let place = self.placement(Some(parent_place), &DMat4::from_rotation_translation(r, t));
            let scaled;
            let contents = if (s - DVec3::ONE).abs().max_element() > 1e-12 {
                scaled = scale_entities(&d.entities, s);
                &scaled
            } else {
                &d.entities
            };
            let shape = self.shape(contents, inh);
            let class =
                classification(model, inst.def, self.o.version).unwrap_or_else(|| "IfcBuildingElementProxy".into());
            let name = if inst.name.is_empty() { d.name.clone() } else { inst.name.clone() };
            let me = self.product(&class, &name, place, shape);
            let children = self.products(contents, place, inh, depth + 1);
            self.relate(me, &children);
            out.push(me);
        }
        out
    }
}

/// A copy of a collection scaled along its axes.
fn scale_entities(e: &Entities, s: DVec3) -> Entities {
    let mut c = e.clone();
    let m = DMat4::from_scale(s);
    for v in c.verts.values_mut() {
        v.pos = m.transform_point3(v.pos);
    }
    let n = DMat3::from_mat4(m).inverse().transpose();
    for f in c.faces.values_mut() {
        f.normal = (n * f.normal).normalize_or_zero();
    }
    for i in c.instances.values_mut() {
        i.xform = m * i.xform;
    }
    c
}

/// Degrees as an IfcCompoundPlaneAngleMeasure (degrees, minutes, seconds, millionths).
fn dms(deg: f64) -> String {
    let total = (deg.abs() * 3600.0 * 1e6).round() as i64;
    let (d, rest) = (total / 3_600_000_000, total % 3_600_000_000);
    let (m, rest) = (rest / 60_000_000, rest % 60_000_000);
    let (s, u) = (rest / 1_000_000, rest % 1_000_000);
    let k = if deg < 0.0 { -1 } else { 1 };
    format!("({},{},{},{})", k * d, k * m, k * s, k * u)
}

/// The IFC text of a model, and the export report.
pub fn write(model: &Model, o: &IfcExport, name: &str) -> (String, String) {
    let mut w = step::Writer::new();
    let mut guid = Guids::new();
    let person = w.add("IFCPERSON", "$,$,'',$,$,$,$,$");
    let org = w.add("IFCORGANIZATION", "$,'Esquisse',$,$,$");
    let po = w.add("IFCPERSONANDORGANIZATION", &format!("#{person},#{org},$"));
    let version = env!("CARGO_PKG_VERSION");
    let app = w.add("IFCAPPLICATION", &format!("#{org},'{version}','Esquisse','Esquisse'"));
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let owner = w.add("IFCOWNERHISTORY", &format!("#{po},#{app},$,.ADDED.,$,$,$,{now}"));
    let len = w.add("IFCSIUNIT", "*,.LENGTHUNIT.,.MILLI.,.METRE.");
    let area = w.add("IFCSIUNIT", "*,.AREAUNIT.,$,.SQUARE_METRE.");
    let vol = w.add("IFCSIUNIT", "*,.VOLUMEUNIT.,$,.CUBIC_METRE.");
    let ang = w.add("IFCSIUNIT", "*,.PLANEANGLEUNIT.,$,.RADIAN.");
    let units = w.add("IFCUNITASSIGNMENT", &refs(&[len, area, vol, ang]));
    let origin = w.add("IFCCARTESIANPOINT", "(0.,0.,0.)");
    let z = w.add("IFCDIRECTION", "(0.,0.,1.)");
    let x = w.add("IFCDIRECTION", "(1.,0.,0.)");
    let wcs = w.add("IFCAXIS2PLACEMENT3D", &format!("#{origin},#{z},#{x}"));
    // True north: the green axis turned clockwise by the north angle.
    let a = model.shadow.north_angle.to_radians();
    let north = w.add("IFCDIRECTION", &format!("({},{})", real(-a.sin()), real(a.cos())));
    let ctx = w.add("IFCGEOMETRICREPRESENTATIONCONTEXT", &format!("$,'Model',3,1.E-05,#{wcs},#{north}"));
    let body =
        w.add("IFCGEOMETRICREPRESENTATIONSUBCONTEXT", &format!("'Body','Model',*,*,*,*,#{ctx},$,.MODEL_VIEW.,$"));
    let pname = if model.meta.name.is_empty() { name.to_string() } else { model.meta.name.clone() };
    let g = guid.next();
    let project = w.add("IFCPROJECT", &format!("{g},#{owner},{},$,$,$,$,(#{ctx}),#{units}", encode(&pname)));
    let mut e = Exp {
        model,
        o,
        w,
        guid,
        owner,
        body_ctx: body,
        styles: HashMap::new(),
        counts: BTreeMap::new(),
        replaced: 0,
        origin,
        z,
        x,
    };
    let project = (project, Kind::Site);
    // Spatial structure: the standard hierarchy (site, building, storey) when asked.
    let hierarchy = if o.standard_hierarchy {
        let sp = e.placement(None, &DMat4::IDENTITY);
        let gs = e.guid.next();
        let sh = &model.shadow;
        let args =
            format!("{gs},#{owner},'Site',$,$,#{sp},$,$,.ELEMENT.,{},{},0.,$,$", dms(sh.latitude), dms(sh.longitude));
        let site = (e.w.add("IFCSITE", &args), Kind::Site);
        let bp = e.placement(Some(sp), &DMat4::IDENTITY);
        let gb = e.guid.next();
        let args = format!("{gb},#{owner},'Building',$,$,#{bp},$,$,.ELEMENT.,$,$,$");
        let building = (e.w.add("IFCBUILDING", &args), Kind::Building);
        let stp = e.placement(Some(bp), &DMat4::IDENTITY);
        let gst = e.guid.next();
        let args = format!("{gst},#{owner},'Storey',$,$,#{stp},$,$,.ELEMENT.,0.");
        let storey = (e.w.add("IFCBUILDINGSTOREY", &args), Kind::Storey);
        e.relate(project, &[site]);
        e.relate(site, &[building]);
        e.relate(building, &[storey]);
        Some(([site, building, storey], stp))
    } else {
        None
    };
    let root_place = match hierarchy {
        Some((_, p)) => p,
        None => e.placement(None, &DMat4::IDENTITY),
    };
    let mut top = Vec::new();
    // The model's loose geometry: one proxy.
    if let Some(shape) = e.shape(&model.root, None) {
        let p = e.placement(Some(root_place), &DMat4::IDENTITY);
        top.push(e.product("IfcBuildingElementProxy", &pname, p, Some(shape)));
    }
    top.extend(e.products(&model.root, root_place, None, 0));
    match hierarchy {
        Some(([site, building, storey], _)) => {
            // Each top-level product under the level it belongs to.
            let under = |k: Kind| match k {
                Kind::Site => project,
                Kind::Building => site,
                Kind::Storey => building,
                Kind::Space | Kind::Element => storey,
            };
            for parent in [project, site, building, storey] {
                let mine: Vec<Made> = top.iter().copied().filter(|c| under(c.1) == parent).collect();
                e.relate(parent, &mine);
            }
        }
        None => {
            let spatial: Vec<Made> = top.iter().copied().filter(|c| c.1 != Kind::Element).collect();
            let elements: Vec<Made> = top.iter().copied().filter(|c| c.1 == Kind::Element).collect();
            e.relate(project, &spatial);
            if !elements.is_empty() {
                // Elements need a spatial container: a building.
                let gb = e.guid.next();
                let args = format!("{gb},#{owner},'Building',$,$,#{root_place},$,$,.ELEMENT.,$,$,$");
                let building = (e.w.add("IFCBUILDING", &args), Kind::Building);
                e.relate(project, &[building]);
                e.relate(building, &elements);
            }
        }
    }
    let (schema, view) = match o.version {
        IfcVersion::Ifc2x3 => ("IFC2X3", "CoordinationView_V2.0"),
        IfcVersion::Ifc4 => ("IFC4", "ReferenceView_V1.2"),
    };
    let stamp = crate::dae::iso_now().trim_end_matches('Z').to_string();
    let mut text = String::new();
    let _ = writeln!(text, "ISO-10303-21;\nHEADER;");
    let _ = writeln!(text, "FILE_DESCRIPTION(('ViewDefinition [{view}]'),'2;1');");
    let file = encode(&format!("{name}.ifc"));
    let _ = writeln!(text, "FILE_NAME({file},'{stamp}',(''),(''),'Esquisse {version}','Esquisse {version}','');");
    let _ = writeln!(text, "FILE_SCHEMA(('{schema}'));\nENDSEC;\nDATA;");
    text.push_str(&e.w.out);
    text.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    (text, export_report(&e.counts, e.replaced))
}

fn export_report(counts: &BTreeMap<&'static str, usize>, replaced: usize) -> String {
    let mut r = String::from("Entités IFC exportées :\n");
    for (k, n) in counts {
        let _ = writeln!(r, "  {n} {k}");
    }
    match replaced {
        0 => {}
        1 => r.push_str(
            "1 type IFC a été remplacé par IfcBuildingElementProxy en raison d'une classification non valide.\n",
        ),
        n => {
            let _ = writeln!(
                r,
                "{n} types IFC ont été remplacés par IfcBuildingElementProxy en raison d'une classification non valide."
            );
        }
    }
    r
}

/// File › Export › 3D Model › IFC (.ifc, or .ifczip: the file zipped).
pub fn export(model: &Model, path: &Path, o: &IfcExport) -> Result<String> {
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "SketchUp".into());
    let (text, report) = write(model, o, &name);
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ifczip")) {
        let mut z = crate::zipfile::Writer::new();
        z.add(&format!("{name}.ifc"), text.as_bytes(), true, 1);
        std::fs::write(path, z.finish())?;
    } else {
        std::fs::write(path, text)?;
    }
    Ok(report)
}

// ------------------------------------------------------------------------------------------
// Import

/// Products that are not drawn (openings are cut out of their elements).
const SKIPPED: &[&str] = &[
    "IFCOPENINGELEMENT",
    "IFCOPENINGSTANDARDCASE",
    "IFCSPACE",
    "IFCSPATIALZONE",
    "IFCANNOTATION",
    "IFCGRID",
    "IFCVIRTUALELEMENT",
];

/// Spatial elements: groups holding their products.
const SPATIAL: &[&str] = &[
    "IFCSITE",
    "IFCBUILDING",
    "IFCBUILDINGSTOREY",
    "IFCEXTERNALSPATIALELEMENT",
    "IFCFACILITY",
    "IFCFACILITYPART",
    "IFCFACILITYPARTCOMMON",
    "IFCBRIDGE",
    "IFCBRIDGEPART",
    "IFCROAD",
    "IFCROADPART",
    "IFCRAILWAY",
    "IFCRAILWAYPART",
    "IFCMARINEFACILITY",
    "IFCMARINEPART",
];

/// Product classes of IFC 2x3 to IFC 4.3 besides [`CLASSES`], for their spelling.
const MORE_CLASSES: &[&str] = &[
    "IfcActuator",
    "IfcAirTerminal",
    "IfcAirTerminalBox",
    "IfcAirToAirHeatRecovery",
    "IfcAlarm",
    "IfcAlignment",
    "IfcAnnotation",
    "IfcAudioVisualAppliance",
    "IfcBeamStandardCase",
    "IfcBearing",
    "IfcBoiler",
    "IfcBorehole",
    "IfcBridge",
    "IfcBridgePart",
    "IfcBurner",
    "IfcCableCarrierFitting",
    "IfcCableCarrierSegment",
    "IfcCableFitting",
    "IfcCableSegment",
    "IfcCaissonFoundation",
    "IfcChiller",
    "IfcCivilElement",
    "IfcCoil",
    "IfcColumnStandardCase",
    "IfcCommunicationsAppliance",
    "IfcCompressor",
    "IfcCondenser",
    "IfcController",
    "IfcConveyorSegment",
    "IfcCooledBeam",
    "IfcCoolingTower",
    "IfcCourse",
    "IfcDamper",
    "IfcDeepFoundation",
    "IfcDistributionBoard",
    "IfcDistributionChamberElement",
    "IfcDistributionControlElement",
    "IfcDistributionElement",
    "IfcDistributionFlowElement",
    "IfcDoorStandardCase",
    "IfcDuctFitting",
    "IfcDuctSegment",
    "IfcDuctSilencer",
    "IfcEarthworksCut",
    "IfcEarthworksElement",
    "IfcEarthworksFill",
    "IfcElectricAppliance",
    "IfcElectricDistributionBoard",
    "IfcElectricFlowStorageDevice",
    "IfcElectricFlowTreatmentDevice",
    "IfcElectricGenerator",
    "IfcElectricMotor",
    "IfcElectricTimeControl",
    "IfcElementComponent",
    "IfcEnergyConversionDevice",
    "IfcEngine",
    "IfcEvaporativeCooler",
    "IfcEvaporator",
    "IfcExternalSpatialElement",
    "IfcFacility",
    "IfcFacilityPart",
    "IfcFacilityPartCommon",
    "IfcFan",
    "IfcFastener",
    "IfcFilter",
    "IfcFireSuppressionTerminal",
    "IfcFlowController",
    "IfcFlowInstrument",
    "IfcFlowMeter",
    "IfcFlowMovingDevice",
    "IfcFlowStorageDevice",
    "IfcFlowTreatmentDevice",
    "IfcGeomodel",
    "IfcGeoslice",
    "IfcGeotechnicalElement",
    "IfcGeotechnicalStratum",
    "IfcGrid",
    "IfcHeatExchanger",
    "IfcHumidifier",
    "IfcImpactProtectionDevice",
    "IfcInterceptor",
    "IfcJunctionBox",
    "IfcKerb",
    "IfcLamp",
    "IfcLightFixture",
    "IfcLinearElement",
    "IfcLiquidTerminal",
    "IfcMarineFacility",
    "IfcMarinePart",
    "IfcMechanicalFastener",
    "IfcMedicalDevice",
    "IfcMemberStandardCase",
    "IfcMobileTelecommunicationsAppliance",
    "IfcMooringDevice",
    "IfcMotorConnection",
    "IfcNavigationElement",
    "IfcOpeningElement",
    "IfcOutlet",
    "IfcPavement",
    "IfcPipeFitting",
    "IfcPipeSegment",
    "IfcPlateStandardCase",
    "IfcProjectionElement",
    "IfcProtectiveDevice",
    "IfcProtectiveDeviceTrippingUnit",
    "IfcProxy",
    "IfcPump",
    "IfcRail",
    "IfcRailway",
    "IfcRailwayPart",
    "IfcReferent",
    "IfcReinforcedSoil",
    "IfcReinforcingBar",
    "IfcReinforcingMesh",
    "IfcRoad",
    "IfcRoadPart",
    "IfcSanitaryTerminal",
    "IfcSensor",
    "IfcSign",
    "IfcSignal",
    "IfcSlabElementedCase",
    "IfcSlabStandardCase",
    "IfcSolarDevice",
    "IfcSolidStratum",
    "IfcSpaceHeater",
    "IfcSpatialZone",
    "IfcStackTerminal",
    "IfcSurfaceFeature",
    "IfcSwitchingDevice",
    "IfcSystemFurnitureElement",
    "IfcTank",
    "IfcTendon",
    "IfcTendonAnchor",
    "IfcTendonConduit",
    "IfcTrackElement",
    "IfcTransformer",
    "IfcTransportationDevice",
    "IfcTubeBundle",
    "IfcUnitaryControlElement",
    "IfcUnitaryEquipment",
    "IfcValve",
    "IfcVehicle",
    "IfcVibrationDamper",
    "IfcVibrationIsolator",
    "IfcVirtualElement",
    "IfcVoidingFeature",
    "IfcVoidStratum",
    "IfcWallElementedCase",
    "IfcWasteTerminal",
    "IfcWaterStratum",
    "IfcWindowStandardCase",
];

/// An IFC class name as SketchUp writes it ("IFCWALL" → "IfcWall").
fn class_name(ty: &str) -> String {
    let known = CLASSES.iter().map(|c| c.name).chain(MORE_CLASSES.iter().copied());
    if let Some(c) = known.into_iter().find(|k| k.eq_ignore_ascii_case(ty)) {
        return c.to_string();
    }
    let rest = ty.get(3..).unwrap_or("").to_ascii_lowercase();
    let mut cs = rest.chars();
    format!("Ifc{}", cs.next().map_or(String::new(), |f| f.to_ascii_uppercase().to_string() + cs.as_str()))
}

fn xyz(p: &P) -> DVec3 {
    let v = p.floats();
    DVec3::new(v.first().copied().unwrap_or(0.0), v.get(1).copied().unwrap_or(0.0), v.get(2).copied().unwrap_or(0.0))
}

fn ring(m: &DMat4, a: f64, b: f64) -> Vec<DVec3> {
    (0..24)
        .map(|k| {
            let t = k as f64 * TAU / 24.0;
            m.transform_point3(DVec3::new(a * t.cos(), b * t.sin(), 0.0))
        })
        .collect()
}

/// Points of the arc through three points.
fn arc3(a: DVec3, b: DVec3, c: DVec3) -> Vec<DVec3> {
    let (ab, ac) = (b - a, c - a);
    let n = ab.cross(ac);
    if n.length() < 1e-12 {
        return vec![a, b, c];
    }
    let center =
        a + (ac.length_squared() * n.cross(ab) + ab.length_squared() * ac.cross(n)) / (2.0 * n.length_squared());
    let nn = n.normalize();
    let ra = a - center;
    let v = c - center;
    let sweep = nn.dot(ra.cross(v)).atan2(ra.dot(v)).rem_euclid(TAU);
    let k = ((24.0 * sweep / TAU).round() as usize).max(2);
    (0..=k).map(|i| center + DQuat::from_axis_angle(nn, sweep * i as f64 / k as f64) * ra).collect()
}

/// Appends points, skipping repeats.
fn extend_unique(out: &mut Vec<DVec3>, pts: impl IntoIterator<Item = DVec3>) {
    for p in pts {
        if out.last().is_none_or(|q| q.distance(p) > 1e-12) {
            out.push(p);
        }
    }
}

/// A loop without its closing point.
fn open_loop(mut l: Vec<DVec3>) -> Vec<DVec3> {
    if l.len() > 1 && l[0].distance(l[l.len() - 1]) < 1e-9 {
        l.pop();
    }
    l
}

/// The faces of a profile swept along `d`, as loop lists (outer first): both ends and the
/// sides, all facing out.
fn prism(outer: &[DVec3], holes: &[Vec<DVec3>], d: DVec3) -> Vec<Vec<Vec<DVec3>>> {
    // Loops turned so that their normal goes along (`along`) or against `d`.
    let orient = |l: &[DVec3], along: bool| -> Vec<DVec3> {
        if (newell_normal(l).dot(d) > 0.0) == along { l.to_vec() } else { l.iter().rev().copied().collect() }
    };
    let bottom_outer = orient(outer, false);
    let bottom_holes: Vec<Vec<DVec3>> = holes.iter().map(|h| orient(h, true)).collect();
    let mut out = Vec::new();
    out.push(std::iter::once(bottom_outer.clone()).chain(bottom_holes.iter().cloned()).collect());
    let top = std::iter::once(orient(outer, true)).chain(holes.iter().map(|h| orient(h, false)));
    out.push(top.map(|l| l.iter().map(|p| *p + d).collect()).collect());
    for l in std::iter::once(&bottom_outer).chain(bottom_holes.iter()) {
        for i in 0..l.len() {
            let (a, b) = (l[i], l[(i + 1) % l.len()]);
            out.push(vec![vec![b, a, a + d, b + d]]);
        }
    }
    out
}

/// Adds a polygon (outer loop then holes) to a mesh.
fn add_poly(mesh: &mut MeshIn, loops: Vec<Vec<DVec3>>, m: &DMat4, mat: Option<u32>, layer: Option<u32>) {
    let mut it = loops.into_iter().filter(|l| l.len() >= 3);
    let Some(outer) = it.next() else { return };
    let mut add = |l: Vec<DVec3>| -> Vec<u32> {
        let b = mesh.pos.len() as u32;
        let n = l.len() as u32;
        mesh.pos.extend(l.into_iter().map(|p| m.transform_point3(p)));
        (b..b + n).collect()
    };
    let outer = add(outer);
    let holes = it.map(&mut add).collect();
    mesh.polys.push(PolyIn { outer, holes, mat, back: mat, layer, ..Default::default() });
}

struct Imp<'a> {
    f: &'a StepFile,
    scene: SceneIn,
    /// Surface style → material.
    styles: HashMap<u64, Option<u32>>,
    /// Representation item → its surface style.
    item_style: HashMap<u64, u64>,
    /// Representation map → definition.
    maps: HashMap<u64, usize>,
    /// Representation map → the name of the type it belongs to.
    map_names: HashMap<u64, String>,
    /// Representation item → IFC layer (with "Utiliser les calques IFC").
    item_layer: HashMap<u64, String>,
    /// Face set → its colour per face (IfcIndexedColourMap).
    colour_maps: HashMap<u64, ColourMap>,
}

/// The colours of an IfcIndexedColourMap and the (1-based) colour of each face.
struct ColourMap {
    colours: Vec<[u8; 3]>,
    index: Vec<usize>,
    opacity: f32,
}

impl Imp<'_> {
    /// The material of the `i`-th face of a face set with a colour map.
    fn face_colour(&mut self, set: u64, i: usize) -> Option<u32> {
        let map = self.colour_maps.get(&set)?;
        let c = *map.colours.get(map.index.get(i)?.wrapping_sub(1))?;
        let opacity = map.opacity;
        let name = format!("Couleur IFC {:02X}{:02X}{:02X}", c[0], c[1], c[2]);
        Some(self.scene.material_index(&name, c, opacity))
    }

    fn inst(&self, id: u64) -> Option<&step::Inst> {
        self.f.get(id)
    }

    fn point(&self, id: u64) -> DVec3 {
        self.inst(id).map_or(DVec3::ZERO, |i| xyz(i.arg(0)))
    }

    fn direction(&self, id: Option<u64>, default: DVec3) -> DVec3 {
        let Some(i) = id.and_then(|i| self.inst(i)) else { return default };
        xyz(i.arg(0)).normalize_or(default)
    }

    /// An IfcAxis2Placement2D/3D.
    fn axis(&self, id: Option<u64>) -> DMat4 {
        let Some(i) = id.and_then(|i| self.inst(i)) else { return DMat4::IDENTITY };
        let o = i.arg(0).as_ref().map_or(DVec3::ZERO, |p| self.point(p));
        let (z, xr) = if i.ty == "IFCAXIS2PLACEMENT2D" {
            (DVec3::Z, self.direction(i.arg(1).as_ref(), DVec3::X))
        } else {
            let z = self.direction(i.arg(1).as_ref(), DVec3::Z);
            (z, self.direction(i.arg(2).as_ref(), if z.x.abs() < 0.9 { DVec3::X } else { DVec3::Y }))
        };
        let x = (xr - z * xr.dot(z)).normalize_or(if z.x.abs() < 0.9 { DVec3::X } else { DVec3::Y });
        DMat4::from_cols(x.extend(0.0), z.cross(x).extend(0.0), z.extend(0.0), o.extend(1.0))
    }

    /// The world transform of an IfcLocalPlacement chain.
    fn placement(&self, id: Option<u64>, depth: usize) -> DMat4 {
        let Some(i) = id.and_then(|i| self.inst(i)) else { return DMat4::IDENTITY };
        if i.ty != "IFCLOCALPLACEMENT" || depth > 64 {
            return DMat4::IDENTITY;
        }
        self.placement(i.arg(0).as_ref(), depth + 1) * self.axis(i.arg(1).as_ref())
    }

    /// An IfcCartesianTransformationOperator (2D/3D, uniform or not).
    fn operator(&self, id: u64) -> DMat4 {
        let Some(o) = self.inst(id) else { return DMat4::IDENTITY };
        let three = o.ty.starts_with("IFCCARTESIANTRANSFORMATIONOPERATOR3D");
        let x0 = self.direction(o.arg(0).as_ref(), DVec3::X);
        let origin = o.arg(2).as_ref().map_or(DVec3::ZERO, |p| self.point(p));
        let s = o.arg(3).as_f64().unwrap_or(1.0);
        let z = if three { self.direction(o.arg(4).as_ref(), DVec3::Z) } else { DVec3::Z };
        let (s2, s3) = match (o.ty.ends_with("NONUNIFORM"), three) {
            (true, true) => (o.arg(5).as_f64().unwrap_or(s), o.arg(6).as_f64().unwrap_or(s)),
            (true, false) => (o.arg(4).as_f64().unwrap_or(s), s),
            _ => (s, s),
        };
        let x = (x0 - z * x0.dot(z)).normalize_or(DVec3::X);
        let y = z.cross(x);
        DMat4::from_cols((x * s).extend(0.0), (y * s2).extend(0.0), (z * s3).extend(0.0), origin.extend(1.0))
    }

    /// Points of an indexed point list (IfcCartesianPointList2D/3D).
    fn point_list(&self, id: Option<u64>) -> Vec<DVec3> {
        id.and_then(|c| self.inst(c)).map(|l| l.arg(0).list().iter().map(xyz).collect()).unwrap_or_default()
    }

    /// Points of a curve: polyline, indexed polycurve, composite curve, circle, ellipse,
    /// trimmed circle.
    fn curve(&self, id: u64, depth: usize) -> Vec<DVec3> {
        let Some(c) = self.inst(id) else { return Vec::new() };
        if depth > 16 {
            return Vec::new();
        }
        match c.ty.as_str() {
            "IFCPOLYLINE" => c.arg(0).refs().iter().map(|p| self.point(*p)).collect(),
            "IFCINDEXEDPOLYCURVE" => {
                let pts = self.point_list(c.arg(0).as_ref());
                if c.arg(1).list().is_empty() {
                    return pts;
                }
                let mut out = Vec::new();
                for seg in c.arg(1).list() {
                    let P::Typed(t, inner) = seg else { continue };
                    let sel: Vec<DVec3> = inner
                        .list()
                        .iter()
                        .filter_map(P::as_f64)
                        .filter_map(|i| pts.get((i as usize).wrapping_sub(1)).copied())
                        .collect();
                    if t == "IFCARCINDEX" && sel.len() == 3 {
                        extend_unique(&mut out, arc3(sel[0], sel[1], sel[2]));
                    } else {
                        extend_unique(&mut out, sel);
                    }
                }
                out
            }
            "IFCCOMPOSITECURVE" => {
                let mut out = Vec::new();
                for seg in c.arg(0).refs() {
                    let Some(s) = self.inst(seg) else { continue };
                    let mut pts = self.curve(s.arg(2).as_ref().unwrap_or(0), depth + 1);
                    if s.arg(1).as_enum() == Some("F") {
                        pts.reverse();
                    }
                    extend_unique(&mut out, pts);
                }
                out
            }
            "IFCCIRCLE" => {
                let r = c.arg(1).as_f64().unwrap_or(0.0);
                ring(&self.axis(c.arg(0).as_ref()), r, r)
            }
            "IFCELLIPSE" => {
                let (a, b) = (c.arg(1).as_f64().unwrap_or(0.0), c.arg(2).as_f64().unwrap_or(0.0));
                ring(&self.axis(c.arg(0).as_ref()), a, b)
            }
            "IFCTRIMMEDCURVE" => {
                let Some(base) = c.arg(0).as_ref().and_then(|b| self.inst(b)) else { return Vec::new() };
                if base.ty != "IFCCIRCLE" {
                    return self.curve(c.arg(0).as_ref().unwrap_or(0), depth + 1);
                }
                let m = self.axis(base.arg(0).as_ref());
                let r = base.arg(1).as_f64().unwrap_or(0.0);
                let par = |p: &P| {
                    p.list().iter().find_map(|x| match x {
                        P::Typed(t, v) if t == "IFCPARAMETERVALUE" => v.as_f64(),
                        _ => None,
                    })
                };
                let (Some(t1), Some(t2)) = (par(c.arg(1)), par(c.arg(2))) else {
                    return self.curve(c.arg(0).as_ref().unwrap_or(0), depth + 1);
                };
                // Parameters in degrees, as most files write plane angles.
                let (t1, t2) = (t1.to_radians(), t2.to_radians());
                let sense = c.arg(3).as_enum() != Some("F");
                let sweep = if sense { t2 - t1 } else { t1 - t2 }.rem_euclid(TAU);
                let sweep = if sweep < 1e-9 { TAU } else { sweep };
                let n = ((24.0 * sweep / TAU).round() as usize).max(1);
                let sign = if sense { 1.0 } else { -1.0 };
                (0..=n)
                    .map(|k| {
                        let a = t1 + sign * sweep * k as f64 / n as f64;
                        m.transform_point3(DVec3::new(r * a.cos(), r * a.sin(), 0.0))
                    })
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// The outer loop and holes of a profile, in the profile's plane.
    fn profile(&self, id: u64, depth: usize) -> Option<(Vec<DVec3>, Vec<Vec<DVec3>>)> {
        let p = self.inst(id)?;
        if depth > 8 {
            return None;
        }
        let pos = |k: usize| self.axis(p.arg(k).as_ref());
        let f = |k: usize| p.arg(k).as_f64();
        let at = |m: &DMat4, pts: &[(f64, f64)]| -> Vec<DVec3> {
            pts.iter().map(|(x, y)| m.transform_point3(DVec3::new(*x, *y, 0.0))).collect()
        };
        Some(match p.ty.as_str() {
            "IFCARBITRARYCLOSEDPROFILEDEF" => (open_loop(self.curve(p.arg(2).as_ref()?, 0)), Vec::new()),
            "IFCARBITRARYPROFILEDEFWITHVOIDS" => {
                let holes = p.arg(3).refs().iter().map(|h| open_loop(self.curve(*h, 0))).collect();
                (open_loop(self.curve(p.arg(2).as_ref()?, 0)), holes)
            }
            "IFCRECTANGLEPROFILEDEF" | "IFCROUNDEDRECTANGLEPROFILEDEF" | "IFCRECTANGLEHOLLOWPROFILEDEF" => {
                let m = pos(2);
                let (x, y) = (f(3)? / 2.0, f(4)? / 2.0);
                let rect = |x: f64, y: f64| [(-x, -y), (x, -y), (x, y), (-x, y)];
                let holes = if p.ty == "IFCRECTANGLEHOLLOWPROFILEDEF" {
                    let t = f(5).unwrap_or(0.0);
                    vec![at(&m, &rect(x - t, y - t))]
                } else {
                    Vec::new()
                };
                (at(&m, &rect(x, y)), holes)
            }
            "IFCCIRCLEPROFILEDEF" | "IFCCIRCLEHOLLOWPROFILEDEF" => {
                let m = pos(2);
                let r = f(3)?;
                let holes = if p.ty == "IFCCIRCLEHOLLOWPROFILEDEF" {
                    let t = f(4).unwrap_or(0.0);
                    vec![ring(&m, r - t, r - t)]
                } else {
                    Vec::new()
                };
                (ring(&m, r, r), holes)
            }
            "IFCELLIPSEPROFILEDEF" => (ring(&pos(2), f(3)?, f(4)?), Vec::new()),
            "IFCISHAPEPROFILEDEF" => {
                let (w, d, tw, tf) = (f(3)? / 2.0, f(4)? / 2.0, f(5)? / 2.0, f(6)?);
                let pts = [
                    (-w, -d),
                    (w, -d),
                    (w, -d + tf),
                    (tw, -d + tf),
                    (tw, d - tf),
                    (w, d - tf),
                    (w, d),
                    (-w, d),
                    (-w, d - tf),
                    (-tw, d - tf),
                    (-tw, -d + tf),
                    (-w, -d + tf),
                ];
                (at(&pos(2), &pts), Vec::new())
            }
            "IFCLSHAPEPROFILEDEF" => {
                let d = f(3)?;
                let (w, t) = (f(4).unwrap_or(d), f(5)?);
                let pts = [(0.0, 0.0), (w, 0.0), (w, t), (t, t), (t, d), (0.0, d)];
                let centred: Vec<(f64, f64)> = pts.iter().map(|(x, y)| (x - w / 2.0, y - d / 2.0)).collect();
                (at(&pos(2), &centred), Vec::new())
            }
            "IFCUSHAPEPROFILEDEF" => {
                let (d, w, tw, tf) = (f(3)? / 2.0, f(4)?, f(5)?, f(6)?);
                let x0 = -w / 2.0;
                let pts = [
                    (x0, -d),
                    (x0 + w, -d),
                    (x0 + w, -d + tf),
                    (x0 + tw, -d + tf),
                    (x0 + tw, d - tf),
                    (x0 + w, d - tf),
                    (x0 + w, d),
                    (x0, d),
                ];
                (at(&pos(2), &pts), Vec::new())
            }
            "IFCTSHAPEPROFILEDEF" => {
                let (d, w, tw, tf) = (f(3)? / 2.0, f(4)? / 2.0, f(5)? / 2.0, f(6)?);
                let pts =
                    [(-tw, -d), (tw, -d), (tw, d - tf), (w, d - tf), (w, d), (-w, d), (-w, d - tf), (-tw, d - tf)];
                (at(&pos(2), &pts), Vec::new())
            }
            "IFCDERIVEDPROFILEDEF" => {
                let (o, h) = self.profile(p.arg(2).as_ref()?, depth + 1)?;
                let t = p.arg(3).as_ref().map_or(DMat4::IDENTITY, |op| self.operator(op));
                let tr = |l: &Vec<DVec3>| l.iter().map(|q| t.transform_point3(*q)).collect::<Vec<_>>();
                (tr(&o), h.iter().map(tr).collect())
            }
            _ => return None,
        })
    }

    /// The material of an item's style.
    fn material_of(&mut self, item: u64) -> Option<u32> {
        let style = *self.item_style.get(&item)?;
        if let Some(&m) = self.styles.get(&style) {
            return m;
        }
        let s = self.inst(style)?;
        let name = s.arg(0).as_str().filter(|n| !n.trim().is_empty()).unwrap_or("Matière IFC").to_string();
        let mut color = None;
        let mut opacity = 1.0f32;
        for r in s.arg(2).refs() {
            let Some(sh) = self.inst(r) else { continue };
            if sh.ty == "IFCSURFACESTYLESHADING" || sh.ty == "IFCSURFACESTYLERENDERING" {
                if let Some(c) = sh.arg(0).as_ref().and_then(|c| self.inst(c)) {
                    let v = |k: usize| (c.arg(k).as_f64().unwrap_or(0.8).clamp(0.0, 1.0) * 255.0).round() as u8;
                    color = Some([v(1), v(2), v(3)]);
                }
                if let Some(t) = sh.arg(1).as_f64() {
                    opacity = (1.0 - t).clamp(0.0, 1.0) as f32;
                }
            }
        }
        let m = color.map(|color| {
            let i = self.scene.material_index(&name, color, opacity);
            self.scene.materials[i as usize].color = color;
            self.scene.materials[i as usize].opacity = opacity;
            i
        });
        self.styles.insert(style, m);
        m
    }

    /// The geometry of a representation item, transformed by `m`, into `mesh`; mapped items
    /// become instances in `children`.
    fn item(
        &mut self,
        id: u64,
        m: DMat4,
        mesh: &mut MeshIn,
        children: &mut Vec<NodeIn>,
        mat: Option<u32>,
        depth: usize,
    ) {
        let Some(it) = self.inst(id).cloned() else { return };
        if depth > 32 {
            return;
        }
        let mat = self.material_of(id).or(mat);
        let layer = self.item_layer.get(&id).cloned().map(|l| self.scene.layer_index(&l));
        match it.ty.as_str() {
            "IFCEXTRUDEDAREASOLID" | "IFCEXTRUDEDAREASOLIDTAPERED" => {
                let Some((outer, holes)) = it.arg(0).as_ref().and_then(|p| self.profile(p, 0)) else { return };
                if outer.len() < 3 {
                    return;
                }
                let pos = m * self.axis(it.arg(1).as_ref());
                let d = self.direction(it.arg(2).as_ref(), DVec3::Z) * it.arg(3).as_f64().unwrap_or(0.0);
                for loops in prism(&outer, &holes, d) {
                    add_poly(mesh, loops, &pos, mat, layer);
                }
            }
            "IFCREVOLVEDAREASOLID" => {
                let Some((outer, _)) = it.arg(0).as_ref().and_then(|p| self.profile(p, 0)) else { return };
                let pos = m * self.axis(it.arg(1).as_ref());
                let (ap, ad) = match it.arg(2).as_ref().and_then(|a| self.inst(a)) {
                    Some(a) => (
                        a.arg(0).as_ref().map_or(DVec3::ZERO, |p| self.point(p)),
                        self.direction(a.arg(1).as_ref(), DVec3::Y),
                    ),
                    None => (DVec3::ZERO, DVec3::Y),
                };
                let angle = it.arg(3).as_f64().unwrap_or(TAU);
                // Plane angles in degrees when larger than a turn in radians.
                let angle = if angle.abs() > TAU + 1e-9 { angle.to_radians() } else { angle };
                // More than a turn is a turn: an angle of 1e12 made billions of rings.
                let angle = if angle.is_finite() { angle.clamp(-TAU, TAU) } else { TAU };
                let n = ((24.0 * angle.abs() / TAU).round() as usize).max(2);
                let rings: Vec<Vec<DVec3>> = (0..=n)
                    .map(|k| {
                        let r = DQuat::from_axis_angle(ad, angle * k as f64 / n as f64);
                        outer.iter().map(|p| ap + r * (*p - ap)).collect()
                    })
                    .collect();
                for k in 0..n {
                    for i in 0..outer.len() {
                        let j = (i + 1) % outer.len();
                        let quad = vec![rings[k][i], rings[k][j], rings[k + 1][j], rings[k + 1][i]];
                        add_poly(mesh, vec![quad], &pos, mat, layer);
                    }
                }
                if angle.abs() < TAU - 1e-6 {
                    add_poly(mesh, vec![rings[0].iter().rev().copied().collect()], &pos, mat, layer);
                    add_poly(mesh, vec![rings[n].clone()], &pos, mat, layer);
                }
            }
            "IFCFACETEDBREP" | "IFCFACETEDBREPWITHVOIDS" | "IFCADVANCEDBREP" => {
                let shells = std::iter::once(it.arg(0).as_ref().unwrap_or(0)).chain(it.arg(1).refs());
                for s in shells {
                    self.shell(s, &m, mesh, mat, layer);
                }
            }
            "IFCSHELLBASEDSURFACEMODEL" | "IFCFACEBASEDSURFACEMODEL" => {
                for s in it.arg(0).refs() {
                    self.shell(s, &m, mesh, mat, layer);
                }
            }
            "IFCCLOSEDSHELL" | "IFCOPENSHELL" | "IFCCONNECTEDFACESET" => self.shell(id, &m, mesh, mat, layer),
            "IFCTRIANGULATEDFACESET" | "IFCTRIANGULATEDIRREGULARNETWORK" => {
                let pts = self.point_list(it.arg(0).as_ref());
                // PnIndex (the last attribute) remaps the indices when present.
                let pn: Vec<usize> = it.arg(4).floats().iter().map(|x| *x as usize).collect();
                let get = |i: f64| {
                    let i = i as usize;
                    let j = if pn.is_empty() { Some(i) } else { pn.get(i.wrapping_sub(1)).copied() };
                    j.and_then(|j| pts.get(j.wrapping_sub(1)).copied())
                };
                for (k, t) in it.arg(3).list().iter().enumerate() {
                    let l: Vec<DVec3> = t.floats().into_iter().filter_map(get).collect();
                    let mat = self.face_colour(id, k).or(mat);
                    add_poly(mesh, vec![l], &m, mat, layer);
                }
            }
            "IFCPOLYGONALFACESET" => {
                let pts = self.point_list(it.arg(0).as_ref());
                let pn: Vec<usize> = it.arg(3).floats().iter().map(|x| *x as usize).collect();
                let pick = |l: &P| -> Vec<DVec3> {
                    l.floats()
                        .into_iter()
                        .filter_map(|i| {
                            let i = i as usize;
                            let j = if pn.is_empty() { Some(i) } else { pn.get(i.wrapping_sub(1)).copied() };
                            j.and_then(|j| pts.get(j.wrapping_sub(1)).copied())
                        })
                        .collect()
                };
                for (k, fid) in it.arg(2).refs().into_iter().enumerate() {
                    let Some(f) = self.inst(fid) else { continue };
                    let mut loops = vec![pick(f.arg(0))];
                    if f.ty == "IFCINDEXEDPOLYGONALFACEWITHVOIDS" {
                        loops.extend(f.arg(1).list().iter().map(pick));
                    }
                    let mat = self.face_colour(id, k).or(mat);
                    add_poly(mesh, loops, &m, mat, layer);
                }
            }
            "IFCMAPPEDITEM" => {
                let Some(map) = it.arg(0).as_ref() else { return };
                let Some(rm) = self.inst(map).cloned() else { return };
                let target = it.arg(1).as_ref().map_or(DMat4::IDENTITY, |t| self.operator(t));
                // The map's items are given in the frame of its origin.
                let origin = self.axis(rm.arg(0).as_ref());
                let d = match self.maps.get(&map) {
                    Some(&d) => d,
                    None => {
                        // The definition is reserved before its items are read: the map was
                        // read again by every item that maps it, so a map of two items mapping
                        // it again made 2^32 readings (and a map of itself, a definition that
                        // contains itself: cut when the model is built).
                        let name = self.map_names.get(&map).cloned().unwrap_or_else(|| "Élément IFC".into());
                        let idx = self.scene.defs.len();
                        self.scene.defs.push(DefIn { name, meshes: Vec::new(), children: Vec::new() });
                        self.maps.insert(map, idx);
                        let mut sub = MeshIn::default();
                        let mut sub_children = Vec::new();
                        let rep = rm.arg(1).as_ref().and_then(|r| self.inst(r));
                        for i in rep.map(|r| r.arg(3).refs()).unwrap_or_default() {
                            self.item(i, DMat4::IDENTITY, &mut sub, &mut sub_children, mat, depth + 1);
                        }
                        let mi = self.scene.add_mesh(sub);
                        self.scene.defs[idx].meshes = vec![mi];
                        self.scene.defs[idx].children = sub_children;
                        idx
                    }
                };
                children.push(NodeIn { def: Some(d), xform: m * target * origin, layer, ..Default::default() });
            }
            "IFCBOOLEANCLIPPINGRESULT" | "IFCBOOLEANRESULT" => {
                // The first operand; half-space clippings are left out.
                if let Some(a) = it.arg(1).as_ref() {
                    self.item(a, m, mesh, children, mat, depth + 1);
                }
            }
            _ => {}
        }
    }

    /// The faces of an IfcClosedShell / IfcOpenShell.
    fn shell(&self, id: u64, m: &DMat4, mesh: &mut MeshIn, mat: Option<u32>, layer: Option<u32>) {
        let Some(s) = self.inst(id) else { return };
        for f in s.arg(0).refs() {
            let Some(face) = self.inst(f) else { continue };
            let mut loops: Vec<(bool, Vec<DVec3>)> = Vec::new();
            for b in face.arg(0).refs() {
                let Some(bound) = self.inst(b) else { continue };
                let Some(lp) = bound.arg(0).as_ref().and_then(|l| self.inst(l)) else { continue };
                let mut pts: Vec<DVec3> = lp.arg(0).refs().iter().map(|p| self.point(*p)).collect();
                if bound.arg(1).as_enum() == Some("F") {
                    pts.reverse();
                }
                loops.push((bound.ty == "IFCFACEOUTERBOUND", pts));
            }
            // The outer bound first (or the first bound when none is marked).
            loops.sort_by_key(|l| !l.0);
            add_poly(mesh, loops.into_iter().map(|l| l.1).collect(), m, mat, layer);
        }
    }

    /// The geometry of a product's body representations.
    fn body(&mut self, shape: Option<u64>, m: DMat4, mesh: &mut MeshIn, children: &mut Vec<NodeIn>) {
        let Some(shape) = shape.and_then(|s| self.inst(s)).cloned() else { return };
        let reps = shape.arg(2).refs();
        let id = |r: &u64| self.inst(*r).and_then(|x| x.arg(1).as_str()).unwrap_or("").to_string();
        let mut chosen: Vec<u64> = reps
            .iter()
            .copied()
            .filter(|r| matches!(id(r).as_str(), "Body" | "Facetation" | "Body-FallBack"))
            .collect();
        if chosen.is_empty() {
            let other = |r: &u64| !matches!(id(r).as_str(), "Axis" | "FootPrint" | "Box" | "Annotation");
            chosen = reps.iter().copied().filter(other).take(1).collect();
        }
        for r in chosen {
            for i in self.inst(r).map(|x| x.arg(3).refs()).unwrap_or_default() {
                self.item(i, m, mesh, children, None, 0);
            }
        }
    }
}

/// What the reader gives the importer.
struct Read {
    scene: SceneIn,
    /// Millimetres per file unit.
    mm: f64,
    schema: &'static str,
    /// Definitions of products, with their IFC class.
    classes: Vec<(usize, String)>,
    /// Openings to cut out of a product's definition, in its coordinates (file units).
    openings: Vec<(usize, MeshIn)>,
    report: String,
}

/// The length unit of a file, in millimetres: the project's, else the first one found.
fn length_unit(f: &StepFile) -> f64 {
    let of = |u: &step::Inst| -> Option<f64> {
        if u.arg(1).as_enum() != Some("LENGTHUNIT") {
            return None;
        }
        match u.ty.as_str() {
            "IFCSIUNIT" if u.arg(3).as_enum() == Some("METRE") => Some(match u.arg(2).as_enum() {
                Some("MILLI") => 1.0,
                Some("CENTI") => 10.0,
                Some("DECI") => 100.0,
                Some("KILO") => 1_000_000.0,
                _ => 1000.0,
            }),
            "IFCCONVERSIONBASEDUNIT" => {
                let n = u.arg(2).as_str().unwrap_or("").to_ascii_uppercase();
                if n.contains("FOOT") || n.contains("FEET") {
                    Some(304.8)
                } else if n.contains("INCH") {
                    Some(25.4)
                } else {
                    None
                }
            }
            _ => None,
        }
    };
    let project = f.insts.values().find(|i| i.ty == "IFCPROJECT" || i.ty == "IFCPROJECTLIBRARY");
    let assigned = project.and_then(|p| f.get(p.arg(8).as_ref()?)).map(|a| a.arg(0).refs()).unwrap_or_default();
    if let Some(mm) = assigned.iter().filter_map(|u| f.get(*u)).find_map(of) {
        return mm;
    }
    let mut ids: Vec<&u64> = f.insts.keys().collect();
    ids.sort_unstable();
    ids.into_iter().filter_map(|k| f.get(*k)).find_map(of).unwrap_or(1000.0)
}

/// The tree of nodes: each node inside its nearest imported ancestor, placed relative to it.
struct Tree<'a> {
    nodes: HashMap<u64, NodeIn>,
    children_of: BTreeMap<u64, Vec<u64>>,
    world: HashMap<u64, DMat4>,
    defs: &'a mut Vec<DefIn>,
}

impl Tree<'_> {
    fn attach(&mut self, id: u64, parent_world: DMat4, depth: usize) -> Option<NodeIn> {
        let mut n = self.nodes.remove(&id)?;
        let w = self.world.get(&id).copied().unwrap_or(DMat4::IDENTITY);
        n.xform = parent_world.inverse() * w;
        let mut kids = Vec::new();
        if depth < 64 {
            for c in self.children_of.get(&id).cloned().unwrap_or_default() {
                kids.extend(self.attach(c, w, depth + 1));
            }
        }
        match n.def {
            // Parts of an element (aggregated) go into its component.
            Some(d) => self.defs[d].children.extend(kids),
            None => n.children.extend(kids),
        }
        (n.def.is_some() || !n.children.is_empty() || !n.meshes.is_empty()).then_some(n)
    }
}

/// Reads an IFC file: one group per spatial element (site, building, storey), one
/// component per product, openings to cut.
fn read(text: &str, o: &IfcImport) -> Result<Read> {
    let f = step::read(text)?;
    if !f.schema.starts_with("IFC") {
        bail!("schéma non IFC : {}", f.schema);
    }
    let schema = if f.schema.starts_with("IFC2X") { "IFC 2x3" } else { "IFC 4" };
    let mut imp = Imp {
        f: &f,
        scene: SceneIn::default(),
        styles: HashMap::new(),
        item_style: HashMap::new(),
        maps: HashMap::new(),
        map_names: HashMap::new(),
        item_layer: HashMap::new(),
        colour_maps: HashMap::new(),
    };
    // Colours per face of face sets.
    for (_, c) in f.of_type("IFCINDEXEDCOLOURMAP") {
        let Some(set) = c.arg(0).as_ref() else { continue };
        let colours = c.arg(2).as_ref().and_then(|l| f.get(l)).map(|l| l.arg(0).list().to_vec()).unwrap_or_default();
        let colours = colours
            .iter()
            .map(|rgb| {
                let v = rgb.floats();
                let ch = |k: usize| (v.get(k).copied().unwrap_or(0.8).clamp(0.0, 1.0) * 255.0).round() as u8;
                [ch(0), ch(1), ch(2)]
            })
            .collect();
        let index = c.arg(3).floats().iter().map(|x| *x as usize).collect();
        let opacity = c.arg(1).as_f64().unwrap_or(1.0).clamp(0.0, 1.0) as f32;
        imp.colour_maps.insert(set, ColourMap { colours, index, opacity });
    }
    // Styles of representation items.
    for (_, s) in f.of_type("IFCSTYLEDITEM") {
        let Some(item) = s.arg(0).as_ref() else { continue };
        let style = s.arg(1).refs().into_iter().find_map(|st| {
            let x = f.get(st)?;
            match x.ty.as_str() {
                "IFCSURFACESTYLE" => Some(st),
                "IFCPRESENTATIONSTYLEASSIGNMENT" => {
                    x.arg(0).refs().into_iter().find(|r| f.get(*r).is_some_and(|y| y.ty == "IFCSURFACESTYLE"))
                }
                _ => None,
            }
        });
        if let Some(st) = style {
            imp.item_style.insert(item, st);
        }
    }
    // Representation maps of types: named after their type.
    for t in f.insts.values() {
        if t.args.len() > 6
            && (t.ty.ends_with("TYPE") || t.ty.ends_with("STYLE"))
            && let Some(n) = t.arg(2).as_str().filter(|n| !n.is_empty())
        {
            for r in t.arg(6).refs() {
                imp.map_names.insert(r, n.to_string());
            }
        }
    }
    // IFC layers.
    if o.tags == IfcTags::IfcLayers {
        for (_, l) in f.of_type("IFCPRESENTATIONLAYERASSIGNMENT").chain(f.of_type("IFCPRESENTATIONLAYERWITHSTYLE")) {
            let name = l.arg(0).as_str().unwrap_or("").to_string();
            for item in l.arg(2).refs() {
                imp.item_layer.insert(item, name.clone());
                // A layer of a shape representation applies to its items.
                if let Some(rep) = f.get(item).filter(|r| r.ty == "IFCSHAPEREPRESENTATION") {
                    for i in rep.arg(3).refs() {
                        imp.item_layer.entry(i).or_insert_with(|| name.clone());
                    }
                }
            }
        }
    }
    // The parent of each product: aggregation, nesting, then spatial containment.
    let mut parent: HashMap<u64, u64> = HashMap::new();
    for (_, r) in f.of_type("IFCRELAGGREGATES").chain(f.of_type("IFCRELNESTS")) {
        if let Some(p) = r.arg(4).as_ref() {
            for c in r.arg(5).refs() {
                parent.insert(c, p);
            }
        }
    }
    for (_, r) in f.of_type("IFCRELCONTAINEDINSPATIALSTRUCTURE") {
        if let Some(p) = r.arg(5).as_ref() {
            for c in r.arg(4).refs() {
                parent.entry(c).or_insert(p);
            }
        }
    }
    let mut voids: HashMap<u64, Vec<u64>> = HashMap::new();
    for (_, r) in f.of_type("IFCRELVOIDSELEMENT") {
        if let (Some(e), Some(op)) = (r.arg(4).as_ref(), r.arg(5).as_ref()) {
            voids.entry(e).or_default().push(op);
        }
    }
    // Products: whatever has an object placement (attribute 6) and may have a shape (7).
    let mut products: Vec<u64> = f
        .insts
        .iter()
        .filter(|(_, i)| {
            i.args.len() >= 7
                && !SKIPPED.contains(&i.ty.as_str())
                && i.arg(5).as_ref().and_then(|p| f.get(p)).is_some_and(|p| p.ty == "IFCLOCALPLACEMENT")
        })
        .map(|(k, _)| *k)
        .collect();
    products.sort_unstable();
    let mut nodes: HashMap<u64, NodeIn> = HashMap::new();
    let mut world: HashMap<u64, DMat4> = HashMap::new();
    let mut classes = Vec::new();
    let mut openings = Vec::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for &pid in &products {
        let Some(p) = f.get(pid) else { continue };
        let place = imp.placement(p.arg(5).as_ref(), 0);
        let class = class_name(&p.ty);
        let name = p.arg(2).as_str().filter(|s| !s.trim().is_empty()).map_or_else(|| class.clone(), str::to_string);
        let mut node = NodeIn { name: name.clone(), xform: place, ..Default::default() };
        let mut mesh = MeshIn::default();
        let mut children = Vec::new();
        imp.body(p.arg(6).as_ref(), DMat4::IDENTITY, &mut mesh, &mut children);
        let spatial = SPATIAL.contains(&p.ty.as_str());
        if !spatial {
            *counts.entry(class.clone()).or_default() += 1;
        }
        let drawn = !mesh.polys.is_empty() || !children.is_empty();
        if !spatial && drawn {
            // The product is a component of its own, classified as its IFC class.
            let mi = imp.scene.add_mesh(mesh);
            imp.scene.defs.push(DefIn { name: name.clone(), meshes: vec![mi], children });
            let di = imp.scene.defs.len() - 1;
            node.def = Some(di);
            classes.push((di, class.clone()));
            if o.tags == IfcTags::IfcClasses {
                node.layer = Some(imp.scene.layer_index(&class));
            }
            // Its openings, in its coordinates.
            for op in voids.get(&pid).into_iter().flatten() {
                let Some(oe) = f.get(*op) else { continue };
                let local = place.inverse() * imp.placement(oe.arg(5).as_ref(), 0);
                let mut hole = MeshIn::default();
                let mut ignored = Vec::new();
                imp.body(oe.arg(6).as_ref(), local, &mut hole, &mut ignored);
                if !hole.polys.is_empty() {
                    openings.push((di, hole));
                }
            }
        } else if drawn {
            // Geometry of a spatial element (a site's terrain): loose in its group.
            node.meshes.push(imp.scene.add_mesh(mesh));
            node.children = children;
        }
        world.insert(pid, place);
        nodes.insert(pid, node);
    }
    let mut children_of: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    let mut roots = Vec::new();
    for &id in &products {
        if !nodes.contains_key(&id) {
            continue;
        }
        let mut p = parent.get(&id).copied();
        for _ in 0..64 {
            match p {
                Some(q) if !nodes.contains_key(&q) => p = parent.get(&q).copied(),
                _ => break,
            }
        }
        match p.filter(|q| nodes.contains_key(q) && *q != id) {
            Some(q) => children_of.entry(q).or_default().push(id),
            None => roots.push(id),
        }
    }
    let mut defs = std::mem::take(&mut imp.scene.defs);
    let mut tree = Tree { nodes, children_of, world, defs: &mut defs };
    let mut top = Vec::new();
    for r in roots {
        top.extend(tree.attach(r, DMat4::IDENTITY, 0));
    }
    imp.scene.defs = defs;
    imp.scene.root.children = top;
    let mut report = String::from("Entités IFC importées :\n");
    for (k, n) in &counts {
        let _ = writeln!(report, "  {n} {k}");
    }
    Ok(Read { mm: length_unit(&f), scene: imp.scene, schema, classes, openings, report })
}

/// Cuts `hole` (a closed volume) out of a definition with the solid tools. False when either
/// is not a solid.
fn cut(model: &mut Model, def: DefId, hole: Entities) -> bool {
    let Some(d) = model.defs.get(def) else { return false };
    if !d.entities.instances.is_empty() {
        return false;
    }
    let mut scratch = Model::new();
    let group = |name: &str, entities: Entities| Definition {
        name: name.into(),
        is_group: true,
        description: String::new(),
        entities,
        attributes: Default::default(),
        ext: Default::default(),
    };
    let place = |def| Instance {
        def,
        xform: DMat4::IDENTITY,
        name: String::new(),
        material: None,
        layer: 0,
        hidden: false,
        locked: false,
        attributes: Default::default(),
        ext: Default::default(),
    };
    let body = scratch.defs.insert(group("corps", d.entities.clone()));
    let opening = scratch.defs.insert(group("ouverture", hole));
    let b = scratch.root.instances.insert(place(body));
    let o = scratch.root.instances.insert(place(opening));
    let Ok(made) = scratch.solid_op(None, o, b, SolidOp::Subtract) else { return false };
    let Some(&r) = made.first() else { return false };
    let inst = scratch.root.instances[r].clone();
    let mut e = scratch.defs[inst.def].entities.clone();
    let all: HashSet<_> = e.verts.keys().collect();
    e.transform_vertices(&all, &inst.xform);
    model.defs[def].entities = e;
    true
}

/// File › Import of an IFC file (or an .ifczip archive holding one).
pub fn import(bytes: &[u8], o: &IfcImport) -> Result<(Model, String)> {
    let text = if crate::zipfile::is_zip(bytes) {
        let entries = crate::zipfile::read(bytes)?;
        let e = entries
            .iter()
            .find(|e| e.name.to_lowercase().ends_with(".ifc"))
            .ok_or_else(|| anyhow::anyhow!("aucun fichier .ifc dans l'archive"))?;
        String::from_utf8_lossy(&e.data).into_owned()
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    let r = read(&text, o)?;
    let opts = BuildOpts { scale: r.mm, swap_yz: false, merge_coplanar: true, orient_faces: false };
    let (mut model, defs) = crate::build::build_mapped(&r.scene, opts);
    // The class of each product, where SketchUp's classifier keeps it.
    for (d, class) in &r.classes {
        if let Some(def) = defs.get(d).and_then(|id| model.defs.get_mut(*id)) {
            esq_core::model::set_attr(&mut def.attributes, APPLIED, r.schema, AttrValue::Text(class.clone()));
        }
    }
    let mut report = r.report;
    let (mut done, mut tried) = (0, 0);
    for (d, hole) in r.openings {
        let Some(&def) = defs.get(&d) else { continue };
        let scene = SceneIn {
            meshes: vec![hole],
            root: NodeIn { meshes: vec![0], ..Default::default() },
            ..Default::default()
        };
        tried += 1;
        if cut(&mut model, def, crate::build::build(&scene, opts).root) {
            done += 1;
        }
    }
    if tried > 0 {
        let _ = writeln!(report, "Ouvertures découpées : {done} sur {tried}");
    }
    Ok((model, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance(def: DefId, xform: DMat4, material: Option<MaterialId>) -> Instance {
        Instance {
            def,
            xform,
            name: String::new(),
            material,
            layer: 0,
            hidden: false,
            locked: false,
            attributes: Default::default(),
            ext: Default::default(),
        }
    }

    fn classified_def(name: &str, entities: Entities, class: &str, is_group: bool) -> Definition {
        let mut attributes = esq_core::model::Attributes::default();
        esq_core::model::set_attr(&mut attributes, APPLIED, "IFC 4", AttrValue::Text(class.into()));
        Definition {
            name: name.into(),
            is_group,
            description: String::new(),
            entities,
            attributes,
            ext: Default::default(),
        }
    }

    /// A 1 m × 0.2 m × 2.5 m wall component classified IfcWall, painted brick.
    fn wall_model() -> Model {
        let mut m = Model::new();
        let mut e = Entities::default();
        let p =
            [DVec3::ZERO, DVec3::new(1000.0, 0.0, 0.0), DVec3::new(1000.0, 200.0, 0.0), DVec3::new(0.0, 200.0, 0.0)];
        let v: Vec<_> = p.iter().map(|x| e.add_vertex_raw(*x)).collect();
        let top: Vec<_> = p.iter().map(|x| e.add_vertex_raw(*x + DVec3::Z * 2500.0)).collect();
        e.add_face_raw(vec![v[0], v[3], v[2], v[1]], vec![]);
        e.add_face_raw(top.clone(), vec![]);
        for i in 0..4 {
            let j = (i + 1) % 4;
            e.add_face_raw(vec![v[i], v[j], top[j], top[i]], vec![]);
        }
        let d = m.defs.insert(classified_def("Mur", e, "IfcWall", false));
        let brick = m.add_material("Brique", [180, 80, 60]);
        m.root.instances.insert(instance(d, DMat4::from_translation(DVec3::new(500.0, 0.0, 0.0)), Some(brick)));
        m
    }

    fn classified(m: &Model) -> Vec<(&Definition, String)> {
        m.defs
            .values()
            .filter_map(|d| match d.attributes.get(APPLIED)?.values().next()? {
                AttrValue::Text(t) => Some((d, t.clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn exports_classified_products_and_reads_them_back() {
        let m = wall_model();
        let cases = [
            (IfcVersion::Ifc4, IfcRepresentation::Tessellation, true, "IFCTRIANGULATEDFACESET("),
            (IfcVersion::Ifc4, IfcRepresentation::Body, false, "IFCFACETEDBREP("),
            (IfcVersion::Ifc2x3, IfcRepresentation::Tessellation, true, "IFCFACETEDBREP("),
        ];
        for (version, representation, standard_hierarchy, geometry) in cases {
            let o = IfcExport { version, representation, standard_hierarchy, ..Default::default() };
            let (text, report) = write(&m, &o, "mur");
            let schema = if version == IfcVersion::Ifc4 { "IFC4" } else { "IFC2X3" };
            assert!(text.contains(&format!("FILE_SCHEMA(('{schema}'))")), "{version:?}");
            assert!(text.contains("IFCWALL(") && text.contains(geometry), "{version:?}\n{text}");
            assert_eq!(text.contains("IFCBUILDINGSTOREY("), standard_hierarchy);
            assert!(text.contains("IFCRELCONTAINEDINSPATIALSTRUCTURE("));
            assert!(report.contains("1 IfcWall"), "{report}");
            let (back, rep) = import(text.as_bytes(), &IfcImport::default()).unwrap();
            assert!(rep.contains("1 IfcWall"), "{rep}");
            // A component classified IfcWall, holding the wall's six faces, with the
            // brick colour.
            let walls = classified(&back);
            assert_eq!(walls.len(), 1, "{version:?}");
            let (d, class) = &walls[0];
            assert_eq!(class, "IfcWall");
            assert_eq!(d.attributes[APPLIED].keys().next().unwrap(), schema_key(version));
            assert_eq!(d.entities.faces.len(), 6, "{version:?}");
            let f = d.entities.faces.values().next().unwrap();
            assert_eq!(back.materials[f.front.unwrap()].color, [180, 80, 60]);
            assert_eq!(back.materials[f.front.unwrap()].name, "Brique");
            let vol = esq_core::solid::volume(&d.entities);
            assert!((vol - 1000.0 * 200.0 * 2500.0).abs() < 1.0, "{version:?} {vol}");
            // The tag named after the class, the wall where it was.
            assert!(back.layers.iter().any(|l| l.name == "IfcWall"));
            let mut minx = f64::MAX;
            esq_core::walk::visit(&back, |pl| {
                for v in pl.ents.verts.values() {
                    minx = minx.min(pl.xform.transform_point3(v.pos).x);
                }
            });
            assert!((minx - 500.0).abs() < 1e-6, "{minx}");
        }
    }

    #[test]
    fn unknown_classes_become_proxies_and_spatial_groups_build_the_hierarchy() {
        let mut m = wall_model();
        let d = m.defs.keys().next().unwrap();
        esq_core::model::set_attr(&mut m.defs[d].attributes, APPLIED, "IFC 4", AttrValue::Text("IfcFurniture".into()));
        // IfcFurniture does not exist in IFC 2x3.
        let o = IfcExport { version: IfcVersion::Ifc2x3, ..Default::default() };
        let (text, report) = write(&m, &o, "x");
        assert!(text.contains("IFCBUILDINGELEMENTPROXY(") && !text.contains("IFCFURNITURE("));
        assert!(report.contains("1 type IFC a été remplacé"), "{report}");
        // A group classified IfcBuildingStorey holding the wall: a storey containing it.
        let mut m = wall_model();
        let wall = m.root.instances.keys().next().unwrap();
        let inst = m.root.instances.remove(wall).unwrap();
        let mut ents = Entities::default();
        ents.instances.insert(inst);
        let storey = m.defs.insert(classified_def("Niveau 1", ents, "IfcBuildingStorey", true));
        m.root.instances.insert(instance(storey, DMat4::IDENTITY, None));
        let (text, _) = write(&m, &IfcExport::default(), "x");
        let f = step::read(&text).unwrap();
        let (sid, _) = f.of_type("IFCBUILDINGSTOREY").next().unwrap();
        let (wid, _) = f.of_type("IFCWALL").next().unwrap();
        let contained = f
            .of_type("IFCRELCONTAINEDINSPATIALSTRUCTURE")
            .any(|(_, r)| r.arg(5).as_ref() == Some(sid) && r.arg(4).refs().contains(&wid));
        assert!(contained, "{text}");
        assert!(f.of_type("IFCRELAGGREGATES").any(|(_, r)| r.arg(5).refs().contains(&sid)));
        // Read back: a group "Niveau 1" holding the wall component.
        let (back, _) = import(text.as_bytes(), &IfcImport::default()).unwrap();
        let g = back.defs.values().find(|d| d.name == "Niveau 1").expect("niveau");
        assert!(g.is_group && g.entities.instances.len() == 1);
    }

    const COLUMNS: &str = "ISO-10303-21;\nHEADER;FILE_SCHEMA(('IFC4'));ENDSEC;\nDATA;
#1=IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.);
#2=IFCCARTESIANPOINT((0.,0.,0.));
#3=IFCAXIS2PLACEMENT3D(#2,$,$);
#4=IFCLOCALPLACEMENT($,#3);
#5=IFCCARTESIANPOINT((2.,0.,0.));
#6=IFCAXIS2PLACEMENT3D(#5,$,$);
#7=IFCLOCALPLACEMENT(#4,#6);
#10=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,0.4,0.2);
#11=IFCDIRECTION((0.,0.,1.));
#12=IFCEXTRUDEDAREASOLID(#10,$,#11,3.);
#13=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#12));
#14=IFCPRODUCTDEFINITIONSHAPE($,$,(#13));
#15=IFCCOLUMN('0a',$,'Poteau',$,$,#7,#14,$,$);
#20=IFCREPRESENTATIONMAP(#3,#13);
#21=IFCCARTESIANTRANSFORMATIONOPERATOR3D($,$,#2,$,$);
#22=IFCMAPPEDITEM(#20,#21);
#23=IFCSHAPEREPRESENTATION($,'Body','MappedRepresentation',(#22));
#24=IFCPRODUCTDEFINITIONSHAPE($,$,(#23));
#25=IFCCOLUMN('0b',$,'Poteau 2',$,$,#4,#24,$,$);
#26=IFCCOLUMNTYPE('0c',$,'Poteau type',$,$,$,(#20),$,$,.COLUMN.);
#30=IFCWALL('0d',$,'Mur',$,$,#4,#34,$,$);
#31=IFCRECTANGLEPROFILEDEF(.AREA.,$,#40,4.,0.2);
#32=IFCEXTRUDEDAREASOLID(#31,$,#11,3.);
#33=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#32));
#34=IFCPRODUCTDEFINITIONSHAPE($,$,(#33));
#40=IFCAXIS2PLACEMENT2D(#41,$);
#41=IFCCARTESIANPOINT((0.,5.));
#50=IFCOPENINGELEMENT('0e',$,'Baie',$,$,#51,#56,$,$);
#51=IFCLOCALPLACEMENT(#4,#52);
#52=IFCAXIS2PLACEMENT3D(#53,$,$);
#53=IFCCARTESIANPOINT((0.,5.,1.));
#54=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,1.,1.);
#55=IFCEXTRUDEDAREASOLID(#54,$,#11,1.);
#56=IFCPRODUCTDEFINITIONSHAPE($,$,(#57));
#57=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#55));
#58=IFCRELVOIDSELEMENT('0f',$,$,$,#30,#50);
ENDSEC;\nEND-ISO-10303-21;\n";

    /// Imports `text` on a thread of its own and says whether it ended within 20 s.
    fn ends(text: String) -> Option<Model> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(import(text.as_bytes(), &IfcImport::default()).ok().map(|x| x.0));
        });
        rx.recv_timeout(std::time::Duration::from_secs(20)).expect("the import ends")
    }

    /// A representation map whose items map it again, twice: every item read the map anew,
    /// 2^32 readings down to the depth limit (the import never ended); a map of itself is
    /// also a component that contains itself.
    #[test]
    fn a_map_that_maps_itself_ends() {
        let t = "ISO-10303-21;\nHEADER;FILE_SCHEMA(('IFC4'));ENDSEC;\nDATA;
#2=IFCCARTESIANPOINT((0.,0.,0.));
#3=IFCAXIS2PLACEMENT3D(#2,$,$);
#4=IFCLOCALPLACEMENT($,#3);
#10=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,0.4,0.2);
#11=IFCDIRECTION((0.,0.,1.));
#12=IFCEXTRUDEDAREASOLID(#10,$,#11,3.);
#13=IFCSHAPEREPRESENTATION($,'Body','MappedRepresentation',(#12,#22,#23));
#20=IFCREPRESENTATIONMAP(#3,#13);
#21=IFCCARTESIANTRANSFORMATIONOPERATOR3D($,$,#2,$,$);
#22=IFCMAPPEDITEM(#20,#21);
#23=IFCMAPPEDITEM(#20,#21);
#24=IFCSHAPEREPRESENTATION($,'Body','MappedRepresentation',(#22));
#25=IFCPRODUCTDEFINITIONSHAPE($,$,(#24));
#26=IFCCOLUMN('0b',$,'Poteau',$,$,#4,#25,$,$);
ENDSEC;\nEND-ISO-10303-21;\n";
        let m = ends(t.to_string()).expect("the column is read");
        assert!(m.defs.values().any(|d| !d.entities.faces.is_empty()));
    }

    /// A revolved solid of 10^12 degrees: as many rings as 24 per turn (billions of
    /// polygons from a few bytes). More than a turn is a turn.
    #[test]
    fn a_huge_revolution_angle_is_one_turn() {
        let t = "ISO-10303-21;\nHEADER;FILE_SCHEMA(('IFC4'));ENDSEC;\nDATA;
#2=IFCCARTESIANPOINT((0.,0.,0.));
#3=IFCAXIS2PLACEMENT3D(#2,$,$);
#4=IFCLOCALPLACEMENT($,#3);
#10=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,0.4,0.2);
#11=IFCDIRECTION((0.,1.,0.));
#14=IFCCARTESIANPOINT((1.,0.,0.));
#15=IFCAXIS1PLACEMENT(#14,#11);
#12=IFCREVOLVEDAREASOLID(#10,#3,#15,1.E12);
#13=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#12));
#16=IFCPRODUCTDEFINITIONSHAPE($,$,(#13));
#17=IFCCOLUMN('0b',$,'Tore',$,$,#4,#16,$,$);
ENDSEC;\nEND-ISO-10303-21;\n";
        let m = ends(t.to_string()).expect("the solid is read");
        let faces: usize = m.defs.values().map(|d| d.entities.faces.len()).sum();
        assert!(faces > 0 && faces < 1000, "{faces}");
    }

    #[test]
    fn reads_extrusions_mapped_items_and_openings() {
        let (m, report) = import(COLUMNS.as_bytes(), &IfcImport::default()).unwrap();
        assert!(report.contains("2 IfcColumn") && report.contains("Ouvertures découpées : 1 sur 1"), "{report}");
        // Two columns of 0.4 × 0.2 × 3 m, one moved 2 m along x; the shared geometry
        // named after its type.
        let mut solids = Vec::new();
        esq_core::walk::visit(&m, |pl| {
            if pl.ents.faces.is_empty() {
                return;
            }
            let lo = pl.ents.verts.values().map(|v| pl.xform.transform_point3(v.pos)).fold(DVec3::MAX, DVec3::min);
            solids.push((esq_core::solid::volume(pl.ents) * pl.xform.determinant(), lo));
        });
        let column = 400.0 * 200.0 * 3000.0;
        assert_eq!(solids.iter().filter(|c| (c.0 - column).abs() < 1.0).count(), 2, "{solids:?}");
        assert!(solids.iter().any(|c| (c.1.x - 1800.0).abs() < 1e-6), "{solids:?}");
        assert!(m.defs.values().any(|d| d.name == "Poteau type"));
        // The wall with its 1 m × 0.2 m × 1 m opening cut through.
        let wall = 4000.0 * 200.0 * 3000.0 - 1000.0 * 200.0 * 1000.0;
        assert!(solids.iter().any(|c| (c.0 - wall).abs() < 1.0), "{solids:?}");
        // Tags after the classes; with "Ne pas créer d'étiquettes" none.
        assert!(m.layers.iter().any(|l| l.name == "IfcColumn"));
        let (m, _) = import(COLUMNS.as_bytes(), &IfcImport { tags: IfcTags::NoTags, ..Default::default() }).unwrap();
        assert_eq!(m.layers.len(), 1);
    }

    #[test]
    fn reads_colours_per_face_and_the_projects_unit() {
        let t = "ISO-10303-21;\nHEADER;FILE_SCHEMA(('IFC4'));ENDSEC;\nDATA;
#1=IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.);
#2=IFCSIUNIT(*,.LENGTHUNIT.,.CENTI.,.METRE.);
#3=IFCUNITASSIGNMENT((#2));
#4=IFCPROJECT('p',$,'P',$,$,$,$,$,#3);
#5=IFCCARTESIANPOINTLIST3D(((0.,0.,0.),(100.,0.,0.),(100.,100.,0.),(0.,100.,0.)));
#6=IFCTRIANGULATEDFACESET(#5,$,$,((1,2,3),(1,3,4)),$);
#7=IFCCOLOURRGBLIST(((1.,0.,0.),(0.,0.,1.)));
#8=IFCINDEXEDCOLOURMAP(#6,$,#7,(1,2));
#9=IFCSHAPEREPRESENTATION($,'Body','Tessellation',(#6));
#10=IFCPRODUCTDEFINITIONSHAPE($,$,(#9));
#11=IFCAXIS2PLACEMENT3D(#12,$,$);
#12=IFCCARTESIANPOINT((0.,0.,0.));
#13=IFCLOCALPLACEMENT($,#11);
#14=IFCSANITARYTERMINAL('s',$,$,$,$,#13,#10,$,$);
ENDSEC;\nEND-ISO-10303-21;\n";
        let (m, report) = import(t.as_bytes(), &IfcImport::default()).unwrap();
        assert!(report.contains("1 IfcSanitaryTerminal"), "{report}");
        let d = m.defs.values().next().unwrap();
        let colours: HashSet<[u8; 3]> =
            d.entities.faces.values().filter_map(|f| f.front).map(|x| m.materials[x].color).collect();
        assert_eq!(colours, HashSet::from([[255, 0, 0], [0, 0, 255]]));
        // Centimetres, as the project says.
        let area: f64 = d.entities.faces.keys().map(|f| esq_core::tess::face_area(&d.entities, f)).sum();
        assert!((area - 1.0e6).abs() < 1e-3, "{area}");
    }

    #[test]
    fn ifczip_holds_the_ifc_file() {
        let d = std::env::temp_dir().join(format!("esq_ifc_{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("mur.ifczip");
        export(&wall_model(), &p, &IfcExport::default()).unwrap();
        let bytes = std::fs::read(&p).unwrap();
        assert!(crate::zipfile::is_zip(&bytes));
        let (m, _) = import(&bytes, &IfcImport::default()).unwrap();
        assert_eq!(classified(&m).len(), 1);
        assert!(dms(-12.5).starts_with("(-12,-30,0,"));
    }
}
