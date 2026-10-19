//! Spaces (`SUSpaceRef`, `Sketchup::Space`, SketchUp 2026). The C API reads them like snaps,
//! through a typed entity list, but cannot write them: `SUSpaceCreate` makes a free space and
//! no function adds one to an entities collection (there is no `SUEntitiesAddSpaces`, and
//! `SUGeometryInput` takes only vertices, edges, curves and faces). Holes cannot be read back
//! either (`SUSpaceAddHole` has no getter). A .skp written by Esquisse therefore loses its spaces,
//! as it loses its snaps; the .esq keeps them.
//!
//! The perimeter is not read: `SUSpaceGetPerimeterPoints(space, points, count*)` copies every
//! point into `points` without being told its size (in SketchUpAPI.dll, `*count` is written
//! before anything reads it, and no `SUSpaceGetNum…` function exists), so a forged file with a
//! long perimeter would write past any buffer. It is deliberately not bound.
//!
//! The functions are bound apart from the main table so that an older SketchUp without them
//! still reads and writes .skp files.

use esq_core::model::Space;

use super::*;

#[allow(non_snake_case, dead_code)]
pub(crate) struct SpaceApi {
    /// `(space*, name, perimeter, count, orientation point, height)`, read in SketchUpAPI.dll:
    /// at least 3 points, height ≥ 0, the ceiling the horizontal plane at
    /// that height.
    SUSpaceCreate: unsafe extern "C" fn(*mut Ref, *const c_char, *const P3, usize, *const P3, f64) -> R,
    SUSpaceRelease: unsafe extern "C" fn(*mut Ref) -> R,
    SUSpaceToEntity: unsafe extern "C" fn(Ref) -> Ref,
    SUSpaceFromEntity: unsafe extern "C" fn(Ref) -> Ref,
    SUSpaceToDrawingElement: unsafe extern "C" fn(Ref) -> Ref,
    /// Fills a `SUStringRef` made by the caller (the API sizes the text).
    SUSpaceGetName: unsafe extern "C" fn(Ref, *mut Ref) -> R,
    /// Copies exactly one point (3 doubles), read in the DLL.
    SUSpaceGetOrientationPoint: unsafe extern "C" fn(Ref, *mut P3) -> R,
    /// Copies exactly one plane (4 doubles).
    SUSpaceGetCeilingPlane: unsafe extern "C" fn(Ref, *mut Plane3) -> R,
    _lib: Library,
}

unsafe impl Send for SpaceApi {}
unsafe impl Sync for SpaceApi {}

/// The space functions of the installed SketchUp, if it has them.
pub(crate) fn space_api() -> Option<&'static SpaceApi> {
    static S: OnceLock<Option<SpaceApi>> = OnceLock::new();
    S.get_or_init(|| {
        api().ok()?;
        let lib = load_library(&find_sketchup_api()?).ok()?;
        // SAFETY: the signatures are those of SketchUpAPI.dll (see above).
        unsafe {
            Some(SpaceApi {
                SUSpaceCreate: *lib.get(b"SUSpaceCreate\0").ok()?,
                SUSpaceRelease: *lib.get(b"SUSpaceRelease\0").ok()?,
                SUSpaceToEntity: *lib.get(b"SUSpaceToEntity\0").ok()?,
                SUSpaceFromEntity: *lib.get(b"SUSpaceFromEntity\0").ok()?,
                SUSpaceToDrawingElement: *lib.get(b"SUSpaceToDrawingElement\0").ok()?,
                SUSpaceGetName: *lib.get(b"SUSpaceGetName\0").ok()?,
                SUSpaceGetOrientationPoint: *lib.get(b"SUSpaceGetOrientationPoint\0").ok()?,
                SUSpaceGetCeilingPlane: *lib.get(b"SUSpaceGetCeilingPlane\0").ok()?,
                _lib: lib,
            })
        }
    })
    .as_ref()
}

/// A free space made by the API (released by the caller), in inches.
pub(crate) fn create(s: &SpaceApi, name: &str, perimeter: &[DVec3], orientation: DVec3, height: f64) -> Option<Ref> {
    let pts: Vec<P3> = perimeter.iter().map(|p| P3 { x: p.x, y: p.y, z: p.z }).collect();
    let o = P3 { x: orientation.x, y: orientation.y, z: orientation.z };
    let name = cstr(name);
    let mut sp = Ref::NULL;
    // SAFETY: the points outlive the call; the space is returned to the caller.
    let r = unsafe { (s.SUSpaceCreate)(&mut sp, name.as_ptr(), pts.as_ptr(), pts.len(), &o, height) };
    (r == 0 && !sp.is_null()).then_some(sp)
}

/// `SURefType_Space`, asked of a space the API makes (the enumeration grows in the middle).
fn space_type(a: &Api, s: &SpaceApi) -> Option<i32> {
    static T: OnceLock<Option<i32>> = OnceLock::new();
    *T.get_or_init(|| {
        let mut sp = create(s, "", &[DVec3::ZERO, DVec3::X, DVec3::Y], DVec3::ZERO, 1.0)?;
        // SAFETY: the space was just made; its type is read and the space released.
        unsafe {
            let t = (a.SUEntityGetType)((s.SUSpaceToEntity)(sp));
            (s.SUSpaceRelease)(&mut sp);
            Some(t)
        }
    })
}

/// A space read from the API, in millimetres: name, orientation point, ceiling. The perimeter
/// stays empty (see the module's notes: its getter cannot be called safely).
pub(crate) fn read_space(a: &Api, s: &SpaceApi, sp: Ref) -> Option<Space> {
    let v3 = |p: P3| DVec3::new(p.x, p.y, p.z) * IN;
    let (mut o, mut pl) = (P3::default(), Plane3::default());
    // SAFETY: `sp` is a live space; each getter copies exactly one `P3` (24 bytes) or one
    // `Plane3` (32 bytes) into the local of that type (checked in SketchUpAPI.dll).
    unsafe {
        if (s.SUSpaceGetOrientationPoint)(sp, &mut o) != 0 || (s.SUSpaceGetCeilingPlane)(sp, &mut pl) != 0 {
            return None;
        }
    }
    Some(Space {
        // SAFETY: the name goes into a string made and released by `read_string`.
        name: read_string(a, |x| unsafe { (s.SUSpaceGetName)(sp, x) }),
        orientation: v3(o),
        ceiling: [pl.a, pl.b, pl.c, pl.d * IN],
        ..Default::default()
    })
}

/// The spaces of an entities collection, each with its drawing element (tag, hidden).
pub(crate) fn read_spaces(a: &Api, ents: Ref) -> Vec<(Space, Ref)> {
    let Some(s) = space_api() else { return Vec::new() };
    let Some(t) = space_type(a, s) else { return Vec::new() };
    typed_entities(a, ents, t)
        .into_iter()
        .filter_map(|e| {
            // SAFETY: `e` is a space of the collection.
            let sp = unsafe { (s.SUSpaceFromEntity)(e) };
            if sp.is_null() {
                return None;
            }
            Some((read_space(a, s, sp)?, unsafe { (s.SUSpaceToDrawingElement)(sp) }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the API keeps of a space is what Esquisse reads back (a .skp of SketchUp 2026 with
    /// spaces is read the same way, through the collection's typed list).
    #[test]
    fn a_space_made_by_the_api_reads_back() {
        let Ok(a) = api() else { return };
        let Some(s) = space_api() else { return };
        let _l = CALLS.lock().unwrap_or_else(|e| e.into_inner());
        assert!(space_type(a, s).is_some_and(|t| t > 50));
        let room = [DVec3::ZERO, DVec3::new(100.0, 0.0, 0.0), DVec3::new(100.0, 50.0, 0.0), DVec3::new(0.0, 50.0, 0.0)];
        let mut sp = create(s, "Salon", &room, DVec3::new(10.0, 10.0, 0.0), 96.0).unwrap();
        let got = read_space(a, s, sp).unwrap();
        // SAFETY: made above.
        unsafe { (s.SUSpaceRelease)(&mut sp) };
        assert_eq!(got.name, "Salon");
        // The perimeter is not read (its getter has no capacity argument).
        assert!(got.perimeter.is_empty());
        assert!((got.orientation - DVec3::new(254.0, 254.0, 0.0)).length() < 1e-6);
        let [x, y, z, d] = got.ceiling;
        assert!((x, y, (z - 1.0).abs()) == (0.0, 0.0, 0.0) && (d + 96.0 * IN).abs() < 1e-6, "{:?}", got.ceiling);
        // No height below the floor.
        assert!(create(s, "x", &room, DVec3::ZERO, -1.0).is_none());
    }

    /// A space whose perimeter is longer than any buffer one would size (what a forged file
    /// could hold) is read without its points: nothing writes past a buffer.
    #[test]
    fn a_space_with_a_huge_perimeter_is_read_safely() {
        let Ok(a) = api() else { return };
        let Some(s) = space_api() else { return };
        let _l = CALLS.lock().unwrap_or_else(|e| e.into_inner());
        let n = 70_000;
        let ring: Vec<DVec3> = (0..n)
            .map(|i| {
                let t = std::f64::consts::TAU * i as f64 / n as f64;
                DVec3::new(1000.0 * t.cos(), 1000.0 * t.sin(), 0.0)
            })
            .collect();
        let Some(mut sp) = create(s, "Grand", &ring, DVec3::ZERO, 10.0) else { return };
        let got = read_space(a, s, sp);
        // SAFETY: made above.
        unsafe { (s.SUSpaceRelease)(&mut sp) };
        let got = got.unwrap();
        assert_eq!(got.name, "Grand");
        assert!(got.perimeter.is_empty());
    }
}
