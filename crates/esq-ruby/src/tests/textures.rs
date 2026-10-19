//! Placage des textures par l'API Ruby, comme SketchUp : `Face#position_material` (projeté,
//! déformé, refusé), `uv_tile_at`, `get_texture_projection`, `clear_texture_*`,
//! `Sketchup::UVHelper`, `Face#mesh` et ses drapeaux, `persistent_id` lu d'un fichier. Les valeurs
//! viennent de la suite TestUp de SketchUp (`TC_Sketchup_Face`) et des méthodes Ruby de
//! SketchUp.exe.

use super::*;

const HELPERS: &str = "def err; yield; nil; rescue Exception => e; \"#{e.class}: #{e.message}\"; end
def r6(p); p.to_a.map { |v| v.round(6) }; end
";

fn ev(code: &str) -> Option<String> {
    must(&format!("{HELPERS}{code}")).map(|(o, _)| o.result)
}

fn png(dir: &std::path::Path) -> String {
    let img = image::RgbaImage::from_fn(8, 8, |x, y| image::Rgba([(x * 30) as u8, (y * 30) as u8, 90, 255]));
    let path = dir.join("uv.png");
    img.save(&path).unwrap();
    path.to_string_lossy().replace('\\', "/")
}

/// A folder of the test's own (not `esq_tex_…`: the temporary texture files the test
/// `texture_names_never_escape_the_temp_folder` checks for, at the same time).
fn scratch(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("esq_placage_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A 100" square on the ground facing up (TestUp's `create_face`) and a textured material of
/// 10" tiles.
fn setup(path: &str) -> String {
    format!(
        "model = Sketchup.active_model
         face = model.active_entities.add_face([0, 0, 0], [100, 0, 0], [100, 100, 0], [0, 100, 0])
         face.reverse!
         mat = model.materials.add('Test Material')
         mat.texture = ['{path}', 10, 10]
         "
    )
}

/// Projected along (0, −1, 1): positioned and projected, the direction kept normalized, the
/// tile corners on the plane through the origin facing the projection, the coordinates the same
/// along the projection (`TC_Sketchup_Face#test_position_material_projected_*`, `uv_tile_at`).
#[test]
fn a_projected_position() {
    let dir = scratch("proj");
    let p = png(&dir);
    let Some(r) = ev(&format!(
        "{}
         dir = Geom::Vector3d.new(0, -1, 1)
         res = face.position_material(mat, [Geom::Point3d.new(3, 0, 0), Geom::Point3d.new(0, 0, 0),
                                            Geom::Point3d.new(30, 0, 0), Geom::Point3d.new(1, 0, 0)], true, dir)
         uvh = face.get_UVHelper(true, true)
         a = uvh.get_front_UVQ([20, 30, 0]); b = uvh.get_front_UVQ([20, 23, 7])
         tile = face.uv_tile_at(Geom::Point3d.new(20, 30, 0), true)
         on_plane = tile.each_slice(2).all? {{ |w, uv| (w.y - w.z).abs < 1e-9 && uv.z == 0 }}
         same = tile.each_slice(2).all? {{ |w, uv| r6(uvh.get_front_UVQ(w)).first(2) == r6(uv).first(2) }}
         [res == face, face.texture_positioned?(true), face.texture_projected?(true), face.texture_positioned?(false),
          r6(face.get_texture_projection(true)), r6(a) == r6(b), tile.size, on_plane, same, r6(tile[0]), r6(tile[1])]",
        setup(&p)
    )) else {
        return;
    };
    assert_eq!(
        r,
        "[true, true, true, false, [0.0, -0.707107, 0.707107], true, 8, true, true, [3.0, 0.0, 0.0], [0.0, 0.0, 0.0]]"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Clearing the projection keeps the texture where it is on the face, positioned; clearing the
/// position forgets both (`test_clear_texture_projection`, `test_clear_texture_position`).
#[test]
fn clearing_the_projection_keeps_the_position() {
    let dir = scratch("clear");
    let p = png(&dir);
    let Some(r) = ev(&format!(
        "{}
         dir = Geom::Vector3d.new(1, 2, 3)
         face.position_material(mat, [[10, 20, 0], [0, 0, 0], [40, 20, 0], [2, 1, 0]], true, dir)
         face.position_material(mat, [[10, 20, 0], [0, 0, 0]], false, dir)
         before = r6(face.get_UVHelper.get_front_UVQ([50, 60, 0]))
         face.clear_texture_projection(true)
         after = r6(face.get_UVHelper.get_front_UVQ([50, 60, 0]))
         r = [before == after, face.texture_projected?(true), face.texture_positioned?(true), face.get_texture_projection(true),
              face.texture_projected?(false)]
         face.clear_texture_position(false)
         r + [face.texture_positioned?(false), face.texture_projected?(false), face.get_texture_projection(false)]",
        setup(&p)
    )) else {
        return;
    };
    assert_eq!(r, "[true, false, true, nil, true, false, false, nil]");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Four pairs: a distorted texture, each model point at its texture point; degenerate points and
/// wrong arguments refused as SketchUp refuses them.
#[test]
fn distorted_and_refused_positions() {
    let dir = scratch("distort");
    let p = png(&dir);
    let Some(r) = ev(&format!(
        "{}
         pairs = [[0, 0, 0], [0, 0, 0], [30, 0, 0], [1, 0, 0], [40, 50, 0], [1, 1, 0], [-20, 50, 0], [0, 1, 0]]
         face.position_material(mat, pairs, true)
         uvh = face.get_UVHelper
         hit = pairs.each_slice(2).all? {{ |w, uv| q = uvh.get_front_UVQ(w); ((q.x / q.z) - uv[0]).abs < 1e-9 && ((q.y / q.z) - uv[1]).abs < 1e-9 }}
         line = [[100, 0, 0], [0, 0, 0], [200, 0, 0], [1, 0, 0], [250, 0, 0], [1, 1, 0], [350, 0, 0], [0, 1, 0]]
         img = model.active_entities.add_image('{p}', ORIGIN, 10)
         image_mat = model.definitions.find(&:image?).entities.grep(Sketchup::Face).first.material
         other = model.active_entities.add_face([0, 200, 0], [10, 200, 0], [10, 210, 0])
         [hit, err {{ face.position_material(mat, line, true) }}, err {{ face.position_material('st', 'ri', 'ng') }},
          err {{ face.position_material(false, false, false) }}.split(':').first, err {{ other.position_material(image_mat, [[0, 0, 0], [0, 0, 0]], true) }},
          other.material, err {{ other.back_material = image_mat }}, face.position_material('red', [[0, 0, 0], [0, 0, 0]], true),
          err {{ face.position_material(mat, [[0, 0, 0], [0, 0, 0]], true, nil, nil) }}]",
        setup(&p)
    )) else {
        return;
    };
    assert_eq!(
        r,
        "[true, \"ArgumentError: Could not compute valid matrix from points\", \"ArgumentError: Cannot find material named st\", \
         \"TypeError\", \
         \"ArgumentError: Cannot assign image material to model entities\", nil, \
         \"ArgumentError: Cannot assign image material to model entities\", false, \
         \"ArgumentError: wrong number of arguments (given 5, expected 3..4)\"]"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// `Face#mesh`: UVQ of the sides asked for (1 front, 2 back), normals (4),
/// zero otherwise; a face on the ground faces down, so without material u = −x, v = y in
/// inches on both sides (`TC_Sketchup_Face#test_mesh_*`).
#[test]
fn mesh_flags() {
    let Some(r) =
        ev("f = Sketchup.active_model.active_entities.add_face([-10, -10, 0], [10, -10, 0], [10, 10, 0], [-10, 10, 0])
         i = f.mesh(0).points.index { |p| p == Geom::Point3d.new(10, -10, 0) } + 1
         m0 = f.mesh(0); m1 = f.mesh(1); m7 = f.mesh(7)
         [r6(m0.uv_at(i, true)), r6(m0.normal_at(i)), r6(m1.uv_at(i, true)), r6(m1.uv_at(i, false)),
          r6(m7.uv_at(i, false)), r6(m7.normal_at(i)), err { f.mesh('x') }, err { f.mesh(-5) }, err { f.mesh(nil) }]")
    else {
        return;
    };
    assert_eq!(
        r,
        "[[0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [-10.0, -10.0, 1.0], [0.0, 0.0, 0.0], [-10.0, -10.0, 1.0], [0.0, 0.0, -1.0], \
         \"TypeError: no implicit conversion of String into Integer\", \"RangeError: value cannot be negative\", \
         \"TypeError: no implicit conversion of NilClass into Integer\"]"
    );
}

/// `get_UVHelper` takes the sides by truth and a third argument only if it is a texture writer;
/// `get_front_UVQ` gives nil for what is not a point.
#[test]
fn uv_helper_arguments() {
    let Some(r) = ev("f = Sketchup.active_model.active_entities.add_face([0, 0, 0], [10, 0, 0], [10, 10, 0])
         tw = Sketchup.create_texture_writer
         [f.get_UVHelper(true, true, tw).class, f.get_UVHelper(nil, nil).class, err { f.get_UVHelper(nil, nil, nil) },
          err { f.get_UVHelper('st', 'ri', 'ng') }, f.get_UVHelper.get_front_UVQ('abc'),
          r6(f.get_UVHelper(false, true).get_back_UVQ([2, 3, 0]))]")
    else {
        return;
    };
    assert_eq!(
        r,
        "[Sketchup::UVHelper, Sketchup::UVHelper, \"TypeError: wrong argument type (expected Sketchup::TextureWriter)\", \
         \"TypeError: wrong argument type (expected Sketchup::TextureWriter)\", nil, [-2.0, 3.0, 1.0]]"
    );
}

/// `Face#area` takes a transformation or an array of 16 numbers.
#[test]
fn area_arguments() {
    let Some(r) = ev("f = Sketchup.active_model.active_entities.add_face([0, 0, 0], [100, 0, 0], [100, 100, 0], [0, 100, 0])
         [f.area([0.5, 0, 0, 0, 0, 0.5, 0, 0, 0, 0, 0.5, 0, 0, 0, 0, 1]).round(6), err { f.area('x') }.split(':').first,
          err { f.area(['not']) }.split(':').first, err { f.area(nil) }.split(':').first, err { f.area([1], 2) }.split(':').first]")
    else {
        return;
    };
    assert_eq!(r, "[2500.0, \"TypeError\", \"ArgumentError\", \"TypeError\", \"ArgumentError\"]");
}

/// A face read from a SketchUp 2021+ file answers to the persistent id it had there
/// (`Model#find_entity_by_persistent_id`, TestUp's `uv_tile_at` tests).
#[test]
fn persistent_ids_read_from_a_file() {
    let Some(i) = interp() else { return };
    let mut m = Model::new();
    let vs: Vec<_> = [(0.0, 0.0), (1000.0, 0.0), (1000.0, 500.0)]
        .iter()
        .map(|&(x, y)| m.root.vertex_at(esq_core::glam::DVec3::new(x, y, 0.0)))
        .collect();
    let f = m.root.add_face_raw(vs, vec![]).unwrap();
    m.root.pids.insert(format!("f{}", slotmap::Key::data(&f).as_ffi()), 27788);
    let code = "m = Sketchup.active_model
                f = m.find_entity_by_persistent_id(27788)
                [f.class, f.persistent_id, m.active_entities.grep(Sketchup::Edge).first.persistent_id != 27788]";
    let out = i.run(code, "pid.rb", &mut m, &mut Vec::new()).unwrap();
    assert!(out.ok, "{}", out.error);
    assert_eq!(out.result, "[Sketchup::Face, 27788, true]");
}
