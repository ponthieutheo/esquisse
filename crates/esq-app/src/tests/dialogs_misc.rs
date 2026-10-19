//! Model Info › Components, Rendering, Credits, driven like a user.

use esq_core::glam::{DMat4, DVec3};
use esq_core::model::{Definition, Entities, Instance, InstanceId};

use super::dialogs::{open, ruby};
use crate::app::EsquisseApp;
use crate::commands::Cmd;
use crate::dialogs::mi_misc::{credits, edit_fades, show_instance_axes};

/// A model with one component (a square) placed twice, the first one open for editing.
fn two_copies(a: &mut EsquisseApp) -> (InstanceId, InstanceId) {
    let mut ids = None;
    a.doc.commit("t", |m, _| {
        let mut e = Entities::default();
        let v: Vec<_> = [(0.0, 0.0), (1000.0, 0.0), (1000.0, 1000.0), (0.0, 1000.0)]
            .iter()
            .map(|(x, y)| e.add_vertex_raw(DVec3::new(*x, *y, 0.0)))
            .collect();
        e.add_face_raw(v, vec![]);
        let def = m.defs.insert(Definition {
            name: "Dalle".into(),
            is_group: false,
            description: String::new(),
            entities: e,
            attributes: Default::default(),
            ext: Default::default(),
        });
        let inst = |x: f64| Instance {
            def,
            xform: DMat4::from_translation(DVec3::new(x, 0.0, 0.0)),
            name: String::new(),
            material: None,
            layer: 0,
            hidden: false,
            locked: false,
            attributes: Default::default(),
            ext: Default::default(),
        };
        let i1 = m.root.instances.insert(inst(0.0));
        let i2 = m.root.instances.insert(inst(5000.0));
        ids = Some((i1, i2));
    });
    let (i1, i2) = ids.unwrap();
    a.doc.enter(i1);
    (i1, i2)
}

/// Colour of the faces of the copy that is not open (x > 4000), as the scene draws them.
fn other_copy_color(a: &EsquisseApp) -> [f32; 4] {
    let opts = crate::scene::SceneOptions {
        ctx: &a.doc.ctx,
        ctx_def: a.doc.ctx_def(),
        sel: &a.doc.sel,
        axes_xform: DMat4::IDENTITY,
        style: &a.style,
    };
    let mesh = crate::scene::build(&a.doc.model, &opts);
    mesh.opaque.iter().chain(&mesh.transparent).find(|v| v.pos[0] > 4000.0).map(|v| v.front).unwrap_or([0.0; 4])
}

#[test]
fn components_page_fades_hides_and_shows_axes() {
    let (mut a, mut ui) = open(Cmd::ModelInfo);
    two_copies(&mut a);
    ui.click(&mut a, "Composants");
    assert_eq!(edit_fades(&a.style), (0.25, 0.5));
    let faded = other_copy_color(&a);
    // Slider all the way to "Foncer": the other copy is drawn at full strength.
    let s = ui.node("Estomper les composants similaires");
    ui.click_at(&mut a, eframe::egui::pos2(s.rect.max.x - 1.0, s.rect.center().y));
    let (_, similar) = edit_fades(&a.style);
    assert!(similar < 0.05, "{similar}");
    let strong = other_copy_color(&a);
    let bg = crate::style::ViewStyle::default().bg_color.map(|c| c as f32 / 255.0);
    let dist = |c: [f32; 4]| (0..3).map(|i| (c[i] - bg[i]).abs()).sum::<f32>();
    assert!(dist(strong) > dist(faded) + 0.1, "{strong:?} / {faded:?}");
    assert_eq!(
        ruby(&mut a, "Sketchup.active_model.rendering_options['InstanceFade'] < 0.05").as_deref().unwrap_or("true"),
        "true"
    );

    // "Masquer": View › Component Edit › Hide Similar Components, the same setting.
    ui.click(&mut a, "Masquer les composants similaires");
    assert!(a.style.hide_similar);
    assert!(ui.node("Estomper les composants similaires").disabled);
    if let Some(r) = ruby(&mut a, "Sketchup.active_model.rendering_options['InstanceHidden']") {
        assert_eq!(r, "true");
    }
    ui.click(&mut a, "Masquer le reste du modèle");
    assert!(a.style.hide_rest);

    // Component axes.
    ui.click(&mut a, "Afficher les axes du composant");
    assert!(show_instance_axes(&a.style));
    let opts =
        crate::scene::SceneOptions { ctx: &[], ctx_def: None, sel: &[], axes_xform: DMat4::IDENTITY, style: &a.style };
    let mesh = crate::scene::build(&a.doc.model, &opts);
    let red = crate::settings::axis_color(0).map(|c| c as f32 / 255.0);
    let at_copy = mesh.lines.iter().filter(|l| l.a == [5000.0, 0.0, 0.0] && (l.color[0] - red[0]).abs() < 0.01).count();
    assert_eq!(at_copy, 1, "un axe rouge à l'origine de la seconde copie");
}

#[test]
fn rendering_page_turns_anti_aliased_textures_on_and_off() {
    let (mut a, mut ui) = open(Cmd::ModelInfo);
    ui.click(&mut a, "Rendu");
    assert_eq!(ui.toggled("Utiliser des textures anticrénelées"), Some(false));
    ui.click(&mut a, "Utiliser des textures anticrénelées");
    assert!(a.doc.model.meta.mipmapping);
    if let Some(r) = ruby(&mut a, "Sketchup.active_model.mipmapping?") {
        assert_eq!(r, "true");
    }
    // The "Simple" templates have them on.
    let t = crate::prefs::builtin_templates()[0].model();
    assert!(t.meta.mipmapping);
}

#[test]
fn credits_page_claims_the_model_and_its_components() {
    let (mut a, mut ui) = open(Cmd::ModelInfo);
    two_copies(&mut a);
    ui.click(&mut a, "Crédits");
    assert_eq!(ui.value("Auteurs du modèle"), "Auteurs: Inconnus");
    ui.click(&mut a, "Revendiquer le crédit");
    let who = crate::settings::user_name();
    let (authors, comps) = credits(&a.doc.model);
    assert_eq!(authors, vec![who.clone()]);
    assert_eq!(comps, vec![("Dalle".to_string(), who.clone())]);
    assert_eq!(ui.value("Auteurs du modèle"), format!("Auteurs: {who}"));
    assert!(ui.node("Revendiquer le crédit").disabled);
}

#[test]
fn statistics_page_counts_purges_and_fixes_problems() {
    let (mut a, mut ui) = open(Cmd::ModelInfo);
    two_copies(&mut a);
    a.doc.exit_all();
    a.doc.commit("t", |m, _| {
        m.root.guides.insert(esq_core::model::Guide::point(DVec3::ZERO));
        m.root.guides.insert(esq_core::model::Guide::line(DVec3::ZERO, DVec3::X));
        m.add_material("Inutile", [1, 2, 3]);
    });
    ui.click(&mut a, "Statistiques");
    let n = |ui: &super::ui_driver::Driver, row: &str| ui.value(&format!("Nombre de {row}"));
    assert_eq!(n(&ui, "Instances de composants"), "2");
    assert_eq!(n(&ui, "Faces"), "0");
    assert_eq!((n(&ui, "Guides"), n(&ui, "Points de guide")), ("1".into(), "1".into()));
    assert_eq!(n(&ui, "Définitions de composants"), "1");
    assert_eq!(n(&ui, "Matières"), "1");
    // Expanded: the component's face counts too (its definition, once).
    ui.click(&mut a, "Développer la hiérarchie des composants");
    assert_eq!(n(&ui, "Faces"), "1");
    assert_eq!(n(&ui, "Arêtes"), "4");
    ui.choose(&mut a, "Statistiques de", "Composants");
    assert_eq!(n(&ui, "Guides"), "0");
    assert_eq!(n(&ui, "Faces"), "1");

    // Purge Unused takes the unused material away.
    ui.click(&mut a, "Purger les éléments inutilisés");
    assert_eq!(n(&ui, "Matières"), "0");

    // A face painted with a material that no longer exists: Fix Problems repairs it.
    let d = a.doc.model.defs.keys().next().unwrap();
    a.doc.commit("t", |m, _| {
        let mat = m.add_material("Rouge", [255, 0, 0]);
        let f = m.defs[d].entities.faces.keys().next().unwrap();
        m.defs[d].entities.faces[f].front = Some(mat);
        m.materials.remove(mat);
    });
    ui.click(&mut a, "Corriger les problèmes");
    assert!(ui.has("Résultats de la validation"));
    let f = a.doc.model.defs[d].entities.faces.keys().next().unwrap();
    assert_eq!(a.doc.model.defs[d].entities.faces[f].front, None);
    assert!(ui.nodes.iter().any(|x| x.value.contains("matière corrigée")), "rapport affiché");
    assert_eq!(a.doc.undo_label(), Some("Corriger les problèmes"));
    ui.click(&mut a, "Corriger les problèmes");
    assert!(ui.has("Aucun problème trouvé."));
}

#[test]
fn animation_page_sets_transitions_and_delay_of_the_slideshow() {
    // View › Animation › Settings opens Model Info › Animation.
    let (mut a, mut ui) = open(Cmd::AnimSettings);
    assert!(a.ui_state.model_info);
    assert_eq!(ui.toggled("Activer les transitions entre scènes"), Some(true));
    assert_eq!((a.anim.transition, a.anim.delay), (2.0, 1.0));
    ui.type_into(&mut a, "Délai entre les scènes", "3");
    assert_eq!(crate::dialogs::mi_anim::settings(&a.doc.model).2, 3.0);
    assert_eq!(a.anim.delay, 3.0);
    ui.click(&mut a, "Activer les transitions entre scènes");
    assert!(ui.node("Durée de la transition").disabled);
    assert_eq!(a.anim.transition, 0.0, "scenes change at once");
    if let Some(r) = ruby(
        &mut a,
        "o = Sketchup.active_model.options; [o['PageOptions']['ShowTransition'], o['SlideshowOptions']['SlideTime']]",
    ) {
        assert_eq!(r, "[false, 3.0]");
    }
}
