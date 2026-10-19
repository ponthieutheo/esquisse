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
