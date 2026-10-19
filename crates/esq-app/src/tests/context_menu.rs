//! The right-click menu of the view: one test per case with the
//! entries in SketchUp.exe's order (French labels of `Localizable.strings`), then the
//! commands the menu brought.

use esq_core::glam::{DMat4, DVec2, DVec3};
use esq_core::meta::TextureData;
use esq_core::model::{AnnotKind, Annotation, AttrValue, EdgeId, EntityRef, FaceId, Guide, InstanceId, SectionPlane};

use super::Harness;
use super::ui_driver::app;
use crate::app::EsquisseApp;
use crate::commands::Cmd;
use crate::context_menu::{self as cm, CtxCmd, Target};
use crate::pick::PickCache;
use crate::tools::ToolId;

const HEADER: [&str; 4] = ["Infos sur l'entité", "Effacer", "Masquer", "---"];
const HEADER_LOCK: [&str; 5] = ["Infos sur l'entité", "Effacer", "Masquer", "Verrouiller", "---"];
const ZOOM: &str = "Zoom sur la sélection";

/// `parts` one after the other.
fn cat(parts: &[&[String]]) -> Vec<String> {
    parts.iter().flat_map(|p| p.iter().cloned()).collect()
}

fn v(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// A submenu and its entries as `labels` lists them.
fn sub(name: &str, items: &[&str]) -> Vec<String> {
    std::iter::once(format!("{name} ›")).chain(items.iter().map(|i| format!("{name} › {i}"))).collect()
}

fn into_app(h: Harness) -> EsquisseApp {
    let mut a = app();
    a.cam = h.cam.clone();
    a.doc = h.doc;
    a.pick = PickCache::build(&a.doc.model, &a.style);
    a
}

/// A 1000 × 600 × 400 box drawn with Rectangle and Push/Pull.
fn box_app() -> EsquisseApp {
    let mut h = Harness::new();
    h.rect_and_box();
    into_app(h)
}

fn top_face(a: &EsquisseApp) -> FaceId {
    let e = a.doc.ents();
    e.faces.keys().find(|f| e.faces[*f].normal.z > 0.9 && e.pos(e.faces[*f].outer[0]).z > 399.0).expect("top")
}

fn some_edge(a: &EsquisseApp) -> EdgeId {
    let e = a.doc.ents();
    e.edges.keys().find(|x| e.edges[*x].curve.is_none() && e.edges[*x].faces.len() == 2).expect("edge")
}

/// The menu of `e` right-clicked.
fn menu(a: &mut EsquisseApp, e: EntityRef) -> Vec<String> {
    let t = cm::right_click(a, Some(e));
    cm::labels(&cm::build(a, t))
}

/// The menu of what is selected now (Shift+F10 without the pointer over the view).
fn menu_of_selection(a: &mut EsquisseApp) -> Vec<cm::Entry> {
    let t = match a.doc.sel.first() {
        Some(e) => Target::Entity(*e),
        None => Target::Nothing,
    };
    cm::build(a, t)
}

fn group_all(a: &mut EsquisseApp, is_group: bool) -> InstanceId {
    a.select_all();
    let items = a.doc.sel.clone();
    let mut made = None;
    a.doc.commit("g", |m, d| made = m.make_group(d, &items, is_group, None));
    a.doc.select(Vec::new());
    made.expect("group")
}

fn select_items() -> [&'static str; 5] {
    [
        "Arêtes englobantes",
        "Faces connectées",
        "Tous les éléments connectés",
        "Tous les éléments avec la même étiquette",
        "Tous avec le même matériel",
    ]
}

// ------------------------------------------------------------------------- the cases

#[test]
fn nothing_under_the_cursor() {
    let mut a = box_app();
    a.doc.select(vec![EntityRef::Face(top_face(&a))]);
    // A right click in the void empties the selection.
    let t = cm::right_click(&mut a, None);
    assert_eq!(t, Target::Nothing);
    assert!(a.doc.sel.is_empty());
    let want = v(&[
        "Tout sélectionner",
        "---",
        "Créer composant...",
        "Créer un groupe",
        "---",
        "Principaux intervalles entre les grilles",
        "Espacement des grilles mineures",
    ]);
    assert_eq!(cm::labels(&cm::build(&a, t)), want);
    // With the modeling grid shown, its snapping comes first, ticked when on.
    a.style.modeling_grid = true;
    let m = cm::build(&a, t);
    assert_eq!(cm::labels(&m)[5], "Accrochage à la grille");
    assert!(cm::find(&m, "Accrochage à la grille").unwrap().checked.is_some());
    // Both spacings start the Axes tool.
    assert_eq!(cm::find(&m, "Espacement des grilles mineures").unwrap().cmd, Cmd::Tool(ToolId::AxesTool));
}

#[test]
fn face() {
    let mut a = box_app();
    let f = top_face(&a);
    let got = menu(&mut a, EntityRef::Face(f));
    let mut sel = select_items().to_vec();
    sel.extend(["Désélectionner les faces", "Inverser la sélection"]);
    let want = cat(&[
        &v(&HEADER),
        &sub("Sélectionner", &sel),
        &v(&["Créer composant...", "Créer un groupe"]),
        &sub("Surface", &["Sélection", "Étiquette", "Matière"]),
        &sub("Intersection des faces", &["Avec le modèle"]),
        &v(&["Aligner la vue", "Aligner les axes", "Inverser les faces", "Orienter les faces", ZOOM]),
    ]);
    assert_eq!(got, want);
    assert_eq!(a.doc.sel, vec![EntityRef::Face(f)], "the face right-clicked is selected");
}

#[test]
fn textured_face_has_the_texture_submenu() {
    let mut a = box_app();
    let f = top_face(&a);
    a.doc.commit("m", |m, d| {
        let mat = m.add_material("Brique", [200, 80, 60]);
        m.materials[mat].ext.texture = Some(TextureData {
            filename: "b.png".into(),
            image: Default::default(),
            px: [2, 2],
            size: [100.0, 100.0],
            avg: [200, 80, 60],
        });
        m.entities_mut(d).faces[f].front = Some(mat);
    });
    let got = menu(&mut a, EntityRef::Face(f));
    let tail =
        sub("Texture", &["Position", "Réinitialiser la position", "---", "Projetée", "Modifier l'image de texture..."]);
    assert_eq!(got[got.len() - tail.len()..], tail[..], "{got:#?}");
    let m = cm::build(&a, Target::Entity(EntityRef::Face(f)));
    assert_eq!(cm::greyed(&m), vec!["Texture › Réinitialiser la position"], "nothing positioned yet");
    assert_eq!(cm::find(&m, "Texture › Projetée").unwrap().checked, Some(false));
}

#[test]
fn edge() {
    let mut a = box_app();
    let e = some_edge(&a);
    let got = menu(&mut a, EntityRef::Edge(e));
    let want = cat(&[
        &v(&HEADER),
        &sub(
            "Sélectionner",
            &[
                "Faces connectées",
                "Tous les éléments connectés",
                "Tous les éléments avec la même étiquette",
                "Désélectionner les arêtes",
                "Inverser la sélection",
            ],
        ),
        &v(&["Créer composant...", "Créer un groupe", "Adoucir", "Diviser", ZOOM]),
    ]);
    assert_eq!(got, want);
    // A soft edge offers Unsoften.
    a.doc.commit("s", |m, d| m.entities_mut(d).edges[e].soft = true);
    assert!(menu(&mut a, EntityRef::Edge(e)).contains(&"Supprimer l’adoucissement".to_string()));
}

/// A circle of radius 300 drawn with the Circle tool, away from anything else.
fn circle(h: &mut Harness) {
    h.tool(ToolId::Circle);
    h.click(DVec3::new(3000.0, 0.0, 0.0));
    h.hover(DVec3::new(3300.0, 0.0, 0.0));
    h.type_vcb("300");
}

#[test]
fn edge_of_an_arc() {
    let mut h = Harness::new();
    circle(&mut h);
    let mut a = into_app(h);
    let e = a.doc.ents().edges.keys().find(|x| a.doc.ents().edges[*x].curve.is_some()).expect("circle edge");
    let got = menu(&mut a, EntityRef::Edge(e));
    // Explode Curve, Convert to Polygon, Find Center: the edge menu's order.
    let n = got.len();
    assert_eq!(
        got[n - 6..],
        v(&["Adoucir", "Diviser", "Éclater la courbe", "Convertir en polygone", "Rechercher le centre", ZOOM])[..],
        "{got:#?}"
    );
}

#[test]
fn group() {
    let mut a = box_app();
    let g = group_all(&mut a, true);
    let got = menu(&mut a, EntityRef::Instance(g));
    let want = cat(&[
        &v(&HEADER_LOCK),
        &v(&["Modifier le groupe", "Éclater", "Créer composant...", "---"]),
        &sub(
            "Sélectionner",
            &["Tous les éléments avec la même étiquette", "Inverser la sélection", "Connecté avec Snaps"],
        ),
        &v(&["Détacher", "Réinitialiser l'échelle", "Réinitialiser l'inclinaison", "---"]),
        &sub("Intersection des faces", &["Avec le modèle"]),
        &v(&["Adoucir/Lisser les arêtes", ZOOM]),
    ]);
    assert_eq!(got, want);
    let m = cm::build(&a, Target::Entity(EntityRef::Instance(g)));
    assert_eq!(cm::greyed(&m), v(&["Détacher", "Réinitialiser l'échelle", "Réinitialiser l'inclinaison"]));
    // Locked: Unlock, and no Edit nor Make Component.
    a.set_locked(true, false);
    let m = cm::build(&a, Target::Entity(EntityRef::Instance(g)));
    assert_eq!(cm::labels(&m)[3], "Déverrouiller");
    assert!(cm::greyed(&m).iter().any(|x| x == "Effacer"));
    assert!(!cm::find(&m, "Modifier le groupe").unwrap().enabled);
}

#[test]
fn component() {
    let mut a = box_app();
    let c = group_all(&mut a, false);
    let got = menu(&mut a, EntityRef::Instance(c));
    let want = cat(&[
        &v(&HEADER_LOCK),
        &v(&["Modifier le composant", "Rendre unique", "Éclater", "---"]),
        &sub("Sauvegarder", &["Localement..."]),
        &sub("Recharger", &["Localement..."]),
        &v(&["---"]),
        &sub(
            "Sélectionner",
            &[
                "Tous les éléments avec la même étiquette",
                "Toutes les copies",
                "Inverser la sélection",
                "Connecté avec Snaps",
            ],
        ),
        &v(&[
            "Détacher",
            "Modifier les axes",
            "Réinitialiser l'échelle",
            "Réinitialiser l'inclinaison",
            "Mettre la définition à l'échelle",
            "---",
        ]),
        &sub("Intersection des faces", &["Avec le modèle"]),
        &v(&["Adoucir/Lisser les arêtes", ZOOM]),
    ]);
    assert_eq!(got, want);
    // Make Unique needs another copy outside the selection.
    let m = cm::build(&a, Target::Entity(EntityRef::Instance(c)));
    assert!(!cm::find(&m, "Rendre unique").unwrap().enabled);
    let copy = a.doc.ents().instances[c].clone();
    a.doc.commit("copie", |m, d| {
        m.entities_mut(d).instances.insert(copy);
    });
    let m = cm::build(&a, Target::Entity(EntityRef::Instance(c)));
    assert!(cm::find(&m, "Rendre unique").unwrap().enabled);
}

#[test]
fn open_group_and_component() {
    let mut a = box_app();
    let g = group_all(&mut a, true);
    a.doc.enter(g);
    assert_eq!(cm::right_click(&mut a, None), Target::Open);
    let want = v(&["Fermer le groupe", "---", "Créer composant...", "Créer un groupe", "---", "Ajouter des ancrages"]);
    assert_eq!(cm::labels(&cm::build(&a, Target::Open)), want);
    a.doc.exit();
    let c = group_all(&mut a, false);
    a.doc.enter(c);
    let want = v(&[
        "Fermer le composant",
        "---",
        "Créer composant...",
        "Créer un groupe",
        "---",
        "Modifier les axes",
        "Infos sur l'entité",
        "---",
        "Ajouter des ancrages",
    ]);
    let m = cm::build(&a, Target::Open);
    assert_eq!(cm::labels(&m), want);
    assert_eq!(cm::greyed(&m), v(&["Créer composant...", "Créer un groupe"]), "nothing selected");
}

#[test]
fn inside_a_group_menus_end_with_add_snaps() {
    let mut a = box_app();
    let g = group_all(&mut a, true);
    a.doc.enter(g);
    let f = top_face(&a);
    let got = menu(&mut a, EntityRef::Face(f));
    assert!(got.contains(&"Intersection des faces › Avec le contexte".to_string()));
    assert_eq!(got[got.len() - 2..], v(&["---", "Ajouter des ancrages"])[..]);
}

#[test]
fn extensions_entries_come_last_on_entities_only() {
    let mut a = box_app();
    let g = group_all(&mut a, true);
    a.ui_state.extra.ruby.set_context_items(vec![esq_ruby::MenuCommand {
        id: 1,
        menu: "Context".into(),
        title: "Mon extension".into(),
        ..Default::default()
    }]);
    a.doc.enter(g);
    let f = top_face(&a);
    let got = menu(&mut a, EntityRef::Face(f));
    assert_eq!(got[got.len() - 3..], v(&["---", "Ajouter des ancrages", "<extensions>"])[..]);
    // builds the void's menu without calling the extensions.
    a.doc.exit();
    assert!(!cm::labels(&cm::build(&a, Target::Nothing)).contains(&"<extensions>".to_string()));
}

fn image_app() -> (EsquisseApp, InstanceId) {
    let mut a = app();
    let mut made = None;
    a.doc.commit("img", |m, d| {
        let tex = TextureData {
            filename: "photo.png".into(),
            image: esq_core::photo::Blob(std::sync::Arc::new(b"\x89PNG fake".to_vec())),
            px: [4, 2],
            size: [0.0, 0.0],
            avg: [10, 20, 30],
        };
        made = m.add_image(d, "Photo", "photo.png", tex, DVec3::ZERO, DVec3::X, DVec3::Z, 400.0, None);
    });
    (a, made.expect("image"))
}

#[test]
fn image() {
    let (mut a, i) = image_app();
    let got = menu(&mut a, EntityRef::Instance(i));
    let want = cat(&[
        &v(&HEADER),
        &v(&[
            "Inverser la sélection",
            "---",
            "Créer composant...",
            "Créer un groupe",
            "Éclater",
            "Exporter",
            "Recharger...",
            ZOOM,
        ]),
        &sub("Ombres", &["Projeter", "Recevoir"]),
        &v(&["Détacher", "Utiliser comme matière"]),
    ]);
    assert_eq!(got, want);
    let m = cm::build(&a, Target::Entity(EntityRef::Instance(i)));
    assert_eq!(cm::find(&m, "Ombres › Projeter").unwrap().checked, Some(true));
    // Shadows › Cast turns off; Use As Material makes a plain material, once.
    a.run(&eframe::egui::Context::default(), Cmd::Ctx(CtxCmd::Shadows(true)));
    assert_eq!(a.cmd_checked(Cmd::Ctx(CtxCmd::Shadows(true))), Some(false));
    let n = a.doc.model.materials.len();
    a.run(&eframe::egui::Context::default(), Cmd::Ctx(CtxCmd::UseAsMaterial));
    assert_eq!(a.doc.model.materials.len(), n + 1);
    let m = cm::build(&a, Target::Entity(EntityRef::Instance(i)));
    assert!(!cm::find(&m, "Utiliser comme matière").unwrap().enabled, "already a material");
}

#[test]
fn section_plane() {
    let mut a = box_app();
    let mut s = None;
    a.doc.commit("s", |m, d| {
        s = Some(m.entities_mut(d).sections.insert(SectionPlane::new(DVec3::new(0.0, 0.0, 200.0), DVec3::Z)))
    });
    let got = menu(&mut a, EntityRef::Section(s.unwrap()));
    let want = cat(&[
        &v(&HEADER),
        &v(&[
            "Inverser la sélection",
            "---",
            "Inverser",
            "Activer la coupe",
            "Aligner la vue",
            "Créer une tranche de section groupée",
            "Résoudre les problèmes de remplissage de la coupe",
        ]),
    ]);
    assert_eq!(got, want);
    let m = cm::build(&a, Target::Entity(EntityRef::Section(s.unwrap())));
    assert!(cm::greyed(&m).is_empty(), "the slice cuts the box");
    // Troubleshoot Section Fill is greyed while the style shows no section fill.
    a.style.section_fill = false;
    let m = cm::build(&a, Target::Entity(EntityRef::Section(s.unwrap())));
    assert_eq!(cm::greyed(&m), v(&["Résoudre les problèmes de remplissage de la coupe"]));
}

fn annot_app(kind: AnnotKind) -> (EsquisseApp, esq_core::model::AnnotId) {
    let mut a = box_app();
    let mut id = None;
    a.doc.commit("a", |m, d| {
        id = Some(m.entities_mut(d).annots.insert(Annotation {
            kind,
            text: String::new(),
            layer: 0,
            hidden: false,
            attach: Default::default(),
        }))
    });
    (a, id.unwrap())
}

#[test]
fn linear_dimension() {
    let (mut a, d) = annot_app(AnnotKind::Linear {
        a: DVec3::ZERO,
        b: DVec3::new(1000.0, 0.0, 0.0),
        offset: DVec3::new(0.0, -300.0, 0.0),
    });
    let got = menu(&mut a, EntityRef::Annot(d));
    let want = cat(&[
        &v(&HEADER),
        &v(&["Inverser la sélection", "---", "Modifier le texte"]),
        &sub("Position du texte", &["Extérieur au début", "Centré", "Extérieur à la fin"]),
    ]);
    assert_eq!(got, want);
    // Centred by default; Outside Start moves the text before the start (Ruby's 0).
    let m = cm::build(&a, Target::Entity(EntityRef::Annot(d)));
    assert_eq!(cm::find(&m, "Position du texte › Centré").unwrap().checked, Some(true));
    a.run(&eframe::egui::Context::default(), Cmd::Ctx(CtxCmd::TextPosition(0)));
    let key = crate::annot::prop_key(d);
    assert_eq!(a.doc.ents().prop(&key, "text_position"), Some(&AttrValue::Number(0.0)));
    let st = crate::annot::style_of(&a.doc.model, a.doc.ents(), d);
    let an = a.doc.ents().annots[d].clone();
    let l = crate::annot::layout_styled(&a.cam, &an, "1000 mm", &st).unwrap();
    let start = a.cam.project(DVec3::new(0.0, -300.0, 0.0)).unwrap();
    let end = a.cam.project(DVec3::new(1000.0, -300.0, 0.0)).unwrap();
    assert!(l.label.distance(start) < l.label.distance(end), "the text sits outside the start");
}

#[test]
fn radial_dimension_type() {
    let (mut a, d) =
        annot_app(AnnotKind::Linear { a: DVec3::ZERO, b: DVec3::new(250.0, 0.0, 0.0), offset: DVec3::ZERO });
    let key = crate::annot::prop_key(d);
    a.doc.commit("r", |m, dd| m.entities_mut(dd).set_prop(&key, "radial", AttrValue::Text("c:0:1".into())));
    let got = menu(&mut a, EntityRef::Annot(d));
    assert_eq!(got[got.len() - 3..], sub("Type", &["Rayon", "Diamètre"])[..]);
    let st = crate::annot::style_of(&a.doc.model, a.doc.ents(), d);
    let an = a.doc.ents().annots[d].clone();
    assert_eq!(crate::annot::label_styled(&an, &st, a.doc.model.units), "R250 mm");
    a.run(&eframe::egui::Context::default(), Cmd::Ctx(CtxCmd::Diameter(true)));
    let st = crate::annot::style_of(&a.doc.model, a.doc.ents(), d);
    assert_eq!(crate::annot::label_styled(&an, &st, a.doc.model.units), "DIA 500 mm");
}

#[test]
fn text_with_a_leader() {
    let (mut a, t) =
        annot_app(AnnotKind::Text { point: DVec3::new(1000.0, 0.0, 400.0), leader: DVec3::new(300.0, 0.0, 300.0) });
    let got = menu(&mut a, EntityRef::Annot(t));
    let want = cat(&[
        &v(&HEADER),
        &v(&["Inverser la sélection", "---", "Modifier le texte"]),
        &sub("Flèche", &["Aucune", "Point", "Fermée", "Ouverte"]),
        &sub("Légende", &["Suivant la vue", "Punaise", "Masqué"]),
    ]);
    assert_eq!(got, want);
    // Leader › Hidden: the text stays, without its line.
    a.run(&eframe::egui::Context::default(), Cmd::Ctx(CtxCmd::Leader(0)));
    let m = cm::build(&a, Target::Entity(EntityRef::Annot(t)));
    assert_eq!(cm::find(&m, "Légende › Masqué").unwrap().checked, Some(true));
    let st = crate::annot::style_of(&a.doc.model, a.doc.ents(), t);
    let an = a.doc.ents().annots[t].clone();
    assert!(crate::annot::layout_styled(&a.cam, &an, "x", &st).unwrap().segs.is_empty());
    // Arrow › Open.
    a.run(&eframe::egui::Context::default(), Cmd::Ctx(CtxCmd::Arrow(4)));
    assert_eq!(crate::annot::style_of(&a.doc.model, a.doc.ents(), t).arrow, 4);
}

#[test]
fn text_on_the_screen() {
    let (mut a, t) = annot_app(AnnotKind::ScreenText { x: 0.1, y: 0.1 });
    let got = menu(&mut a, EntityRef::Annot(t));
    assert_eq!(got, cat(&[&v(&HEADER), &v(&["Inverser la sélection", "---", "Modifier le texte"])]));
}

#[test]
fn guide() {
    let mut a = box_app();
    let mut g = None;
    a.doc.commit("g", |m, d| g = Some(m.entities_mut(d).guides.insert(Guide::point(DVec3::new(10.0, 10.0, 0.0)))));
    let got = menu(&mut a, EntityRef::Guide(g.unwrap()));
    assert_eq!(got, cat(&[&v(&HEADER), &v(&["Inverser la sélection"])]));
}

#[test]
fn several_faces() {
    let mut a = box_app();
    let f = top_face(&a);
    let other = a.doc.ents().faces.keys().find(|x| *x != f).unwrap();
    a.doc.select(vec![EntityRef::Face(f), EntityRef::Face(other)]);
    let got = menu(&mut a, EntityRef::Face(f));
    let mut sel = select_items().to_vec();
    sel.extend(["Désélectionner les faces", "Inverser la sélection"]);
    let want = cat(&[
        &v(&HEADER),
        &v(&["Éclater"]),
        &sub("Sélectionner", &sel),
        &sub("Surface", &["Sélection", "Étiquette", "Matière"]),
        &v(&["Créer composant...", "Créer un groupe"]),
        &sub("Intersection des faces", &["Avec le modèle", "Avec la sélection"]),
        &v(&["Aligner la vue", "Inverser les faces", ZOOM]),
    ]);
    assert_eq!(got, want);
    assert_eq!(a.doc.sel.len(), 2, "a right click on a selected face keeps the selection");
}

#[test]
fn a_face_with_its_edges_is_one_entity() {
    let mut a = box_app();
    let f = top_face(&a);
    let mut sel: Vec<EntityRef> = a.doc.ents().face_edges(f).into_iter().map(EntityRef::Edge).collect();
    sel.push(EntityRef::Face(f));
    a.doc.select(sel);
    assert!(cm::is_single(a.doc.ents(), &a.doc.sel));
    assert!(menu(&mut a, EntityRef::Face(f)).contains(&"Aligner les axes".to_string()), "the face's menu");
}

#[test]
fn two_solids() {
    let mut h = Harness::new();
    h.rect_and_box();
    let mut a = into_app(h);
    let g1 = group_all(&mut a, true);
    let copy = a.doc.ents().instances[g1].clone();
    let mut g2 = None;
    a.doc.commit("c", |m, d| {
        let mut c = copy;
        c.xform = DMat4::from_translation(DVec3::new(500.0, 0.0, 0.0)) * c.xform;
        let def = m.defs[c.def].clone();
        c.def = m.defs.insert(def);
        g2 = Some(m.entities_mut(d).instances.insert(c));
    });
    a.doc.select(vec![EntityRef::Instance(g1), EntityRef::Instance(g2.unwrap())]);
    let got = menu(&mut a, EntityRef::Instance(g1));
    let mut sel = select_items().to_vec();
    sel.push("Inverser la sélection");
    let want = cat(&[
        &v(&HEADER_LOCK),
        &v(&["Éclater"]),
        &sub("Sélectionner", &sel),
        &sub("Surface", &["Sélection", "Étiquette", "Matière"]),
        &v(&["Créer composant...", "Créer un groupe"]),
        &sub("Intersection des faces", &["Avec le modèle", "Avec la sélection"]),
        &sub("Solides", &["Intersection", "Union", "Soustraire", "Découper", "Diviser"]),
        &v(&["Enveloppe externe", "Adoucir/Lisser les arêtes", ZOOM]),
    ]);
    assert_eq!(got, want);
}

#[test]
fn matched_photo_menu() {
    let mut a = box_app();
    a.doc.model.scenes.push(super::outils::photo_scene("A", DVec3::new(6000.0, 300.0, 1500.0)));
    a.active_scene = Some(0);
    assert!(a.photo_cmd_enabled());
    let got = cm::labels(&cm::build(&a, Target::Nothing));
    assert_eq!(got, v(&["Modifier la photo adaptée", "Projeter la photo", "Zoom sur la photo adaptée"]));
}

#[test]
fn axes() {
    let mut a = box_app();
    a.style.axes = true;
    a.doc.select(vec![EntityRef::Face(top_face(&a))]);
    // On the red axis, away from the box: the axes' menu, the selection emptied.
    let m = a.cam.project(DVec3::new(-2000.0, 0.0, 0.0)).unwrap();
    assert!(cm::axes_at(&a, m));
    cm::open(&mut a, m, true);
    assert_eq!(a.ui_state.extra.ctx_menu.target, Some(Target::Axes));
    assert!(a.doc.sel.is_empty());
    let menu = &a.ui_state.extra.ctx_menu.entries;
    assert_eq!(cm::labels(menu), v(&["Placer", "Déplacer", "Réinitialiser", "Aligner la vue", "Masquer"]));
    assert_eq!(cm::greyed(menu), v(&["Réinitialiser"]), "the axes are where they started");
    // Not on the box's face, nor beside the axis.
    assert!(!cm::axes_at(&a, a.cam.project(DVec3::new(500.0, 300.0, 400.0)).unwrap()));
    assert!(!cm::axes_at(&a, m + DVec2::new(0.0, 30.0)));
    // Reset and Align View.
    let ctx = eframe::egui::Context::default();
    a.doc.model.axes = DMat4::from_translation(DVec3::new(100.0, 50.0, 0.0)) * DMat4::from_rotation_z(0.3);
    a.run(&ctx, Cmd::Ctx(CtxCmd::AlignViewAxes));
    assert!(a.cam.forward().dot(-DVec3::Z) > 0.999 && a.cam.target.distance(DVec3::new(100.0, 50.0, 0.0)) < 1e-9);
    let green = a.doc.model.axes.transform_vector3(DVec3::Y);
    assert!(a.cam.up().dot(green) > 0.99, "green up the screen");
    a.run(&ctx, Cmd::Ctx(CtxCmd::ResetAxes));
    assert_eq!(a.doc.model.axes, DMat4::IDENTITY);
}

// ------------------------------------------------------------------- picking and keys

#[test]
fn right_click_picks_like_the_select_tool() {
    let mut a = box_app();
    let f = top_face(&a);
    let m = a.cam.project(DVec3::new(500.0, 300.0, 400.0)).unwrap();
    assert_eq!(cm::hit_at(&a, m), Some(EntityRef::Face(f)));
    cm::open(&mut a, m, true);
    assert_eq!(a.doc.sel, vec![EntityRef::Face(f)]);
    assert_eq!(a.ui_state.extra.ctx_menu.target, Some(Target::Entity(EntityRef::Face(f))));
    assert!(cm::labels(&a.ui_state.extra.ctx_menu.entries).contains(&"Aligner les axes".to_string()));
    // From the keyboard without the pointer: the selection's menu, the selection unchanged.
    let e = some_edge(&a);
    a.doc.select(vec![EntityRef::Edge(e), EntityRef::Face(f)]);
    cm::open(&mut a, DVec2::new(5.0, 5.0), false);
    assert_eq!(a.doc.sel.len(), 2);
    assert_eq!(menu_of_selection(&mut a), a.ui_state.extra.ctx_menu.entries);
}

// ------------------------------------------------------------------------ the commands

#[test]
fn select_submenu_commands() {
    let mut a = box_app();
    let f = top_face(&a);
    let ctx = eframe::egui::Context::default();
    a.doc.select(vec![EntityRef::Face(f)]);
    a.run(&ctx, Cmd::Ctx(CtxCmd::SelectBounding));
    assert_eq!(a.doc.sel.len(), 5, "the face and its 4 edges");
    a.doc.select(vec![EntityRef::Face(f)]);
    a.run(&ctx, Cmd::Ctx(CtxCmd::SelectConnectedFaces));
    assert_eq!(a.doc.sel.iter().filter(|e| matches!(e, EntityRef::Face(_))).count(), 5, "the top and the 4 sides");
    a.doc.select(vec![EntityRef::Face(f)]);
    a.run(&ctx, Cmd::Ctx(CtxCmd::SelectAllConnected));
    assert_eq!(a.doc.sel.len(), 6 + 12);
    // Same tag: everything is on Untagged.
    a.doc.select(vec![EntityRef::Face(f)]);
    a.run(&ctx, Cmd::Ctx(CtxCmd::SelectSameTag));
    assert_eq!(a.doc.sel.len(), 18);
    // Same material: the painted face only.
    let mut mat = None;
    a.doc.commit("p", |m, d| {
        let x = m.add_material("Rouge", [255, 0, 0]);
        m.entities_mut(d).faces[f].front = Some(x);
        mat = Some(x);
    });
    a.doc.select(vec![EntityRef::Face(f)]);
    a.run(&ctx, Cmd::Ctx(CtxCmd::SelectSameMaterial));
    assert_eq!(a.doc.sel, vec![EntityRef::Face(f)]);
}

#[test]
fn all_instances_of_a_component() {
    let mut a = box_app();
    let c = group_all(&mut a, false);
    let copy = a.doc.ents().instances[c].clone();
    a.doc.commit("copie", |m, d| {
        m.entities_mut(d).instances.insert(copy);
    });
    a.doc.select(vec![EntityRef::Instance(c)]);
    a.run(&eframe::egui::Context::default(), Cmd::Ctx(CtxCmd::SelectAllInstances));
    assert_eq!(a.doc.sel.len(), 2);
}

#[test]
fn soften_explode_curve_polygon_and_centre() {
    let mut h = Harness::new();
    circle(&mut h);
    let mut a = into_app(h);
    let ctx = eframe::egui::Context::default();
    let e = a.doc.ents().edges.keys().find(|x| a.doc.ents().edges[*x].curve.is_some()).unwrap();
    a.doc.select(vec![EntityRef::Edge(e)]);
    // Find Center: a guide point at the centre.
    a.run(&ctx, Cmd::Ctx(CtxCmd::FindCenter));
    let g = a.doc.ents().guides.values().next().expect("guide point");
    assert!(g.is_point() && g.pos.distance(DVec3::new(3000.0, 0.0, 0.0)) < 1e-6);
    // Convert to Polygon: the curve stays, but is no more offered the conversion.
    a.run(&ctx, Cmd::Ctx(CtxCmd::ConvertToPolygon));
    assert!(!menu(&mut a, EntityRef::Edge(e)).contains(&"Convertir en polygone".to_string()));
    // Soften, then Explode Curve: plain edges.
    a.run(&ctx, Cmd::Ctx(CtxCmd::Soften(true)));
    assert!(a.doc.ents().edges[e].soft && !a.doc.ents().edges[e].smooth);
    a.run(&ctx, Cmd::Ctx(CtxCmd::ExplodeCurve));
    assert!(a.doc.ents().edges.values().all(|x| x.curve.is_none()));
    assert!(!menu(&mut a, EntityRef::Edge(e)).contains(&"Éclater la courbe".to_string()));
}

#[test]
fn area_by_tag_and_material() {
    let mut a = box_app();
    let f = top_face(&a);
    let ctx = eframe::egui::Context::default();
    a.doc.select(vec![EntityRef::Face(f)]);
    // Every face of the box is on Untagged: the whole box (2 × (1000·600 + 1000·400 + 600·400)).
    a.run(&ctx, Cmd::Ctx(CtxCmd::AreaTag));
    let want = esq_core::units::format_area(2.0 * (600_000.0 + 400_000.0 + 240_000.0), a.doc.model.units);
    assert_eq!(a.ui_state.extra.ops.area.as_deref(), Some(want.as_str()));
    a.doc.commit("p", |m, d| {
        let x = m.add_material("Rouge", [255, 0, 0]);
        m.entities_mut(d).faces[f].front = Some(x);
    });
    a.run(&ctx, Cmd::Ctx(CtxCmd::AreaMaterial));
    let want = esq_core::units::format_area(600_000.0, a.doc.model.units);
    assert_eq!(a.ui_state.extra.ops.area.as_deref(), Some(want.as_str()));
}

#[test]
fn align_view_and_axes_on_a_face() {
    let mut a = box_app();
    let f = top_face(&a);
    a.doc.select(vec![EntityRef::Face(f)]);
    let ctx = eframe::egui::Context::default();
    a.run(&ctx, Cmd::Ctx(CtxCmd::AlignAxesFace));
    let ax = a.doc.model.axes;
    assert!(ax.z_axis.truncate().distance(DVec3::Z) < 1e-9, "blue along the face's normal");
    assert!((ax.w_axis.z - 400.0).abs() < 1e-9, "the origin on the face");
    a.run(&ctx, Cmd::Ctx(CtxCmd::AlignViewFace));
    assert!(a.cam.forward().dot(-DVec3::Z) > 0.999, "looking straight down at the face");
    assert!(a.cam.target.distance(DVec3::new(500.0, 300.0, 400.0)) < 1e-6, "at its centre");
}

#[test]
fn instance_commands() {
    let mut a = box_app();
    let g = group_all(&mut a, true);
    let ctx = eframe::egui::Context::default();
    a.doc.select(vec![EntityRef::Instance(g)]);
    a.doc.commit("s", |m, d| m.entities_mut(d).instances[g].xform = DMat4::from_scale(DVec3::new(2.0, 1.0, 1.0)));
    let m = cm::build(&a, Target::Entity(EntityRef::Instance(g)));
    assert!(cm::find(&m, "Réinitialiser l'échelle").unwrap().enabled);
    a.run(&ctx, Cmd::Ctx(CtxCmd::ResetScale));
    assert_eq!(a.doc.ents().instances[g].xform, DMat4::IDENTITY);
    // Make Component of a group: it becomes a component in place.
    a.run(&ctx, Cmd::Ctx(CtxCmd::GroupToComponent));
    let [EntityRef::Instance(c)] = a.doc.sel.as_slice() else { panic!("{:?}", a.doc.sel) };
    let c = *c;
    assert!(!a.doc.model.defs[a.doc.ents().instances[c].def].is_group);
    // Change Axes of the component, with the Axes tool: click the new origin, red, green.
    a.run(&ctx, Cmd::Ctx(CtxCmd::ChangeAxes));
    assert_eq!(a.tool.id(), ToolId::AxesTool);
}

#[test]
fn change_axes_tool_moves_the_definition_axes() {
    let mut h = Harness::new();
    h.rect_and_box();
    h.doc.select(h.doc.ents().faces.keys().map(EntityRef::Face).collect());
    let items = h.doc.sel.clone();
    let mut c = None;
    h.doc.commit("c", |m, d| c = m.make_group(d, &items, false, None));
    let c = c.unwrap();
    let def = h.doc.ents().instances[c].def;
    h.tool = Box::new(crate::tools::AxesTool::for_component(Vec::new(), c));
    // Origin at the far top corner, red toward -x, green toward -y.
    h.click(DVec3::new(1000.0, 600.0, 400.0));
    h.click(DVec3::new(0.0, 600.0, 400.0));
    h.click(DVec3::new(1000.0, 0.0, 400.0));
    assert!(h.tool.done(), "the tool gives way once the axes are set");
    let x = h.doc.ents().instances[c].xform;
    assert!(x.w_axis.truncate().distance(DVec3::new(1000.0, 600.0, 400.0)) < 1e-6, "{x:?}");
    // The geometry did not move.
    let world: Vec<DVec3> = h.doc.model.defs[def].entities.verts.values().map(|v| x.transform_point3(v.pos)).collect();
    assert!(world.iter().any(|p| p.distance(DVec3::ZERO) < 1e-6));
    assert!(world.iter().all(|p| p.z > -1e-6 && p.z < 400.0 + 1e-6));
}

#[test]
fn make_unique_of_several_copies() {
    let mut a = box_app();
    let c = group_all(&mut a, false);
    let copy = a.doc.ents().instances[c].clone();
    let mut others = Vec::new();
    a.doc.commit("copies", |m, d| {
        for _ in 0..2 {
            others.push(m.entities_mut(d).instances.insert(copy.clone()));
        }
    });
    a.doc.select(vec![EntityRef::Instance(c), EntityRef::Instance(others[0])]);
    a.run(&eframe::egui::Context::default(), Cmd::Ctx(CtxCmd::MakeUnique));
    let e = a.doc.ents();
    assert_eq!(e.instances[c].def, e.instances[others[0]].def, "the two selected share the new definition");
    assert_ne!(e.instances[c].def, e.instances[others[1]].def);
}

// --- The four entries that stayed greyed.

/// Select › Connected with Snaps: the components whose snaps touch, face to
/// face, those of a component already reached.
#[test]
fn connected_with_snaps_selects_the_chain() {
    use esq_core::model::SnapPoint;
    let mut a = box_app();
    let c = group_all(&mut a, false);
    let def = a.doc.ents().instances[c].def;
    // The block has a snap on each end along red, pointing out.
    a.doc.commit("snaps", |m, _| {
        for (x, dir) in [(1000.0, DVec3::X), (0.0, -DVec3::X)] {
            let s = SnapPoint { pos: DVec3::new(x, 300.0, 0.0), dir, up: DVec3::Z, id: 0, hidden: false, layer: 0 };
            m.add_snap(Some(def), s);
        }
    });
    let copy = a.doc.ents().instances[c].clone();
    let mut others = Vec::new();
    a.doc.commit("copies", |m, d| {
        // Two blocks end to end after the first, then one apart, then one touching but turned.
        for (x, turn) in [(1000.0, false), (2000.0, false), (5000.0, false), (6000.0, true)] {
            let mut i = copy.clone();
            i.xform = DMat4::from_translation(DVec3::new(x, 0.0, 0.0));
            if turn {
                i.xform *= DMat4::from_rotation_z(std::f64::consts::FRAC_PI_2);
            }
            others.push(m.entities_mut(d).instances.insert(i));
        }
    });
    a.doc.select(vec![EntityRef::Instance(c)]);
    let menu = cm::build(&a, Target::Entity(EntityRef::Instance(c)));
    assert!(cm::find(&menu, "Sélectionner › Connecté avec Snaps").is_some_and(|i| i.enabled));
    a.run(&eframe::egui::Context::default(), Cmd::Ctx(CtxCmd::ConnectedSnaps));
    let got: HashSetOf = a.doc.sel.iter().copied().collect();
    let want: HashSetOf = [c, others[0], others[1]].into_iter().map(EntityRef::Instance).collect();
    assert_eq!(got, want, "the chain of the first three, not the one apart");
}

type HashSetOf = std::collections::HashSet<EntityRef>;

/// Troubleshoot Section Fill: a flag of the section plane, no operation;
/// while on, the open chains of the cut (here the box without its top) show in red.
#[test]
fn troubleshoot_section_fill_shows_what_keeps_the_cut_open() {
    let mut a = box_app();
    let top = top_face(&a);
    a.doc.commit("open", |m, d| {
        m.entities_mut(d).faces.remove(top);
    });
    let mut s = None;
    a.doc.commit("s", |m, d| {
        let e = m.entities_mut(d);
        let id = e.sections.insert(SectionPlane::new(DVec3::new(500.0, 0.0, 0.0), DVec3::X));
        e.active_section = Some(id);
        s = Some(id);
    });
    let s = s.unwrap();
    a.doc.select(vec![EntityRef::Section(s)]);
    let red = |a: &EsquisseApp| {
        let opts = crate::scene::SceneOptions {
            ctx: &[],
            ctx_def: None,
            sel: &[],
            axes_xform: DMat4::IDENTITY,
            style: &a.style,
        };
        let mesh = crate::scene::build(&a.doc.model, &opts);
        let c = crate::scene::TROUBLESHOOT_RED.map(|v| v as f32 / 255.0);
        mesh.lines.iter().filter(|l| l.color[..3] == c[..]).count()
    };
    assert_eq!(red(&a), 0);
    let undo = a.doc.undo_label().map(String::from);
    a.run(&eframe::egui::Context::default(), Cmd::Ctx(CtxCmd::TroubleshootFill));
    assert_eq!(a.cmd_checked(Cmd::Ctx(CtxCmd::TroubleshootFill)), Some(true));
    assert_eq!(a.doc.undo_label().map(String::from), undo, "a viewing aid: nothing to undo");
    assert!(red(&a) >= 1, "the open sides of the cut");
    // A closed box: nothing to show.
    let mut b = box_app();
    b.doc.commit("s", |m, d| {
        let e = m.entities_mut(d);
        let mut sp = SectionPlane::new(DVec3::new(500.0, 0.0, 0.0), DVec3::X);
        sp.troubleshoot = true;
        let id = e.sections.insert(sp);
        e.active_section = Some(id);
    });
    assert_eq!(red(&b), 0);
}

/// Project Photo: the photo becomes the material of the selected faces,
/// placed so that each point of a face shows the pixel of the photo seen there; a face that
/// already has a material keeps it when the answer is No.
#[test]
fn project_photo_paints_the_faces_with_what_the_photo_shows() {
    let mut a = box_app();
    let mut scene = super::outils::photo_scene("Façade", DVec3::new(6000.0, -4000.0, 3000.0));
    let mut png = Vec::new();
    let img = image::RgbaImage::from_pixel(16, 12, image::Rgba([200, 120, 40, 255]));
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    scene.photo.as_mut().unwrap().image = esq_core::photo::Blob(std::sync::Arc::new(png));
    a.doc.model.scenes.push(scene);
    a.active_scene = Some(0);
    let photo = a.active_photo().cloned().unwrap();
    // The two faces the camera sees: the front (y = 0) and the top; the top already painted.
    let e = a.doc.ents();
    let front = e.faces.keys().find(|f| e.faces[*f].normal.y < -0.9).unwrap();
    let top = top_face(&a);
    a.doc.commit("peinture", |m, d| {
        let mat = m.add_material("Rouge", [200, 0, 0]);
        m.entities_mut(d).faces[top].front = Some(mat);
    });
    a.doc.select(vec![EntityRef::Face(front), EntityRef::Face(top)]);
    let menu = cm::build(&a, Target::Nothing);
    assert!(cm::find(&menu, "Projeter la photo").is_some_and(|i| i.enabled));
    crate::ctx_dialogs::project(&mut a, false);
    assert_eq!(a.doc.undo_label(), Some("Projeter la photo"));
    let e = a.doc.ents();
    let m = &a.doc.model;
    let mat = e.faces[front].front.expect("the photo's material");
    assert!(m.materials[mat].name.starts_with("Façade") && m.materials[mat].ext.texture.is_some());
    assert_eq!(m.materials[e.faces[top].front.unwrap()].name, "Rouge", "No keeps the top's material");
    // Each point of the face shows the pixel of the photo seen there.
    let map = e.faces[front].tex.as_ref().and_then(|t| t.front).expect("a map");
    for p in [DVec3::new(100.0, 0.0, 50.0), DVec3::new(900.0, 0.0, 380.0), DVec3::new(500.0, 0.0, 200.0)] {
        let uv = map.at(p);
        let px = photo.camera.project(p).unwrap();
        let from_map = DVec2::new(uv.x * photo.camera.size.x, (1.0 - uv.y) * photo.camera.size.y);
        assert!(from_map.distance(px) < 1e-6, "{p:?}: {from_map:?} / {px:?}");
    }
    // Yes: the top too.
    crate::ctx_dialogs::project(&mut a, true);
    assert_eq!(a.doc.ents().faces[top].front, Some(mat));
}

/// The axes' Move (`CMoveSketchCSDlg`): the Move Sketching Context dialog moves the axes along
/// their own directions and turns them, one operation « Déplacer les axes ».
#[test]
fn move_axes_dialog_moves_and_turns_the_axes() {
    let x = crate::ctx_dialogs::moved_axes(DMat4::IDENTITY, DVec3::new(100.0, 0.0, 0.0), DVec3::new(0.0, 0.0, 90.0));
    assert!(x.w_axis.truncate().distance(DVec3::new(100.0, 0.0, 0.0)) < 1e-9);
    assert!(x.x_axis.truncate().distance(DVec3::Y) < 1e-9, "red turned toward green");
    let mut a = app();
    a.run(&eframe::egui::Context::default(), Cmd::Ctx(CtxCmd::MoveAxes));
    let mut ui = super::ui_driver::Driver::new();
    ui.frames(&mut a, 2);
    assert!(ui.has("Déplacer le contexte d'esquisse"));
    ui.type_into(&mut a, "Déplacer X (rouge)", "1000");
    ui.type_into(&mut a, "Faire pivoter Z (bleu)", "90");
    ui.click(&mut a, "OK");
    let ax = a.doc.model.axes;
    let unit = esq_core::units::parse_length("1000", a.doc.model.units).unwrap();
    assert!(ax.w_axis.truncate().distance(DVec3::new(unit, 0.0, 0.0)) < 1e-6, "{ax:?}");
    assert!(ax.x_axis.truncate().distance(DVec3::Y) < 1e-9);
    assert_eq!(a.doc.undo_label(), Some("Déplacer les axes"));
    assert!(!ui.has("Déplacer le contexte d'esquisse"), "OK closes it");
    a.doc.undo();
    assert_eq!(a.doc.model.axes, DMat4::IDENTITY);
}
