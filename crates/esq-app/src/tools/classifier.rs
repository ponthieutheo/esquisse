//! Classifier (`CClassifierTool`): a click gives the type chosen in the Classifier (an IFC
//! type of a loaded classification system) to a component or a group. Modifiers, tapped:
//! Alt = sample the type of an object, Shift = make the component unique then assign,
//! Ctrl = remove its type (each toggles back to plain assignment). The status lists them
//! as SketchUp does ("%s | %s", "Key = text" joined by " | ").

use std::sync::Mutex;

use eframe::egui;
use esq_core::classify::{self, Schema};
use esq_core::glam::DVec2;
use esq_core::model::{EntityRef, InstanceId};

use super::{Overlay, Tool, ToolCx, ToolId};

/// The type chosen in the Classifier (schema, type), kept while the application runs, like
/// SketchUp's Classifier toolbar.
static CURRENT: Mutex<Option<(String, String)>> = Mutex::new(None);

pub fn current_type() -> Option<(String, String)> {
    CURRENT.lock().ok().and_then(|c| c.clone())
}

pub fn set_current_type(t: Option<(String, String)>) {
    if let Ok(mut c) = CURRENT.lock() {
        *c = t;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Apply,
    /// Alt.
    Sample,
    /// Ctrl.
    Remove,
    /// Shift.
    MakeUnique,
}

#[derive(Default)]
pub struct ClassifierTool {
    mode: Mode,
    hover: Option<InstanceId>,
    cursor: Option<DVec2>,
    /// The tooltip next to the cursor.
    tip: String,
    note: Option<String>,
}

fn instance_under(cx: &ToolCx, m: DVec2) -> Option<InstanceId> {
    let pick = cx.infer.pick;
    let (o, d) = cx.cam().ray(m);
    let hit = pick
        .screen_edge(cx.cam(), m, crate::pick::EDGE_PX)
        .map(|e| e.2.clone())
        .or_else(|| pick.ray_face(o, d).map(|f| f.1.clone()))?;
    match hit.selectable(&cx.doc.ctx)? {
        EntityRef::Instance(i) => Some(i),
        _ => None,
    }
}

impl ClassifierTool {
    fn toggle(&mut self, m: Mode) {
        self.mode = if self.mode == m { Mode::Apply } else { m };
    }

    /// "Schema:Type" of every type of the object under the cursor.
    fn types_of(cx: &ToolCx, i: InstanceId) -> Vec<String> {
        let def = cx.doc.ents().instances[i].def;
        classify::applied(&cx.doc.model, def).into_iter().map(|(s, t)| format!("{s}:{t}")).collect()
    }

    /// The tooltip next to the cursor (`…d390`, `…dbd0`).
    fn tip_text(&self, cx: &ToolCx) -> String {
        let types = self.hover.map(|i| Self::types_of(cx, i)).unwrap_or_default();
        match self.mode {
            Mode::Apply | Mode::MakeUnique => match current_type() {
                Some((_, t)) => format!("Appliquer {t}"),
                None => "Aucune classification n'a été sélectionnée, rien ne sera appliqué".into(),
            },
            Mode::Remove if types.is_empty() => "Type d'effacement : <non défini>".into(),
            Mode::Remove => format!("Effacer {}", types.join(", ")),
            Mode::Sample if types.is_empty() => "Type d'échantillon : <non défini>".into(),
            Mode::Sample => format!("Échantillonner {}", types.join(", ")),
        }
    }

    fn click(&mut self, cx: &mut ToolCx, i: InstanceId) {
        self.note = None;
        let inst = &cx.doc.ents().instances[i];
        let def = inst.def;
        let locked = inst.locked;
        match self.mode {
            Mode::Sample => {
                if let Some((s, t)) = classify::applied(&cx.doc.model, def).into_iter().next() {
                    set_current_type(Some((s, t)));
                }
                self.mode = Mode::Apply;
            }
            Mode::Remove => {
                cx.doc.select(vec![EntityRef::Instance(i)]);
                if !classify::applied(&cx.doc.model, def).is_empty() {
                    cx.doc.commit("Supprimer le type", |m, _| {
                        classify::remove_all(m, def);
                    });
                }
            }
            Mode::Apply | Mode::MakeUnique => {
                let Some((schema, ty)) = current_type() else {
                    self.note = Some("Aucune classification n'a été sélectionnée, rien ne sera appliqué".into());
                    return;
                };
                if locked {
                    self.note = Some(
                        "Seuls les éléments et les groupes déverrouillés peuvent être attribués comme types de classement."
                            .into(),
                    );
                    return;
                }
                cx.doc.select(vec![EntityRef::Instance(i)]);
                let unique = self.mode == Mode::MakeUnique || cx.doc.model.defs[def].is_group;
                cx.doc.commit("Appliquer le type", |m, d| {
                    let def = if unique { m.make_unique(d, i) } else { def };
                    classify::apply(m, def, &schema, &ty);
                });
            }
        }
        *cx.message = self.note.clone();
    }
}

impl Tool for ClassifierTool {
    fn id(&self) -> ToolId {
        ToolId::Classifier
    }
    fn status(&self) -> String {
        let what = match self.mode {
            Mode::Apply => "Cliquez sur un composant ou groupe pour lui attribuer un type.",
            Mode::Sample => "Cliquez sur un composant ou un groupe pour charger son type dans le classificateur.",
            Mode::Remove => "Cliquez sur un composant ou groupe pour supprimer son type.",
            Mode::MakeUnique => "Cliquez sur une instance de composant pour la rendre unique et lui attribuer un type.",
        };
        format!(
            "{what} | Alt = Échantillonner un type. | Maj = Rendez-le unique et attribuez-lui un type. | Ctrl = Supprimer le type."
        )
    }
    fn alt_tap(&mut self, _cx: &mut ToolCx) {
        self.toggle(Mode::Sample);
    }
    fn shift_tap(&mut self, _cx: &mut ToolCx) {
        self.toggle(Mode::MakeUnique);
    }
    fn ctrl_tap(&mut self, _cx: &mut ToolCx) {
        self.toggle(Mode::Remove);
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.hover = instance_under(cx, m);
        self.cursor = Some(m);
        self.tip = self.tip_text(cx);
    }
    fn release(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse_move(cx, m);
        if let Some(i) = self.hover {
            self.click(cx, i);
            self.tip = self.tip_text(cx);
        }
    }
    fn overlay(&self, o: &mut Overlay) {
        if let (Some(c), Some(_)) = (self.cursor, self.hover) {
            o.tooltip(o.origin + egui::vec2(c.x as f32 + 16.0, c.y as f32 + 16.0), &self.tip);
        }
    }
    fn panel(&mut self, ctx: &egui::Context, rect: egui::Rect, cx: &mut ToolCx) {
        let schemas = classify::schemas(&cx.doc.model);
        let mut load = false;
        egui::Area::new(egui::Id::unique("classifier_panel"))
            .fixed_pos(rect.left_top() + egui::vec2(12.0, 12.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.strong("Classificateur");
                    let cur = current_type();
                    let shown = cur.as_ref().map(|(s, t)| format!("{t} ({s})")).unwrap_or_else(|| "Aucun type".into());
                    egui::ComboBox::from_id_salt("classifier_type").width(220.0).selected_text(shown).show_ui(
                        ui,
                        |ui| {
                            if schemas.is_empty() {
                                ui.label("Aucun système de classification chargé.");
                            }
                            for s in &schemas {
                                ui.label(egui::RichText::new(&s.name).strong());
                                for t in &s.types {
                                    let pick = Some((s.name.clone(), t.clone()));
                                    if ui.selectable_label(cur == pick, t).clicked() {
                                        set_current_type(pick);
                                    }
                                }
                            }
                        },
                    );
                    load = ui.button("Importer un système de classification...").clicked();
                });
            });
        let dialog = || {
            let d = rfd::FileDialog::new().add_filter("Fichiers de classification (*.skc)", &["skc"]);
            match crate::dialogs::mi_classif::import_dir() {
                Some(dir) => d.set_directory(dir),
                None => d,
            }
        };
        if load && let Some(p) = dialog().pick_file() {
            match load_skc_file(&p) {
                Ok((s, data)) => cx.doc.commit("Importer les classements", |m, _| {
                    classify::store_schema(m, &s);
                    classify::keep_file(m, &s.name, data);
                }),
                Err(e) => *cx.message = Some(e),
            }
        }
    }
}

// ---------------------------------------------------------------------------------------
// .skc files: a zip with documentProperties.xml (title), Schemas/*.xsd and *.xsd.filter.

/// Reads a classification system (.skc), like `Classifications#load_schema`: the schema and
/// the file's bytes (kept with the model, a .skp carries them).
pub fn load_skc_file(path: &std::path::Path) -> Result<(Schema, Vec<u8>), String> {
    let bad = || format!("Impossible de lire le système de classification : {}", path.display());
    let data = std::fs::read(path).map_err(|_| bad())?;
    let s = classify::parse_skc(&data, &path.to_string_lossy()).ok_or_else(bad)?;
    Ok((s, data))
}

/// Reads a classification system (.skc), like `Classifications#load_schema`.
#[cfg(test)]
pub fn load_skc(path: &std::path::Path) -> Result<Schema, String> {
    load_skc_file(path).map(|(s, _)| s)
}
#[cfg(test)]
mod tests {
    /// A `.skc` whose types name each other as parent (`substitutionGroup`): reading it
    /// never ended (the parent walk went round the cycle forever).
    #[test]
    fn a_cycle_of_parent_types_ends() {
        let xsd = r#"<xs:schema targetNamespace="urn:x">
<xs:element substitutionGroup="x:B" name="A"/>
<xs:element substitutionGroup="x:A" name="B"/>
</xs:schema>"#;
        let skc = crate::zip::write(&[
            ("documentProperties.xml".into(), b"<dp:title>Boucle</dp:title>".to_vec()),
            ("Schemas/boucle.xsd".into(), xsd.as_bytes().to_vec()),
            ("Schemas/boucle.xsd.filter".into(), b"A\n".to_vec()),
        ]);
        let dir = std::env::temp_dir().join(format!("esq-skc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("boucle.skc");
        std::fs::write(&p, skc).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let q = p.clone();
        std::thread::spawn(move || {
            let _ = tx.send(super::load_skc(&q).map(|s| s.types));
        });
        let types = rx.recv_timeout(std::time::Duration::from_secs(20)).expect("the reading ends").unwrap();
        assert_eq!(types, ["A", "B"]);
        let s = crate::dialogs::mi_classif::read_schema(&p).unwrap();
        assert_eq!(s.types, ["A", "B"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
