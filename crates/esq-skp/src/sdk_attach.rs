//! What SketchUp's SDK (SketchUpAPI.dll) reads of the points texts and dimensions are attached
//! to, and how it moves them when what they are attached to changes: the oracle of
//! `tests/attaches.rs`. Tests only; Esquisse reads and writes these
//! attaches natively.

use super::*;

/// An end of a text or a linear dimension of the model's root, as the SDK gives it.
#[derive(Clone, Debug, PartialEq)]
pub struct SdkEnd {
    /// "texte", "départ" or "arrivée".
    pub which: &'static str,
    /// Where the SDK places it (mm).
    pub point: [f64; 3],
    /// What it is attached to: the type of the path's leaf (`SURefType`) and the number of
    /// instances of the path; `None` when the SDK gives no path.
    pub leaf: Option<(i32, usize)>,
}

/// What is done to the model before its ends are read again. A transformation is a 4×4
/// matrix by columns, its translation in millimetres.
#[derive(Clone, Copy, Debug)]
pub enum SdkOp {
    Nothing,
    /// Every instance of the root transformed (placed by this, after its own placement).
    TransformInstances([f64; 16]),
    /// Every vertex of every definition transformed: the definitions edited.
    TransformDefinitionVertices([f64; 16]),
    /// The instances of the root erased.
    EraseInstances,
    /// The edges of every definition erased (their vertices go with them).
    EraseDefinitionEdges,
}

/// `m` (translation in mm) as a transformation of the SDK (inches).
fn inches(m: &[f64; 16]) -> [f64; 16] {
    let mut x = *m;
    for v in &mut x[12..15] {
        *v /= IN;
    }
    x
}

/// `a · b`, 4×4 by columns.
fn mul(a: &[f64; 16], b: &[f64; 16]) -> [f64; 16] {
    std::array::from_fn(|i| (0..4).map(|k| a[k * 4 + i % 4] * b[(i / 4) * 4 + k]).sum())
}

/// `SURefType` numbers of what an annotation can be attached to, as SketchUpAPI.dll 26.2 gives
/// them (measured: two types come before `Face` that the headers of older SDKs did not have).
pub mod ref_type {
    pub const EDGE: i32 = 6;
    pub const FACE: i32 = 11;
    pub const VERTEX: i32 = 26;
    pub const GUIDE_POINT: i32 = 28;
    pub const GUIDE_LINE: i32 = 29;
}

fn mm(p: P3) -> [f64; 3] {
    [p.x * IN, p.y * IN, p.z * IN]
}

/// The leaf type and depth of an instance path (released here).
fn path_of(a: &Api, mut path: Ref) -> Option<(i32, usize)> {
    if path.is_null() {
        return None;
    }
    let (mut leaf, mut depth) = (Ref::NULL, 0usize);
    let ok = unsafe { (a.SUInstancePathGetLeafAsEntity)(path, &mut leaf) } == 0;
    unsafe { (a.SUInstancePathGetPathDepth)(path, &mut depth) };
    unsafe { (a.SUInstancePathRelease)(&mut path) };
    (ok && !leaf.is_null()).then(|| (unsafe { (a.SUEntityGetType)(leaf) }, depth))
}

/// The ends of the root's texts and linear dimensions of the file at `path`, once `op` is done.
/// `None` when the SDK does not open it.
pub(crate) fn attach_ends(a: &Api, path: &Path, op: SdkOp) -> Option<Vec<SdkEnd>> {
    let cp = cstr(&path.to_string_lossy());
    let (mut m, mut status) = (Ref::NULL, 0);
    if unsafe { (a.SUModelCreateFromFileWithStatus)(&mut m, cp.as_ptr(), &mut status) } != 0 {
        return None;
    }
    let mut root = Ref::NULL;
    unsafe { (a.SUModelGetEntities)(m, &mut root) };
    let insts = list(
        |n| unsafe { (a.SUEntitiesGetNumInstances)(root, n) },
        |n, p, g| unsafe { (a.SUEntitiesGetInstances)(root, n, p, g) },
    );
    let defs = list(
        |n| unsafe { (a.SUModelGetNumComponentDefinitions)(m, n) },
        |n, p, g| unsafe { (a.SUModelGetComponentDefinitions)(m, n, p, g) },
    );
    let def_edges = |d: Ref| {
        let mut e = Ref::NULL;
        unsafe { (a.SUComponentDefinitionGetEntities)(d, &mut e) };
        let edges = list(
            |n| unsafe { (a.SUEntitiesGetNumEdges)(e, false, n) },
            |n, p, g| unsafe { (a.SUEntitiesGetEdges)(e, false, n, p, g) },
        );
        (e, edges)
    };
    // The points as SketchUp shows them before the change (the SDK works them out only when
    // asked: a connection cut loose keeps the last one).
    ends(a, root);
    match op {
        SdkOp::Nothing => {}
        SdkOp::TransformInstances(t) => {
            for i in &insts {
                let mut x = Xf { v: [0.0; 16] };
                unsafe { (a.SUComponentInstanceGetTransform)(*i, &mut x) };
                let y = Xf { v: mul(&inches(&t), &x.v) };
                unsafe { (a.SUComponentInstanceSetTransform)(*i, &y) };
            }
        }
        SdkOp::TransformDefinitionVertices(t) => {
            let t = inches(&t);
            for d in &defs {
                let mut done = std::collections::HashSet::new();
                for e in def_edges(*d).1 {
                    for get in [a.SUEdgeGetStartVertex, a.SUEdgeGetEndVertex] {
                        let mut v = Ref::NULL;
                        if unsafe { get(e, &mut v) } != 0 || !done.insert(v.0 as usize) {
                            continue;
                        }
                        let mut p = P3::default();
                        unsafe { (a.SUVertexGetPosition)(v, &mut p) };
                        let f = |r: usize| t[r] * p.x + t[4 + r] * p.y + t[8 + r] * p.z + t[12 + r];
                        let q = P3 { x: f(0), y: f(1), z: f(2) };
                        unsafe { (a.SUVertexSetPosition)(v, &q) };
                    }
                }
            }
        }
        SdkOp::EraseInstances => {
            let mut v: Vec<Ref> = insts.iter().map(|i| unsafe { (a.SUComponentInstanceToEntity)(*i) }).collect();
            unsafe { (a.SUEntitiesErase)(root, v.len(), v.as_mut_ptr()) };
        }
        SdkOp::EraseDefinitionEdges => {
            for d in &defs {
                let (e, edges) = def_edges(*d);
                let mut v: Vec<Ref> = edges.iter().map(|x| unsafe { (a.SUEdgeToEntity)(*x) }).collect();
                unsafe { (a.SUEntitiesErase)(e, v.len(), v.as_mut_ptr()) };
            }
        }
    }
    let out = ends(a, root);
    unsafe { (a.SUModelRelease)(&mut m) };
    Some(out)
}

/// The ends of the texts and linear dimensions of a collection (asking for them also has
/// SketchUp work their points out, which it keeps for a connection cut loose).
fn ends(a: &Api, root: Ref) -> Vec<SdkEnd> {
    let mut out = Vec::new();
    for tx in list(
        |n| unsafe { (a.SUEntitiesGetNumTexts)(root, n) },
        |n, p, g| unsafe { (a.SUEntitiesGetTexts)(root, n, p, g) },
    ) {
        let (mut pt, mut path) = (P3::default(), Ref::NULL);
        if unsafe { (a.SUTextGetPoint)(tx, &mut pt, &mut path) } == 0 {
            out.push(SdkEnd { which: "texte", point: mm(pt), leaf: path_of(a, path) });
        }
    }
    for dm in list(
        |n| unsafe { (a.SUEntitiesGetNumDimensions)(root, n) },
        |n, p, g| unsafe { (a.SUEntitiesGetDimensions)(root, n, p, g) },
    ) {
        let mut ty = 0;
        unsafe { (a.SUDimensionGetType)(dm, &mut ty) };
        if ty != 1 {
            continue;
        }
        let d = unsafe { (a.SUDimensionLinearFromDimension)(dm) };
        for (which, get) in [("départ", a.SUDimensionLinearGetStartPoint), ("arrivée", a.SUDimensionLinearGetEndPoint)]
        {
            let (mut pt, mut path) = (P3::default(), Ref::NULL);
            if unsafe { get(d, &mut pt, &mut path) } == 0 {
                out.push(SdkEnd { which, point: mm(pt), leaf: path_of(a, path) });
            }
        }
    }
    out
}
