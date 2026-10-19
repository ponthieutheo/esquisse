//! Zone « model » côté application : fichier du document, chemin d'édition, barre d'état, et
//! une étape d'annulation par opération Ruby (`start_operation` … `commit_operation`).

use std::path::PathBuf;

use esq_core::model::{EntityRef, Model};
use esq_ruby::RunOutput;
use serde_json::Value as J;

use super::ruby::{HOST_LOCK, interp};
use crate::doc::Document;
use crate::ruby_host::model::{ModelApp, Replace, ScriptState, call};

#[derive(Default)]
pub(super) struct Stub {
    /// Images asked for (rendered by the GPU in the application).
    pub images: Vec<(PathBuf, crate::export2d::Kind, crate::export2d::RasterOptions)>,
    pub said: Vec<String>,
    /// The saved options of File › Import / Export.
    pub formats: esq_formats::AllOptions,
    pub results: Vec<(String, String)>,
    /// The view of the 2D graphics, and the saved options of File › Export › 2D Graphic.
    pub view2d: Option<(crate::camera::Camera, crate::style::ViewStyle)>,
    pub prefs2d: serde_json::Map<String, J>,
}

impl ModelApp for Stub {
    fn view(&self) -> Option<esq_skp::View> {
        None
    }
    fn view2d(&self) -> Option<(crate::camera::Camera, crate::style::ViewStyle)> {
        self.view2d.clone()
    }
    fn prefs2d(&mut self) -> J {
        J::Object(self.prefs2d.clone())
    }
    fn set_pref2d(&mut self, key: &str, value: J) {
        self.prefs2d.insert(key.to_string(), value);
    }
    fn image(
        &mut self,
        _model: &mut Model,
        _ctx: &[esq_core::model::InstanceId],
        _sel: &[EntityRef],
        path: &std::path::Path,
        kind: crate::export2d::Kind,
        o: &crate::export2d::RasterOptions,
    ) -> Result<(), String> {
        self.images.push((path.to_path_buf(), kind, o.clone()));
        Ok(())
    }
    fn say(&mut self, text: &str) {
        self.said.push(text.to_string());
    }
    fn formats(&mut self) -> esq_formats::AllOptions {
        self.formats.clone()
    }
    fn set_formats(&mut self, o: esq_formats::AllOptions) {
        self.formats = o;
    }
    fn results(&mut self, title: &str, text: String) {
        self.results.push((title.to_string(), text));
    }
}

pub(super) struct TestHost {
    pub app: Stub,
    pub st: ScriptState,
}

impl esq_ruby::Host for TestHost {
    fn call(&mut self, model: &mut Model, sel: &mut Vec<EntityRef>, op: &str, args: &J) -> Result<J, String> {
        match op.strip_prefix("model.") {
            Some(_) => call(&mut self.app, &mut self.st, model, sel, op, args),
            None => Err(format!("opération hôte non implémentée: {op}")),
        }
    }
}

pub(super) fn run(h: &mut TestHost, doc: &mut Document, code: &str) -> Option<RunOutput> {
    let i = interp()?;
    let _g = HOST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let o = esq_ruby::with_host(h, || {
        doc.run_script_steps("Script Ruby", |m, s| (i.run(code, "modele.rb", m, s), esq_ruby::take_ops()))
    })
    .unwrap();
    assert!(o.ok, "erreur Ruby: {}", o.error);
    Some(o)
}

pub(super) fn host() -> TestHost {
    TestHost { app: Stub::default(), st: ScriptState::default() }
}

#[test]
fn each_operation_is_one_undo_step() {
    let mut h = host();
    let mut doc = Document::new(Model::new());
    let Some(o) = run(
        &mut h,
        &mut doc,
        "m = Sketchup.active_model
         m.start_operation('Carré', true)
         m.entities.add_face([0,0,0], [1,0,0], [1,1,0], [0,1,0])
         m.commit_operation
         m.start_operation('Annulé', true)
         m.entities.add_face([5,0,0], [6,0,0], [6,1,0])
         m.abort_operation
         m.start_operation('Triangle', true)
         m.entities.add_face([9,0,0], [10,0,0], [10,1,0])
         m.commit_operation
         m.start_operation('Suite', true, false, true)
         m.entities.add_face([20,0,0], [21,0,0], [21,1,0])
         m.commit_operation
         m.entities.grep(Sketchup::Face).size",
    ) else {
        return;
    };
    assert_eq!(o.result, "3");
    // « Suite » est transparente : elle rejoint « Triangle ».
    assert_eq!(doc.undo_label(), Some("Triangle"));
    doc.undo();
    assert_eq!(doc.model.root.faces.len(), 1);
    assert_eq!(doc.undo_label(), Some("Carré"));
    doc.undo();
    assert!(doc.model.root.faces.is_empty());
    assert!(doc.undo_label().is_none());
}

#[test]
fn reading_ids_is_not_a_change() {
    let mut h = host();
    let mut doc = Document::new(Model::new());
    let Some(_) = run(&mut h, &mut doc, "m = Sketchup.active_model; [m.guid, m.layers.map(&:name)]") else { return };
    assert!(doc.undo_label().is_none(), "lire le GUID ou les étiquettes a modifié le modèle");
}

#[test]
fn save_path_title_and_status() {
    let dir = std::env::temp_dir().join(format!("esq_model_app_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let file = dir.join("maison.esq");
    let f = file.to_string_lossy().replace('\\', "/");
    let mut h = host();
    let sketchy = crate::style::ViewStyle { jitter: true, bg_color: [244, 238, 222], ..Default::default() };
    h.app.view2d = Some((crate::camera::Camera::default(), sketchy.clone()));
    let mut doc = Document::new(Model::new());
    let Some(o) = run(
        &mut h,
        &mut doc,
        &format!(
            "m = Sketchup.active_model
             m.entities.add_face([0,0,0], [1,0,0], [1,1,0])
             r = [m.path, m.modified?, m.save('{f}'), m.title, m.path == '{f}', m.modified?]
             Sketchup.set_status_text('Bonjour')
             Sketchup.status_text = 'Encore'
             r << m.export('{d}/vue.png')
             r << m.save_copy('{d}/copie.esq')
             r << (m.save_copy('{f}') rescue $!.message)
             r",
            d = dir.to_string_lossy().replace('\\', "/")
        ),
    ) else {
        return;
    };
    assert_eq!(
        o.result,
        "[\"\", true, true, \"maison\", true, false, true, true, \"Cannot save copy to current model file.\"]"
    );
    assert!(file.is_file() && dir.join("copie.esq").is_file());
    assert_eq!(h.app.said, ["Bonjour", "Encore"]);
    assert_eq!(h.app.images[0].0, PathBuf::from(dir.join("vue.png").to_string_lossy().replace('\\', "/")));
    assert_eq!(h.st.path.as_deref(), Some(PathBuf::from(&f).as_path()));
    // The model saved is the one of the script, with the view's rendering options (as File ›
    // Save writes them).
    let back = esq_core::io::load_esq(&file).unwrap();
    assert_eq!(back.root.faces.len(), 1);
    let mut style = crate::style::ViewStyle::default();
    style.restore(back.view_ext["ro"].as_object().unwrap());
    assert!(style.jitter && style.bg_color == sketchy.bg_color);

    // Opening a file replaces the model the script works on.
    let mut h = host();
    let mut doc = Document::new(Model::new());
    let Some(o) = run(
        &mut h,
        &mut doc,
        &format!(
            "r = Sketchup.open_file('{f}', with_status: true)
             [r, Sketchup.active_model.entities.grep(Sketchup::Face).size, Sketchup.open_file('{f}.absent')]"
        ),
    ) else {
        return;
    };
    // with_status: true gives the LOAD_STATUS_* code (TestUp, TC_Sketchup#test_open_file_with_status_success).
    assert_eq!(o.result, "[0, 1, false]");
    assert!(matches!(h.st.replace, Some(Replace::Open(_))));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn active_path_follows_the_edit_context() {
    let mut h = host();
    let mut doc = Document::new(Model::new());
    let Some(o) = run(
        &mut h,
        &mut doc,
        "m = Sketchup.active_model
         g = m.entities.add_group
         g.entities.add_face([0,0,0], [1,0,0], [1,1,0])
         r = [m.active_path, m.active_entities == m.entities]
         m.active_path = [g]
         r += [m.active_path == [g], m.active_entities.grep(Sketchup::Face).size, m.edit_transform.identity?]
         g.locked = true
         r << begin; m.active_path = [g]; rescue => e; e.message; end
         m.active_path = nil
         r + [m.active_path]",
    ) else {
        return;
    };
    assert_eq!(o.result, "[nil, true, true, 1, true, \"Cannot open instance path with locked instances\", nil]");
    assert!(h.st.ctx_changed && h.st.ctx.is_empty());
}

/// `erase_entities` effaçait le groupe en cours d'édition : SketchUp lève ArgumentError
/// (TestUp test_erase_entities_active_entities) ; un autre groupe s'efface.
#[test]
fn the_open_group_cannot_be_erased() {
    let mut h = host();
    let mut doc = Document::new(Model::new());
    let Some(o) = run(
        &mut h,
        &mut doc,
        "m = Sketchup.active_model
         g = m.entities.add_group
         g.entities.add_line([0, 0, 0], [9, 9, 9])
         g2 = m.entities.add_group
         g2.entities.add_line([0, 0, 0], [9, 9, 9])
         m.active_path = [g]
         a = begin; m.entities.erase_entities(g); rescue => e; \"#{e.class}: #{e.message}\"; end
         m.entities.erase_entities(g2)
         r = [a, g.deleted?, g2.deleted?]
         m.active_path = nil
         # A component: every instance of the definition being edited stays, the definition too.
         d = m.definitions.add('Porte')
         d.entities.add_line([0, 0, 0], [1, 1, 1])
         i1 = m.entities.add_instance(d, Geom::Transformation.new)
         i2 = m.entities.add_instance(d, Geom::Transformation.new([5, 0, 0]))
         m.active_path = [i1]
         r << begin; m.entities.erase_entities(i2); rescue => e; e.class; end
         r << begin; m.definitions.remove(d); rescue => e; \"#{e.class}: #{e.message}\"; end
         r << begin; i1.erase!; rescue => e; e.class; end
         m.active_path = nil
         r + [i2.deleted?, m.definitions.remove(d), i1.deleted?]",
    ) else {
        return;
    };
    // Drawingelement#erase! too (TestUp test_erase_Bang_active_entities).
    assert_eq!(
        o.result,
        "[\"ArgumentError: cannot remove an instance in the active editing path\", false, true, ArgumentError, \
         \"ArgumentError: cannot remove definition with an instance in the active editing path\", ArgumentError, \
         false, true, true]"
    );
}

/// `DefinitionList#load` du fichier du modèle lui-même : SketchUp refuse « You cannot insert a
/// component or model into itself. » (TestUp test_load_recursive_model).
#[test]
fn a_model_does_not_load_into_itself() {
    let dir = std::env::temp_dir().join(format!("esq_model_recursif_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let f = dir.join("maison.skp").to_string_lossy().replace('\\', "/");
    let mut h = host();
    let mut doc = Document::new(Model::new());
    let Some(o) = run(
        &mut h,
        &mut doc,
        &format!(
            "m = Sketchup.active_model
             m.entities.add_face([0,0,0], [10,0,0], [10,10,0])
             m.save('{f}')
             n = m.definitions.size
             r = begin; m.definitions.load('{f}'); rescue => e; \"#{{e.class}}: #{{e.message}}\"; end
             [r, m.definitions.size - n]"
        ),
    ) else {
        return;
    };
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(o.result, "[\"RuntimeError: You cannot insert a component or model into itself.\", 0]");
}

/// `Sketchup.open_file` d'un fichier d'un SketchUp plus récent :
/// l'ancienne forme n'ouvre pas un fichier qu'on peut lire avec avertissement (false, TestUp
/// test_open_file_legacy_overload_should_not_open_soft_block) ; avec with_status: il s'ouvre
/// (LOAD_STATUS_SUCCESS_MORE_RECENT) ; un fichier illisible ne s'ouvre jamais.
#[test]
fn newer_files_open_like_in_sketchup() {
    let dir = std::env::temp_dir().join(format!("esq_model_recent_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let mut m = Model::new();
    m.root.add_polyline(
        &[esq_core::glam::DVec3::ZERO, esq_core::glam::DVec3::X * 100.0, esq_core::glam::DVec3::new(100.0, 100.0, 0.0)],
        true,
        false,
    );
    let vff = esq_skp::native::write::write(&m, None, None, &Default::default()).unwrap();
    let at = vff.windows(3).position(|w| w == b"VFF").unwrap();
    let (mut soft, mut hard) = (vff.clone(), vff.clone());
    soft[at + 7..at + 9].copy_from_slice(&1000u16.to_le_bytes());
    hard[at + 5..at + 7].copy_from_slice(&1000u16.to_le_bytes());
    std::fs::write(dir.join("soft.skp"), &soft).unwrap();
    std::fs::write(dir.join("hard.skp"), &hard).unwrap();
    let d = dir.to_string_lossy().replace('\\', "/");
    let mut h = host();
    let mut doc = Document::new(Model::new());
    let Some(o) = run(
        &mut h,
        &mut doc,
        &format!(
            "[Sketchup.open_file('{d}/soft.skp'), Sketchup.open_file('{d}/hard.skp', with_status: true),
              Sketchup.open_file('{d}/soft.skp', with_status: true), Sketchup.active_model.entities.grep(Sketchup::Face).size]"
        ),
    ) else {
        return;
    };
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(o.result, "[false, false, 1, 1]");
}

/// `DefinitionList#import` ne lisait qu'OBJ et STL : il passe par les importeurs de
/// l'application, comme Model#import (DWG, DXF, DAE, 3DS… : TestUp test_import), et rend une
/// définition que rien ne place.
#[test]
fn definitions_import_through_the_application() {
    let dir = std::env::temp_dir().join(format!("esq_model_import_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    std::fs::write(dir.join("tri.obj"), "v 0 0 0\nv 10 0 0\nv 0 10 0\nf 1 2 3\n").unwrap();
    std::fs::write(dir.join("rien.xyz"), "?").unwrap();
    let d = dir.to_string_lossy().replace('\\', "/");
    let mut h = host();
    let mut doc = Document::new(Model::new());
    let Some(o) = run(
        &mut h,
        &mut doc,
        &format!(
            "m = Sketchup.active_model
             d = m.definitions.import('{d}/tri.obj')
             [d.class, d.entities.grep(Sketchup::Face).size, d.instances.size, m.entities.size, d.path == '{d}/tri.obj',
              begin; m.definitions.import('{d}/rien.xyz'); rescue => e; e.class; end]"
        ),
    ) else {
        return;
    };
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(o.result, "[Sketchup::ComponentDefinition, 1, 0, 0, true, ArgumentError]");
}

/// `explode` défaisait le groupe en cours d'édition : SketchUp rend false et le laisse
/// (TestUp test_explode_active_entities) ; hors du chemin, il éclate.
#[test]
fn the_open_group_does_not_explode() {
    let mut h = host();
    let mut doc = Document::new(Model::new());
    let Some(o) = run(
        &mut h,
        &mut doc,
        "m = Sketchup.active_model
         g = m.entities.add_group
         g.entities.add_face([0, 0, 0], [5, 0, 0], [5, 9, 0], [0, 9, 0])
         m.active_path = [g]
         r = [g.explode, g.deleted?]
         m.active_path = nil
         r + [g.explode.size, g.deleted?]",
    ) else {
        return;
    };
    assert_eq!(o.result, "[false, false, 5, true]");
}

/// A model a script closes takes its entities' observers with it: the next model's
/// definition of the same number does not inherit them (TestUp
/// test_delete_all_attributes_on_dynamic_property_dictionary after TC_Sketchup_ComponentDefinition).
#[test]
fn a_closed_models_observers_go_with_it() {
    let _l = crate::prefs::TEMPLATE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut h = host();
    let mut doc = Document::new(Model::new());
    let Some(o) = run(
        &mut h,
        &mut doc,
        "heard = []
         ob = Class.new(Sketchup::EntityObserver) { define_method(:onChangeEntity) { |e| heard << e.name } }.new
         d = Sketchup.active_model.definitions.add('Avant')
         d.add_observer(ob)
         d.name = 'Avant2'
         n = heard.size
         Sketchup.active_model.close(true)
         d2 = Sketchup.active_model.definitions.add('Apres')
         d2.name = 'Encore'
         [n, heard.size, d2.name]",
    ) else {
        return;
    };
    assert_eq!(o.result, "[1, 1, \"Encore\"]");
}
