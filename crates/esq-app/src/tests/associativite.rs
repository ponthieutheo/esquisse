//! Cotes et textes attachés (`esq_core::attach`), pilotés comme à la souris : les outils Cote et
//! Texte les attachent à ce qui est sous le curseur, et ils suivent ce à quoi ils sont attachés.

use esq_core::attach::AttachTo;
use esq_core::glam::DVec3;
use esq_core::model::{AnnotKind, Annotation, EntityRef};

use super::Harness;
use crate::tools::ToolId;

fn annots(h: &Harness) -> Vec<Annotation> {
    h.doc.model.root.annots.values().cloned().collect()
}

fn ends(a: &Annotation) -> (DVec3, DVec3) {
    match a.kind {
        AnnotKind::Linear { a, b, .. } => (a, b),
        AnnotKind::Text { point, .. } => (point, point),
        AnnotKind::ScreenText { .. } => panic!("texte d'écran"),
    }
}

fn near(p: DVec3, q: DVec3) -> bool {
    p.distance(q) < 1e-6
}

/// Placée sur deux coins de la boîte, la cote s'attache à leurs sommets (`CDimensionTool`) ;
/// la face du haut poussée plus haut l'emporte ; annuler et rétablir la ramènent.
#[test]
fn a_dimension_on_two_corners_follows_push_pull_and_undo() {
    let mut h = Harness::new();
    h.rect_and_box();
    h.tool(ToolId::Dimension);
    h.click(DVec3::new(0.0, 0.0, 400.0));
    h.click(DVec3::new(1000.0, 0.0, 400.0));
    h.click(DVec3::new(500.0, -300.0, 700.0));
    let a = annots(&h);
    assert_eq!(a.len(), 1);
    for w in 0..2 {
        let end = a[0].attach[w].as_ref().expect("attachée");
        assert!(end.path.is_empty() && matches!(end.to, AttachTo::Vertex(_)), "{end:?}");
    }
    h.tool(ToolId::PushPull);
    h.click(DVec3::new(500.0, 300.0, 400.0));
    h.hover(DVec3::new(500.0, 300.0, 600.0));
    h.type_vcb("300");
    let (p, q) = ends(&annots(&h)[0]);
    assert!(near(p, DVec3::new(0.0, 0.0, 700.0)) && near(q, DVec3::new(1000.0, 0.0, 700.0)), "{p} {q}");
    h.doc.undo();
    let (p, _) = ends(&annots(&h)[0]);
    assert!(near(p, DVec3::new(0.0, 0.0, 400.0)), "{p}");
    h.doc.redo();
    let (p, _) = ends(&annots(&h)[0]);
    assert!(near(p, DVec3::new(0.0, 0.0, 700.0)), "{p}");
    // Not highlighted: it is associated.
    let id = h.doc.model.root.annots.keys().next().unwrap();
    assert_eq!(crate::annot::style_of(&h.doc.model, &h.doc.model.root, id).highlight, None);
}

/// Un texte posé sur la face d'un groupe s'y attache par le chemin du groupe ; il suit le groupe
/// déplacé, pendant le glisser (l'aperçu) comme après.
#[test]
fn a_text_on_a_group_follows_it_while_it_moves() {
    let mut h = Harness::new();
    h.rect_and_box();
    let all: Vec<EntityRef> = h.doc.ents().faces.keys().map(EntityRef::Face).collect();
    let mut g = None;
    h.doc.commit("Grouper", |m, d| g = m.make_group(d, &all, true, None));
    let g = g.unwrap();
    h.tool(ToolId::Text);
    h.click(DVec3::new(500.0, 300.0, 400.0));
    h.click(DVec3::new(500.0, 300.0, 900.0));
    let a = annots(&h);
    let end = a[0].attach[0].as_ref().expect("attaché");
    assert_eq!(end.path, vec![g]);
    assert!(matches!(end.to, AttachTo::OnFace(..)), "{end:?}");
    h.doc.select(vec![EntityRef::Instance(g)]);
    h.tool(ToolId::Move);
    h.click(DVec3::new(1000.0, 0.0, 0.0));
    h.hover(DVec3::new(1800.0, 0.0, 0.0));
    // While it drags, the preview shows the text with the group.
    let shown = h.doc.display_model().root.annots.values().next().cloned().unwrap();
    assert!(ends(&shown).0.x > 1000.0, "{:?}", ends(&shown));
    h.type_vcb("1500");
    let (p, _) = ends(&annots(&h)[0]);
    assert!(near(p, DVec3::new(2000.0, 300.0, 400.0)), "{p}");
}

/// Cliquer une arête cote l'arête entière, attachée à ses deux sommets : la boîte tournée, la
/// cote tourne avec. Ce qui la porte effacé, elle reste où elle était, détachée et mise en
/// évidence (« Mettre en évidence les cotations non associées »).
#[test]
fn a_dimension_on_an_edge_turns_with_it_and_stays_when_it_goes() {
    let mut h = Harness::new();
    h.rect_and_box();
    h.tool(ToolId::Dimension);
    h.click(DVec3::new(1000.0, 200.0, 400.0));
    h.click(DVec3::new(1000.0, 300.0, 1400.0));
    let a = annots(&h);
    assert!(a[0].attach.iter().all(|e| e.as_ref().is_some_and(|e| matches!(e.to, AttachTo::Vertex(_)))));
    // The box and its dimension, turned together.
    let mut all: Vec<EntityRef> = h.doc.ents().faces.keys().map(EntityRef::Face).collect();
    all.extend(h.doc.ents().annots.keys().map(EntityRef::Annot));
    h.doc.select(all);
    h.tool(ToolId::Rotate);
    h.click(DVec3::ZERO);
    h.click(DVec3::new(1000.0, 0.0, 0.0));
    h.hover(DVec3::new(0.0, 1000.0, 0.0));
    h.type_vcb("90");
    let (p, q) = ends(&annots(&h)[0]);
    // The edge x = 1000, z = 400 of the box, turned a quarter about the blue axis.
    assert!(near(p, DVec3::new(0.0, 1000.0, 400.0)) || near(q, DVec3::new(0.0, 1000.0, 400.0)), "{p} {q}");
    assert!(near(p, DVec3::new(-600.0, 1000.0, 400.0)) || near(q, DVec3::new(-600.0, 1000.0, 400.0)), "{p} {q}");
    // Erased: detached where it was, highlighted.
    // A point of the edge where the cursor takes the edge (not the dimension).
    h.tool(ToolId::Select);
    let at = (1..10)
        .map(|k| p + (q - p) * (k as f64 / 10.0))
        .find(|x| {
            h.click(*x);
            matches!(h.doc.sel.as_slice(), [EntityRef::Edge(_)])
        })
        .expect("un point de l'arête");
    // The edge erased, its vertices stay (the box's other edges hold them): still attached.
    h.tool(ToolId::Eraser);
    h.click(at);
    let a = annots(&h);
    assert!(a[0].attach.iter().all(Option::is_some));
    // The whole box erased (Edit › Delete): detached where it was.
    h.doc.commit("Supprimer", |m, d| {
        let e = m.entities_mut(d);
        let all: Vec<EntityRef> = e.edges.keys().map(EntityRef::Edge).collect();
        e.erase(&all);
    });
    let a = annots(&h);
    assert_eq!(a.len(), 1);
    assert!(a[0].attach.iter().all(Option::is_none));
    assert_eq!(ends(&a[0]), (p, q));
    let id = h.doc.model.root.annots.keys().next().unwrap();
    assert_eq!(crate::annot::style_of(&h.doc.model, &h.doc.model.root, id).highlight, Some([255, 0, 0]));
}
