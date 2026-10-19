//! Model Info › Text and Dimensions, driven like a user.

use esq_core::glam::{DVec2, DVec3};
use esq_core::model::{AnnotKind, Annotation, EntityRef};

use super::dialogs::open;
use crate::annot::{dim_defaults, style_of, text_defaults};
use crate::commands::Cmd;

fn add(a: &mut crate::app::EsquisseApp, kind: AnnotKind) -> esq_core::model::AnnotId {
    let mut id = None;
    a.doc.commit("t", |m, _| {
        id = Some(m.root.annots.insert(Annotation {
            kind,
            text: "Note".into(),
            layer: 0,
            hidden: false,
            attach: Default::default(),
        }))
    });
    id.unwrap()
}

#[test]
fn text_page_sets_new_texts_and_updates_the_selected_ones() {
    let (mut a, mut ui) = open(Cmd::ModelInfo);
    let screen = add(&mut a, AnnotKind::ScreenText { x: 0.1, y: 0.1 });
    let leader = add(&mut a, AnnotKind::Text { point: DVec3::ZERO, leader: DVec3::new(100.0, 0.0, 100.0) });
    ui.click(&mut a, "Texte");
    assert_eq!(ui.value("Extrémité"), "Point");
    assert_eq!(ui.value("Légende"), "Suivant la vue");

    // New defaults: open arrows, pushpin leaders.
    ui.choose(&mut a, "Extrémité", "Flèche ouverte");
    ui.choose(&mut a, "Légende", "Punaise");
    let d = text_defaults(&a.doc.model);
    assert_eq!((d.arrow, d.leader_type), (4, 2));
    // Existing texts keep their look, as in SketchUp.
    assert_eq!(style_of(&a.doc.model, &a.doc.model.root, leader).arrow, 2);
    let fresh = Annotation {
        kind: AnnotKind::Text { point: DVec3::ZERO, leader: DVec3::X },
        text: String::new(),
        layer: 0,
        hidden: false,
        attach: Default::default(),
    };
    assert_eq!(crate::annot::default_style(&a.doc.model, &fresh).arrow, 4);

    // Select all leader text, then give it the page's look.
    ui.click(&mut a, "Sélectionner tout le texte des légendes");
    assert_eq!(a.doc.sel, vec![EntityRef::Annot(leader)]);
    ui.click(&mut a, "Actualiser le texte sélectionné");
    let st = style_of(&a.doc.model, &a.doc.model.root, leader);
    assert_eq!((st.arrow, st.leader_type), (4, 2));

    // The screen-text font: another family, through "Polices...".
    ui.click(&mut a, "Polices du texte d'écran");
    let families = crate::fonts::font_list();
    if families.families.contains_key("Arial") {
        ui.choose(&mut a, "Famille du texte d'écran", "Arial");
        assert_eq!(text_defaults(&a.doc.model).screen.name, "Arial");
        ui.click(&mut a, "Sélectionner tout le texte d'écran");
        assert_eq!(a.doc.sel, vec![EntityRef::Annot(screen)]);
        ui.click(&mut a, "Actualiser le texte sélectionné");
        assert_eq!(style_of(&a.doc.model, &a.doc.model.root, screen).font.name, "Arial");
    }
    // The Ruby API reads the same properties (`Text#arrow_type`, `#font`).
    let Some(r) = super::dialogs::ruby(
        &mut a,
        "t = Sketchup.active_model.entities.grep(Sketchup::Text).find(&:has_leader?); [t.arrow_type, t.leader_type]",
    ) else {
        return;
    };
    assert_eq!(r, "[4, 2]");
}

#[test]
fn dimensions_page_aligns_text_and_hides_what_is_unreadable() {
    let (mut a, mut ui) = open(Cmd::ModelInfo);
    let p = DVec3::ZERO;
    let q = DVec3::new(1000.0, 1000.0, 0.0);
    let dim = add(&mut a, AnnotKind::Linear { a: p, b: q, offset: DVec3::new(-200.0, 200.0, 0.0) });
    ui.click(&mut a, "Cotations");
    assert_eq!(ui.value("Extrémités"), "Flèche fermée");
    ui.choose(&mut a, "Extrémités", "Barre");
    assert_eq!(dim_defaults(&a.doc.model).arrow, 1);
    ui.click(&mut a, "Aligner sur l'écran");
    ui.click(&mut a, "Aligner sur la ligne de cotation");
    ui.choose(&mut a, "Position du texte", "Centré");
    let d = dim_defaults(&a.doc.model);
    assert!(!d.align_screen && d.position == 1);
    ui.click(&mut a, "Sélectionner toutes les cotations");
    ui.click(&mut a, "Actualiser les cotations sélectionnées");
    let st = style_of(&a.doc.model, &a.doc.model.root, dim);
    assert_eq!((st.arrow, st.align_screen, st.position), (1, false, 1));

    // Seen from above, the 45° dimension's text follows its line.
    let mut cam = crate::camera::Camera {
        size: DVec2::new(800.0, 600.0),
        distance: 5000.0,
        pitch: 89.9f64.to_radians(),
        ..Default::default()
    };
    cam.target = DVec3::new(500.0, 500.0, 0.0);
    let ann = a.doc.model.root.annots[dim].clone();
    let text = crate::annot::label(&ann, a.doc.model.units);
    let l = crate::annot::layout_styled(&cam, &ann, &text, &st).unwrap();
    let s = cam.project(q).unwrap() - cam.project(p).unwrap();
    let line = s.y.atan2(s.x);
    let diff = (l.angle - line).rem_euclid(std::f64::consts::PI);
    assert!(diff.min(std::f64::consts::PI - diff) < 1e-3 && l.angle.abs() > 0.1, "{} / {line}", l.angle);
    // A dimension seen end-on is hidden (expert settings on by default)…
    let along = q.normalize();
    let end_on = (0..360)
        .map(|y| crate::camera::Camera { yaw: (y as f64).to_radians(), pitch: 0.0, ..cam.clone() })
        .max_by(|c1, c2| c1.forward().dot(along).abs().total_cmp(&c2.forward().dot(along).abs()))
        .unwrap();
    assert!(end_on.forward().dot(along).abs() > 0.97);
    assert!(crate::annot::hidden_in_view(&end_on, &ann, &text, &st));
    assert!(!crate::annot::hidden_in_view(&cam, &ann, &text, &st));
    // …unless the expert setting is off.
    ui.click(&mut a, "Paramètres de cotation avancés");
    ui.click(&mut a, "Masquer en vue perspective");
    assert!(!dim_defaults(&a.doc.model).hide_foreshortened);
    // Not attached to the geometry: highlighted.
    assert_eq!(style_of(&a.doc.model, &a.doc.model.root, dim).highlight, Some([255, 0, 0]));
}
