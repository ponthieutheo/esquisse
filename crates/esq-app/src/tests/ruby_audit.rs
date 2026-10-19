//! Audit of hollow Ruby methods, application side: what a script changes
//! is one undo step, is saved with the document, and is used by the tools.

use esq_core::glam::{DMat4, DVec3};
use esq_core::model::{EntityRef, Model};

use super::Harness;
use super::ruby_model::{host, run};
use crate::doc::Document;
use crate::pick::{Handle, SnapKind};
use crate::tools::ToolId;

#[test]
fn script_snaps_are_undone_redone_and_saved() {
    let mut h = host();
    let mut doc = Document::new(Model::new());
    let Some(o) = run(
        &mut h,
        &mut doc,
        "ents = Sketchup.active_model.entities
         s = ents.add_snap([1, 2, 3], [0, 1, 0])
         s.hidden = true
         s.set_attribute('esq', 'nom', 'prise')
         [ents.grep(Sketchup::Snap).size, s.hidden?]",
    ) else {
        return;
    };
    assert_eq!(o.result, "[1, true]");
    let snap = doc.model.root.snaps[0];
    assert!(snap.id != 0 && snap.hidden);
    assert!((snap.pos - DVec3::new(25.4, 50.8, 76.2)).length() < 1e-9);
    assert_eq!(doc.undo_label(), Some("Script Ruby"));
    doc.undo();
    assert!(doc.model.root.snaps.is_empty());
    doc.redo();
    assert_eq!(doc.model.root.snaps, vec![snap]);

    // Saved and read back with its id, its state and its attributes.
    let dir = std::env::temp_dir().join(format!("esq_ruby_audit_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let file = dir.join("ancrage.esq");
    esq_core::io::save_esq(&doc.model, &file).unwrap();
    let back = esq_core::io::load_esq(&file).unwrap();
    assert_eq!(back.root.snaps, vec![snap]);
    let mut doc = Document::new(back);
    let Some(o) = run(
        &mut h,
        &mut doc,
        "s = Sketchup.active_model.entities.grep(Sketchup::Snap).first
         [s.position.to_a, s.direction.to_a, s.hidden?, s.get_attribute('esq', 'nom')]",
    ) else {
        return;
    };
    assert_eq!(o.result, "[[1.0, 2.0, 3.0], [0.0, 1.0, 0.0], true, \"prise\"]");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_host_without_tools_lets_scripts_select_a_tool() {
    // The test host answers only `model.*`: the tools zone falls back (its message test used to
    // compare a UTF-8 message with a binary literal and raise Encoding::CompatibilityError).
    let mut h = host();
    let mut doc = Document::new(Model::new());
    let Some(o) =
        run(&mut h, &mut doc, "Sketchup.active_model.select_tool(nil); Sketchup.active_model.tools.active_tool_name")
    else {
        return;
    };
    assert_eq!(o.result, "\"SelectionTool\"");
}

#[test]
fn the_move_tool_uses_snaps_added_by_a_script() {
    // Two instances of a box component; the snap comes from Ruby, not from the Snap tool.
    let mut hs = Harness::new();
    hs.rect_and_box();
    let items: Vec<EntityRef> = hs.doc.ents().faces.keys().map(EntityRef::Face).collect();
    let mut a = None;
    hs.doc.commit("Composant", |m, d| a = m.make_group(d, &items, false, None));
    let a = a.unwrap();
    hs.doc.commit("Copie", |m, d| {
        let mut i = m.entities(d).instances[a].clone();
        i.xform = DMat4::from_translation(DVec3::new(2500.0, 0.0, 0.0));
        m.entities_mut(d).instances.insert(i);
    });
    let mut h = host();
    let Some(o) = run(
        &mut h,
        &mut hs.doc,
        "d = Sketchup.active_model.entities.grep(Sketchup::ComponentInstance).first.definition
         d.entities.add_snap([1000.mm, 300.mm, 100.mm], [1, 0, 0])
         d.entities.grep(Sketchup::Snap).size",
    ) else {
        return;
    };
    assert_eq!(o.result, "1");
    hs.cam.target = DVec3::new(1800.0, 300.0, 200.0);
    hs.cam.distance = 9000.0;
    hs.doc.select(vec![EntityRef::Instance(a)]);
    hs.tool(ToolId::Move);
    let own = DVec3::new(1000.0, 300.0, 100.0);
    hs.hover(own);
    assert_eq!(hs.tool.snap().unwrap().kind, SnapKind::Handle(Handle::Snap, false));
    hs.click(own);
    // Over the snap of the other instance: the two snaps meet, facing each other.
    let target = DVec3::new(3500.0, 300.0, 100.0);
    hs.hover(target);
    assert_eq!(hs.tool.snap().unwrap().kind, SnapKind::Handle(Handle::Snap, false));
    hs.click(target);
    let x = hs.doc.ents().instances[a].xform;
    assert!(x.transform_point3(own).distance(target) < 1e-6);
    assert!(x.transform_vector3(DVec3::X).abs_diff_eq(-DVec3::X, 1e-9));

    // A hidden snap is not offered.
    let Some(_) = run(
        &mut h,
        &mut hs.doc,
        "Sketchup.active_model.definitions.each { |d| d.entities.grep(Sketchup::Snap).each { |s| s.hidden = true } }",
    ) else {
        return;
    };
    hs.tool(ToolId::Move);
    let own = x.transform_point3(own);
    hs.hover(own);
    assert_ne!(hs.tool.snap().unwrap().kind, SnapKind::Handle(Handle::Snap, false));
}

/// The view zone's host (`ruby_host::view::ViewCx`) on a camera of its own.
struct ViewHost {
    cam: crate::camera::Camera,
    style: crate::style::ViewStyle,
    active: Option<usize>,
    vs: crate::ruby_host::view::ViewState,
}

impl esq_ruby::Host for ViewHost {
    fn call(
        &mut self,
        model: &mut Model,
        _sel: &mut Vec<EntityRef>,
        op: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let mut cx = crate::ruby_host::view::ViewCx {
            cam: &mut self.cam,
            style: &mut self.style,
            active_scene: &mut self.active,
            anim: (2.0, 1.0),
            vs: &mut self.vs,
        };
        cx.call(model, op, args)
    }
}

#[test]
fn locked_inferences_constrain_the_input_points() {
    let Some(i) = super::ruby::interp() else { return };
    let mut h = ViewHost {
        cam: crate::camera::Camera { size: esq_core::glam::DVec2::new(800.0, 600.0), ..Default::default() },
        style: Default::default(),
        active: None,
        vs: Default::default(),
    };
    let mut m = Model::new();
    let _g = super::ruby::HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let code = "m = Sketchup.active_model
         v = m.active_view
         m.entities.add_face([10, 10, 0], [110, 10, 0], [110, 110, 0], [10, 110, 0])
         m.entities.add_face([20, 150, 0], [100, 150, 0], [100, 150, 100], [20, 150, 100])
         v.camera = Sketchup::Camera.new([60, -140, 200], [60, 60, 0], [0, 0, 1])
         free = v.inputpoint(400, 300)
         # Two points: the line through them (here the blue axis).
         v.lock_inference(Sketchup::InputPoint.new(Geom::Point3d.new(0, 0, 0)), Sketchup::InputPoint.new(Geom::Point3d.new(0, 0, 100)))
         ip = v.inputpoint(400, 300)
         line = [v.inference_locked?, ip.position.to_a.map { |c| c.round(6) }, ip.degrees_of_freedom]
         # A point on the upright face: its plane (y = 150).
         v.lock_inference
         s = v.screen_coords([60, 150, 50])
         wall = v.inputpoint(s.x, s.y)
         v.lock_inference(wall)
         ip = v.inputpoint(400, 300)
         plane = [wall.face.normal.to_a.map(&:abs), ip.position.to_a.map { |c| c.round(6) }, ip.degrees_of_freedom]
         v.lock_inference
         [line, plane, v.inference_locked?, v.inputpoint(400, 300).position.to_a == free.position.to_a]";
    let mut sel = Vec::new();
    let o = esq_ruby::with_host(&mut h, || i.run(code, "verrou.rb", &mut m, &mut sel)).unwrap();
    assert!(o.ok, "erreur Ruby: {}", o.error);
    assert_eq!(o.result, "[[true, [0.0, 0.0, 60.0], 1], [[0.0, 1.0, 0.0], [60.0, 150.0, -90.0], 2], false, true]");
}

/// The pick path of an input point ends with the edge it lies on, after the instances that hold
/// it (SketchUp ; TestUp test_instance_path).
#[test]
fn an_input_point_path_ends_with_its_edge() {
    let Some(i) = super::ruby::interp() else { return };
    let mut h = ViewHost {
        cam: crate::camera::Camera { size: esq_core::glam::DVec2::new(800.0, 600.0), ..Default::default() },
        style: Default::default(),
        active: None,
        vs: Default::default(),
    };
    let mut m = Model::new();
    let _g = super::ruby::HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let code = "m = Sketchup.active_model
         v = m.active_view
         d = m.definitions.add('chemin')
         g1 = d.entities.add_group
         g2 = g1.entities.add_group
         g2.transform!([10, 20, 30])
         g3 = g2.entities.add_group
         e = g3.entities.add_line([10, 10, 10], [20, 20, 20])
         inst = m.entities.add_instance(d, Geom::Transformation.new([20, 30, 40]))
         v.zoom_extents
         tr = inst.transformation * g1.transformation * g2.transformation * g3.transformation
         s = v.screen_coords(Geom.linear_combination(0.5, e.start.position, 0.5, e.end.position).transform(tr))
         ip = Sketchup::InputPoint.new
         ip.pick(v, s.x, s.y)
         [ip.instance_path.to_a == [inst, g1, g2, g3, e], ip.edge == e, ip.depth]";
    let mut sel = Vec::new();
    let o = esq_ruby::with_host(&mut h, || i.run(code, "chemin.rb", &mut m, &mut sel)).unwrap();
    assert!(o.ok, "erreur Ruby: {}", o.error);
    assert_eq!(o.result, "[true, true, 5]");
}

#[test]
fn send_action_runs_the_application_commands() {
    let mut a = crate::app::EsquisseApp::bare(None, None);
    let Some(r) = super::dialogs::ruby(
        &mut a,
        "$VERBOSE = nil
         [Sketchup.send_action('selectLineTool:'), Sketchup.send_action('pasUneAction:'), Sketchup.send_action(CMD_VIEW_TOP),
          Sketchup.send_action('renderWireframe:')]",
    ) else {
        return;
    };
    assert_eq!(r, "[true, false, true, true]");
    // As in SketchUp, the commands run after the script, from the application's loop.
    assert_eq!(a.pending_cmds.len(), 3);
    a.run_pending(&eframe::egui::Context::default());
    assert!(a.pending_cmds.is_empty());
    assert_eq!(a.tool.id(), ToolId::Line);
    assert!(a.cam.pitch.to_degrees() > 89.0, "{}", a.cam.pitch.to_degrees());
    assert_eq!(a.style.faces, crate::style::FaceStyle::Wireframe);
}
