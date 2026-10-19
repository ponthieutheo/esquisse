use super::*;

/// A 4 x 2 PNG: red top-left pixel, the rest blue. Returns its path with forward slashes.
/// Written once per process: tests run in parallel and must never read a half-written file.
fn png() -> String {
    static PATH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    PATH.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("esq_entities_image_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("photo.png");
        let mut img = ::image::RgbImage::from_pixel(4, 2, ::image::Rgb([0, 0, 255]));
        img.put_pixel(0, 0, ::image::Rgb([255, 0, 0]));
        img.save(&path).unwrap();
        path.to_string_lossy().replace('\\', "/")
    })
    .clone()
}

fn with_image(code: &str) -> String {
    format!("m = Sketchup.active_model; ents = m.entities\n i = ents.add_image('{}', [1, 2, 3], 8)\n{code}", png())
}

#[test]
fn add_image_makes_a_textured_image_instance() {
    let p = png();
    let Some((o, m)) = must(&with_image(&format!(
        "[i.class.name, i.typename, i.width, i.height, i.pixelwidth, i.pixelheight, i.origin.to_a, i.normal.to_a,
          i.zrotation, i.path == '{p}', i.transformation.origin.to_a, i.is_a?(Sketchup::Drawingelement),
          i.is_a?(Sketchup::ComponentInstance), ents.grep(Sketchup::Image).size, ents.to_a.last == i, i.valid?,
          i.bounds.max.to_a]"
    ))) else {
        return;
    };
    assert_eq!(
        o.result,
        "[\"Sketchup::Image\", \"Image\", 8.0, 4.0, 4, 2, [1.0, 2.0, 3.0], [0.0, 0.0, 1.0], 0.0, true, [1.0, 2.0, 3.0], true, false, 1, true, true, [9.0, 6.0, 3.0]]"
    );
    // A real picture: the definition's face is painted with the image's own textured material.
    let inst = m.root.instances.values().next().unwrap();
    assert!(m.is_image_def(inst.def));
    let face = m.defs[inst.def].entities.faces.values().next().unwrap();
    let tex = m.materials[face.front.unwrap()].ext.texture.as_ref().unwrap();
    assert_eq!(tex.px, [4, 2]);
    assert!(!tex.image.0.is_empty());
}

#[test]
fn add_image_sizes_and_errors() {
    let p = png();
    let Some(r) = ev(&format!(
        "ents = Sketchup.active_model.entities
         a = ents.add_image('{p}', [0,0,0], 0, 6)
         b = ents.add_image('{p}', [0,0,0], 2, 9)
         [a.width, a.height, b.width, b.height, ents.add_image('', [0,0,0], 5)]"
    )) else {
        return;
    };
    assert_eq!(r, "[12.0, 6.0, 2.0, 9.0, nil]");
    let Some((o, _)) = run("Sketchup.active_model.entities.add_image('/pas/la.png', [0,0,0], 5)") else { return };
    assert_eq!(o.result, "nil");
    assert!(o.out.contains("Could not create Image for /pas/la.png"), "{}", o.out);
    assert_eq!(
        raised(&format!("Sketchup.active_model.entities.add_image('{p}', [0,0,0], 0)")).unwrap(),
        "ArgumentError: Image size cannot be 0\n"
    );
    assert_eq!(
        raised("Sketchup.active_model.entities.add_image('x', [0,0,0])").unwrap(),
        "ArgumentError: Too few arguments - at least 3 required"
    );
}

#[test]
fn image_resizing() {
    let Some(r) = ev(&with_image(
        "out = [i.send(:width=, 10), i.width, i.height]
         out += [i.send(:height=, 2), i.width, i.height]
         out += [i.send(:size=, 6), i.width, i.height]
         i.size = [5, 7]
         out += [i.width, i.height]
         i.size = [0, 3]
         out += [i.width, i.height, i.origin.to_a]",
    )) else {
        return;
    };
    assert_eq!(r, "[10, 10.0, 4.0, 2, 10.0, 2.0, nil, 6.0, 3.0, 5.0, 7.0, 6.0, 3.0, [1.0, 2.0, 3.0]]");
    assert_eq!(raised(&with_image("i.size = []")).unwrap(), "ArgumentError: Too few arguments - at least 1 required");
    assert_eq!(raised(&with_image("i.size = 0")).unwrap(), "ArgumentError: Image size cannot be 0\n");
    assert_eq!(raised(&with_image("i.width = 'x'")).unwrap(), "TypeError: no implicit conversion to float from string");
}

#[test]
fn image_placement() {
    let Some(r) = ev(&with_image(
        "out = [i.send(:origin=, [5, 5, 0]), i.origin.to_a]
         rot = Geom::Transformation.rotation([0, 0, 0], [0, 0, 1], Math::PI / 2)
         out += [i.send(:transformation=, rot).equal?(i), i.origin.to_a, i.zrotation.round(6), i.width.round(6)]
         out += [i.transform!(Geom::Transformation.translation([0, 0, 4])).equal?(i), i.origin.to_a.map { |v| v.round(6) },
                 i.normal.to_a.map { |v| v.round(6) }]",
    )) else {
        return;
    };
    assert_eq!(
        r,
        "[[5, 5, 0], [5.0, 5.0, 0.0], true, [0.0, 0.0, 0.0], 1.570796, 4.0, true, [0.0, 0.0, 4.0], [0.0, 0.0, 1.0]]"
    );
    let zero = "Geom::Transformation.new([0,0,0,0, 0,0,0,0, 0,0,0,0, 0,0,0,1])";
    assert_eq!(
        raised(&with_image(&format!("i.transformation = {zero}"))).unwrap(),
        "ArgumentError: Unable to set transformation: transformation matrix is not invertible"
    );
    assert_eq!(
        raised(&with_image(&format!("i.transform!({zero})"))).unwrap(),
        "ArgumentError: Unable to apply transformation: transformation matrix is not invertible"
    );
}

#[test]
fn image_rep_gives_the_pixels() {
    let Some(r) = ev(&with_image(
        "rep = i.image_rep
         c = rep.colors
         [rep.class.name, rep.width, rep.height, c.size, c.map { |x| [x.red, x.green, x.blue] }.uniq.sort]",
    )) else {
        return;
    };
    assert_eq!(r, "[\"Sketchup::ImageRep\", 4, 2, 8, [[0, 0, 255], [255, 0, 0]]]");
}

#[test]
fn exploding_an_image_leaves_a_textured_face() {
    let Some(r) = ev(&with_image(
        "parts = i.explode
         f = parts.grep(Sketchup::Face).first
         [parts.grep(Sketchup::Face).size, parts.grep(Sketchup::Edge).size, i.valid?, f.material.texture.nil?,
          f.vertices.map { |v| v.position.to_a }.max, ents.grep(Sketchup::Image).size]",
    )) else {
        return;
    };
    assert_eq!(r, "[1, 4, false, false, [9.0, 6.0, 3.0], 0]");
    for call in ["i.width", "i.path", "i.size = 3", "i.image_rep", "i.explode"] {
        assert_eq!(
            raised(&with_image(&format!("i.erase!\n {call}"))).unwrap(),
            "TypeError: reference to deleted Image",
            "{call}"
        );
    }
}

#[test]
fn image_gluing() {
    let Some(r) = ev(&with_image(
        "f = ents.add_face([0,0,0], [20,0,0], [20,20,0], [0,20,0])
         a = [i.glued_to, i.send(:glued_to=, nil)]
         a",
    )) else {
        return;
    };
    assert_eq!(r, "[nil, nil]");
    assert_eq!(
        raised(&with_image("i.glued_to = 5")).unwrap(),
        "TypeError: wrong argument type (expected Sketchup::Entity)"
    );
    assert_eq!(
        raised(&with_image("i.glued_to = ents.add_line([0,0,0], [1,0,0])")).unwrap(),
        "ArgumentError: wrong type - expected Sketchup::Face, Sketchup::Group, SketchUp::Image or Sketchup::ComponentInstance"
    );
}

#[test]
fn image_definitions_answer_image() {
    let Some((o, _)) = must(&with_image("[m.definitions.any?(&:image?), m.definitions.add('Simple').image?]")) else {
        return;
    };
    assert_eq!(o.result, "[true, false]");
}
