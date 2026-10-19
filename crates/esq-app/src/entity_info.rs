//! Window › Entity Info, as SketchUp's `CEntityInfoPage` and its pages (`CSurfacePage` for
//! faces, `CGroupPage`, `CCompInstPage`, `CSectionPage`…): the kind of what is selected, its
//! tag, what each kind has (area and materials of faces, length and soft/smooth of edges,
//! radius and segments of an arc, names and advanced attributes of groups and components, text
//! of a label or a dimension, size of an image), and the toggles of the base page
//! (hidden, locked, cast and receive shadows; their operations).

use std::collections::HashMap;

use eframe::egui::{self, Color32, RichText};
use esq_core::glam::DVec3;
use esq_core::model::{AnnotKind, AttrValue, EntityRef, InstanceId, Model};
use esq_core::tess::face_area;
use esq_core::units::format_length;

use crate::app::EsquisseApp;
use crate::dialogs::name_widget;

/// The property key of an entity in `Entities::props` (Ruby handles use the same letters).
pub fn prop_key(e: EntityRef) -> Option<String> {
    let (k, id) = match e {
        EntityRef::Face(x) => ('f', slotmap::Key::data(&x).as_ffi()),
        EntityRef::Edge(x) => ('e', slotmap::Key::data(&x).as_ffi()),
        EntityRef::Instance(x) => ('i', slotmap::Key::data(&x).as_ffi()),
        _ => return None,
    };
    Some(format!("{k}{id}"))
}

/// One of the shadow flags of an entity (`casts`, `receives`): on unless switched off.
pub fn shadow_flag(app: &EsquisseApp, e: EntityRef, name: &str) -> bool {
    let Some(key) = prop_key(e) else { return true };
    !matches!(app.doc.ents().prop(&key, name), Some(AttrValue::Bool(false)))
}

/// The toggles a kind of entity has in SketchUp: (hidden, locked, cast, receive).
fn toggles_of(e: EntityRef) -> (bool, bool, bool, bool) {
    match e {
        EntityRef::Face(_) => (true, false, true, true),
        EntityRef::Instance(_) => (true, true, true, true),
        _ => (true, false, false, false),
    }
}

/// What SketchUp names a kind of entity in the header (one entity, and,
/// several of the same kind; their French text from `Localizable.strings`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Face,
    Edge,
    Surface,
    Component,
    Group,
    SolidComponent,
    SolidGroup,
    Image,
    Guide,
    GuidePoint,
    LinearDimension,
    Text,
    SectionPlane,
    Snap,
    /// Not in SketchUp's table (its spaces have their own page): our own words.
    Space,
}

impl Kind {
    fn one(self) -> &'static str {
        match self {
            Kind::Face => "Face",
            Kind::Edge => "Arête",
            Kind::Surface => "Surface",
            Kind::Component => "Composant",
            Kind::Group => "Groupe",
            Kind::SolidComponent => "Comp. solide",
            Kind::SolidGroup => "Groupe solide",
            Kind::Image => "image",
            Kind::Guide => "Guide",
            Kind::GuidePoint => "Point de guidage",
            Kind::LinearDimension => "Cotation linéaire",
            Kind::Text => "Texte",
            Kind::SectionPlane => "Plan de section",
            Kind::Snap => "Ancrage",
            Kind::Space => "Espace",
        }
    }

    fn many(self, n: usize) -> String {
        let w = match self {
            Kind::Face => "faces",
            Kind::Edge => "arêtes",
            Kind::Surface => "surfaces",
            Kind::Component => "composants",
            Kind::Group => "groupes",
            Kind::SolidComponent => "comp. solides",
            Kind::SolidGroup => "groupes solides",
            Kind::Image => "images",
            Kind::Guide => "guides",
            Kind::GuidePoint => "points de guidage",
            Kind::LinearDimension => "cotations linéaires",
            Kind::Text => "texte",
            Kind::SectionPlane => "plans de section",
            Kind::Snap => "ancrages",
            Kind::Space => "espaces",
        };
        format!("{n} {w}")
    }
}

fn kind_of(doc: &crate::doc::Document, e: EntityRef, solid: &dyn Fn(InstanceId) -> bool) -> Kind {
    let ents = doc.ents();
    match e {
        EntityRef::Face(_) => Kind::Face,
        EntityRef::Edge(_) => Kind::Edge,
        EntityRef::Instance(i) => {
            let def = ents.instances.get(i).map(|x| x.def);
            if doc.model.image_of(doc.ctx_def(), i).is_some() {
                Kind::Image
            } else if def.and_then(|d| doc.model.defs.get(d)).is_some_and(|d| d.is_group) {
                if solid(i) { Kind::SolidGroup } else { Kind::Group }
            } else if solid(i) {
                Kind::SolidComponent
            } else {
                Kind::Component
            }
        }
        EntityRef::Guide(g) => {
            if ents.guides.get(g).is_some_and(|x| x.dir == DVec3::ZERO) {
                Kind::GuidePoint
            } else {
                Kind::Guide
            }
        }
        EntityRef::Annot(a) => match ents.annots.get(a).map(|x| &x.kind) {
            Some(AnnotKind::Linear { .. }) => Kind::LinearDimension,
            _ => Kind::Text,
        },
        EntityRef::Section(_) => Kind::SectionPlane,
        EntityRef::Snap(_) => Kind::Snap,
        EntityRef::Space(_) => Kind::Space,
    }
}

/// The curve every selected edge belongs to when the selection is that whole curve and only it
/// (one edge of a curve counts as the curve, as in SketchUp).
fn whole_curve(ents: &esq_core::model::Entities, sel: &[EntityRef]) -> Option<u32> {
    let mut curve = None;
    for e in sel {
        let EntityRef::Edge(ed) = e else { return None };
        let c = ents.edges.get(*ed)?.curve?;
        if curve.is_some_and(|x| x != c) {
            return None;
        }
        curve = Some(c);
    }
    let c = curve?;
    (sel.len() == 1 || esq_core::curves::curve_edges(ents, c).len() == sel.len()).then_some(c)
}

/// "Arc", "Cercle", "Polygone" or "Courbe" (an arc curve that is a polygon,
/// a closed arc, an open one; any other curve).
fn curve_name(ents: &esq_core::model::Entities, c: u32) -> &'static str {
    let kind = match ents.prop(&format!("c{c}"), "kind") {
        Some(AttrValue::Text(t)) => t.as_str(),
        _ => "",
    };
    match esq_core::curves::arc_fit(ents, c) {
        Some(_) if kind == "polygon" => "Polygone",
        Some(fit) if kind != "curve" => {
            if fit.closed {
                "Cercle"
            } else {
                "Arc"
            }
        }
        _ => "Courbe",
    }
}

/// Whether the selection is one whole surface: two faces or more joined by soft edges, with
/// nothing else than those faces and soft edges between them (kind 2).
fn whole_surface(ents: &esq_core::model::Entities, sel: &[EntityRef]) -> bool {
    use std::collections::HashSet;
    let faces: HashSet<_> =
        sel.iter().filter_map(|e| if let EntityRef::Face(f) = e { Some(*f) } else { None }).collect();
    let Some(&first) = faces.iter().next() else { return false };
    if faces.len() < 2 {
        return false;
    }
    let mut seen = HashSet::from([first]);
    let mut todo = vec![first];
    while let Some(f) = todo.pop() {
        for e in ents.face_edges(f) {
            let Some(edge) = ents.edges.get(e) else { continue };
            if !edge.soft {
                continue;
            }
            for g in &edge.faces {
                if seen.insert(*g) {
                    todo.push(*g);
                }
            }
        }
    }
    seen == faces
        && sel.iter().all(|e| match e {
            EntityRef::Face(_) => true,
            EntityRef::Edge(x) => {
                ents.edges.get(*x).is_some_and(|ed| ed.soft && ed.faces.iter().all(|g| faces.contains(g)))
            }
            _ => false,
        })
}

/// The header of Entity Info, as SketchUp writes it: the name of one entity
/// ("Face", "Groupe solide"), of a whole curve ("Arc", "Cercle"…) or surface, "%d faces" for
/// several of one kind, "%d Entités" for a mix; then " (%d verrouillé)" or " (Tous
/// verrouillés)" when some or all are locked, else " (%d dans modèle)" for copies of one
/// definition. `solid` says whether a group or component is a solid.
pub fn title(doc: &crate::doc::Document, sel: &[EntityRef], solid: &dyn Fn(InstanceId) -> bool) -> String {
    if sel.is_empty() {
        return "Aucune sélection".into();
    }
    let ents = doc.ents();
    let n = sel.len();
    let mut out = if let Some(c) = whole_curve(ents, sel) {
        curve_name(ents, c).to_string()
    } else if n == 1 {
        kind_of(doc, sel[0], solid).one().to_string()
    } else if whole_surface(ents, sel) {
        Kind::Surface.one().to_string()
    } else {
        let kinds: Vec<Kind> = sel.iter().map(|e| kind_of(doc, *e, solid)).collect();
        let instance = |k: &Kind| matches!(k, Kind::Component | Kind::Group | Kind::SolidComponent | Kind::SolidGroup);
        if kinds.iter().all(|k| *k == kinds[0]) {
            kinds[0].many(n)
        } else if kinds.iter().all(instance) {
            let all_solid = kinds.iter().all(|k| matches!(k, Kind::SolidComponent | Kind::SolidGroup));
            let groups = kinds.iter().all(|k| matches!(k, Kind::Group | Kind::SolidGroup));
            let comps = kinds.iter().all(|k| matches!(k, Kind::Component | Kind::SolidComponent));
            match (all_solid, groups, comps) {
                (true, true, _) => Kind::SolidGroup.many(n),
                (true, _, true) => Kind::SolidComponent.many(n),
                (true, _, _) => format!("{n} Composants solides/groupes"),
                (false, true, _) => Kind::Group.many(n),
                (false, _, true) => Kind::Component.many(n),
                _ => format!("{n} comp./groupes"),
            }
        } else {
            format!("{n} Entités")
        }
    };
    let locked = sel
        .iter()
        .filter(|e| matches!(e, EntityRef::Instance(i) if ents.instances.get(*i).is_some_and(|x| x.locked)))
        .count();
    if locked > 0 {
        if locked < n {
            out.push_str(&format!(" ({locked} verrouillé)"));
            return out;
        }
        if locked != 1 {
            out.push_str(" (Tous verrouillés)");
            return out;
        }
    }
    // Copies of one definition (components, groups, images).
    let mut def = None;
    for e in sel {
        let EntityRef::Instance(i) = e else { return out };
        let Some(d) = ents.instances.get(*i).map(|x| x.def) else { return out };
        if def.is_some_and(|x| x != d) {
            return out;
        }
        def = Some(d);
    }
    if let Some(d) = def {
        let copies = doc.model.instance_count(d);
        if copies > 0 {
            out.push_str(&format!(" ({copies} dans modèle)"));
        }
    }
    out
}

pub fn show(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let sel = app.doc.sel.clone();
    if sel.is_empty() {
        ui.label("Aucune sélection.");
        return;
    }
    let solid = |i: InstanceId| {
        let d = app.doc.ents().instances.get(i).map(|x| x.def);
        d.is_some_and(|d| solid_volume(ui, app, i, d).is_some())
    };
    ui.label(RichText::new(title(&app.doc, &sel, &solid)).strong());
    let locked = sel
        .iter()
        .any(|e| matches!(e, EntityRef::Instance(i) if app.doc.ents().instances.get(*i).is_some_and(|x| x.locked)));
    // A locked group or component shows its settings greyed, except the lock (as SketchUp).
    ui.add_enabled_ui(!locked, |ui| {
        tag_row(app, ui, &sel);
        measures(app, ui, &sel);
        faces(app, ui, &sel);
        edges(app, ui, &sel);
        match sel.as_slice() {
            [EntityRef::Instance(i)] => instance(app, ui, *i),
            [EntityRef::Annot(a)] => annotation(app, ui, *a),
            [EntityRef::Section(s)] => section(app, ui, *s),
            [EntityRef::Space(s)] => space(app, ui, *s),
            _ => {}
        }
    });
    toggles(app, ui, &sel, locked);
}

/// "Étiquette:": the tag of the selection.
fn tag_row(app: &mut EsquisseApp, ui: &mut egui::Ui, sel: &[EntityRef]) {
    let ents = app.doc.ents();
    let first = sel.first().and_then(|e| ents.layer_of(*e));
    let mixed = sel.iter().any(|e| ents.layer_of(*e) != first);
    let layers: Vec<String> = app.doc.model.layers.iter().map(|l| l.name.clone()).collect();
    let mut chosen = first.unwrap_or(0);
    let shown = if mixed { String::new() } else { layers.get(chosen).cloned().unwrap_or_default() };
    egui::ComboBox::from_label("Étiquette:").selected_text(shown).show_ui(ui, |ui| {
        for (i, name) in layers.iter().enumerate() {
            ui.selectable_value(&mut chosen, i, name);
        }
    });
    if Some(chosen) != first || (mixed && chosen != first.unwrap_or(0)) {
        let sel = sel.to_vec();
        app.doc.commit("Changer d'étiquette", |m, d| {
            let e = m.entities_mut(d);
            for it in &sel {
                e.set_layer(*it, chosen);
            }
        });
    }
}

/// The area of the faces ("Surface :") and the length of the edges ("Longueur :"), and for
/// the edges of one arc, circle or polygon, its radius and number of segments.
fn measures(app: &mut EsquisseApp, ui: &mut egui::Ui, sel: &[EntityRef]) {
    let units = app.doc.model.units;
    let ents = app.doc.ents();
    let x = app.doc.ctx_xform();
    let (mut nf, mut ne, mut area, mut length) = (0, 0, 0.0, 0.0);
    let mut curves = std::collections::BTreeSet::new();
    for e in sel {
        match *e {
            EntityRef::Face(f) if ents.faces.contains_key(f) => {
                nf += 1;
                area += face_area(ents, f);
            }
            EntityRef::Edge(ed) if ents.edges.contains_key(ed) => {
                ne += 1;
                let (a, b) = ents.edge_points(ed);
                length += x.transform_point3(a).distance(x.transform_point3(b));
                curves.insert(ents.edges[ed].curve);
            }
            _ => {}
        }
    }
    let grid = |ui: &mut egui::Ui, rows: &[(&str, String)]| {
        egui::Grid::new(("ei_measures", rows.len())).num_columns(2).show(ui, |ui| {
            for (k, v) in rows {
                ui.label(*k);
                ui.label(v.as_str());
                ui.end_row();
            }
        });
    };
    if nf > 0 {
        grid(ui, &[("Surface :", esq_core::units::format_area(area, units))]);
    }
    if ne == 0 {
        return;
    }
    // Every selected edge is one curve, and the whole of it: an arc, a circle or a polygon
    // (`CArcPP`); another curve shows its length only (`CCurvePP`).
    let one = match (curves.len(), curves.iter().next()) {
        (1, Some(Some(c))) if whole_curve(ents, sel) == Some(*c) => Some(*c),
        _ => None,
    };
    if let Some(c) = one
        && arc_page(app, ui, c)
    {
        return;
    }
    grid(ui, &[("Longueur :", format_length(length, units))]);
}

/// The uniform scale of the open context (Entity Info shows world sizes), `None` when it
/// stretches one direction more than another (radius and size are then not editable).
fn uniform_scale(x: &esq_core::glam::DMat4) -> Option<f64> {
    let s = [x.x_axis, x.y_axis, x.z_axis].map(|a| a.truncate().length());
    (s.iter().all(|v| (v - s[0]).abs() <= s[0] * 1e-9) && s[0] > 1e-12).then_some(s[0])
}

/// The page of an arc, a circle or a polygon (`CArcPP`): Radius and Segments, typed like
/// SketchUp, and its length (« Longueur de l'arc »,
/// « Circonférence », « Périmètre »). False when the curve is no arc.
fn arc_page(app: &mut EsquisseApp, ui: &mut egui::Ui, c: u32) -> bool {
    use esq_core::arc_edit::{self, ArcCurve};
    let ents = app.doc.ents();
    if matches!(ents.prop(&format!("c{c}"), "kind"), Some(AttrValue::Text(t)) if t == "curve") {
        return false;
    }
    let Some(arc) = ArcCurve::of(ents, c) else { return false };
    let units = app.doc.model.units;
    let scale = uniform_scale(&app.doc.ctx_xform());
    let s = scale.unwrap_or(1.0);
    let polygon = matches!(ents.prop(&format!("c{c}"), "kind"), Some(AttrValue::Text(t)) if t == "polygon");
    let segs = arc.segments();
    let editable_segs = arc_edit::segments_editable(ents, c);
    let radius_text = format_length(arc.fit.radius * s, units);
    let mut radius_typed = None;
    let mut segs_typed = None;
    egui::Grid::new("ei_arc").num_columns(2).show(ui, |ui| {
        ui.add_enabled_ui(scale.is_some(), |ui| {
            radius_typed = field(ui, &format!("radius{c}"), "Rayon:", &radius_text, "");
        });
        ui.end_row();
        ui.add_enabled_ui(editable_segs, |ui| {
            segs_typed = field(ui, &format!("segs{c}"), "Segments :", &segs.to_string(), "");
        });
        ui.end_row();
        let label = match (polygon, arc.fit.closed) {
            (true, true) => "Périmètre :",
            (true, false) => "Longueur :",
            (false, true) => "Circonférence :",
            (false, false) => "Longueur de l'arc :",
        };
        let length = if polygon {
            esq_core::curves::curve_edges(ents, c)
                .iter()
                .map(|e| {
                    let (a, b) = ents.edge_points(*e);
                    a.distance(b)
                })
                .sum::<f64>()
        } else {
            arc.length()
        };
        ui.label(label);
        ui.label(format_length(length * s, units));
        ui.end_row();
    });
    let sweep = arc.sweep();
    if let Some(t) = radius_typed
        && let Some(r) = esq_core::units::parse_length(&t, units).map(|r| r / s)
    {
        // A value that did not change, a null one or one SketchUp refuses leaves the arc as
        // it was (the field shows the radius again).
        let changed = (arc.fit.radius - r).abs() * 1.0e12 > r.abs().min(arc.fit.radius.abs());
        if changed && r.abs() / 25.4 > 0.001 && arc_edit::check(r, segs, sweep).is_ok() {
            app.doc.commit("Propriétés", |m, d| {
                arc_edit::set_radius(m.entities_mut(d), c, r);
            });
            reselect_curve(app, c);
        }
    }
    if let Some(t) = segs_typed {
        let n: i64 = t.trim().parse().unwrap_or(0);
        if n != segs as i64 {
            let min = arc.min_segments() as i64;
            if n < min || n > 999 {
                crate::warnings::warn(format!("Les segments de la courbe doivent être compris entre {min} et 999"));
            } else if arc_edit::check(arc.fit.radius, n as usize, sweep).is_err() {
                crate::warnings::warn(format!(
                    "Le nombre de segments de la courbe <{n}> est trop élevé pour l'angle et le rayon donnés"
                ));
            } else {
                app.doc.commit("Propriétés", |m, d| {
                    arc_edit::set_segments(m.entities_mut(d), c, n as usize);
                });
                reselect_curve(app, c);
            }
        }
    }
    true
}

/// After an edit of the curve, its new edges are the selection.
fn reselect_curve(app: &mut EsquisseApp, c: u32) {
    let edges = esq_core::curves::curve_edges(app.doc.ents(), c).into_iter().map(EntityRef::Edge).collect();
    app.doc.select(edges);
}

/// "Matière :": the front and back materials of the selected faces (a click makes one the
/// current material of Window › Materials).
fn faces(app: &mut EsquisseApp, ui: &mut egui::Ui, sel: &[EntityRef]) {
    let ents = app.doc.ents();
    let faces: Vec<_> =
        sel.iter().filter_map(|e| if let EntityRef::Face(f) = e { ents.faces.get(*f) } else { None }).collect();
    let Some(first) = faces.first() else { return };
    let same = |f: &dyn Fn(&esq_core::model::Face) -> Option<esq_core::model::MaterialId>| {
        let v = f(first);
        faces.iter().all(|x| f(x) == v).then_some(v)
    };
    let (front, back) = (same(&|f| f.front), same(&|f| f.back));
    let mut pick = None;
    ui.horizontal(|ui| {
        ui.label("Matière :");
        for (side, m) in [("Avant", front), ("Arrière", back)] {
            let mat = m.flatten().and_then(|id| app.doc.model.materials.get(id));
            let name = match m {
                None => "Matières diverses".to_string(),
                Some(None) => "Par défaut".to_string(),
                Some(Some(_)) => mat.map(|x| x.name.clone()).unwrap_or_default(),
            };
            let r = crate::mat_panel::swatch(ui, mat, 24.0, false).on_hover_text(format!("{side} : {name}"));
            name_widget(ui, &r, &format!("Matière {}", side.to_lowercase()), Some(&name));
            if r.clicked() {
                pick = Some(m.flatten());
            }
        }
    });
    if let Some(m) = pick {
        app.material = m;
        app.ui_state.panels.insert(crate::commands::Panel::Materials);
        app.ui_state.unfold = Some(crate::commands::Panel::Materials);
    }
}

/// "Adouci :" and "Lisser :" for the selected edges.
fn edges(app: &mut EsquisseApp, ui: &mut egui::Ui, sel: &[EntityRef]) {
    let ents = app.doc.ents();
    let ids: Vec<_> = sel.iter().filter_map(|e| if let EntityRef::Edge(x) = e { Some(*x) } else { None }).collect();
    let list: Vec<_> = ids.iter().filter_map(|e| ents.edges.get(*e)).collect();
    if list.is_empty() {
        return;
    }
    let (mut soft, mut smooth) = (list.iter().all(|e| e.soft), list.iter().all(|e| e.smooth));
    let (soft0, smooth0) = (soft, smooth);
    egui::Grid::new("ei_edges").num_columns(2).show(ui, |ui| {
        for (label, v) in [("Adouci :", &mut soft), ("Lisser :", &mut smooth)] {
            let l = ui.label(label);
            let r = ui.checkbox(v, "");
            name_widget(ui, &r.labelled_by(l.id), label, None);
            ui.end_row();
        }
    });
    if (soft, smooth) != (soft0, smooth0) {
        let label = if soft != soft0 { if soft { "Adoucir" } else { "Supprimer l’adoucissement" } } else { "Lisser" };
        app.doc.commit(label, |m, d| {
            let e = m.entities_mut(d);
            for id in &ids {
                if let Some(x) = e.edges.get_mut(*id) {
                    if soft != soft0 {
                        x.soft = soft;
                    }
                    if smooth != smooth0 {
                        x.smooth = smooth;
                    }
                }
            }
        });
    }
}

/// A text field that commits on Enter or when it loses the focus (`hint`: SketchUp's
/// placeholder text).
fn field(ui: &mut egui::Ui, id: &str, label: &str, value: &str, hint: &str) -> Option<String> {
    let mut text = ui
        .data_mut(|d| d.get_temp::<(String, String)>(egui::Id::unique(("ei_field", id))))
        .filter(|(was, _)| was == value)
        .map(|(_, t)| t)
        .unwrap_or_else(|| value.to_string());
    let mut out = None;
    ui.horizontal(|ui| {
        let l = ui.label(label);
        let r = ui.add(egui::TextEdit::singleline(&mut text).hint_text(hint).desired_width(170.0));
        let r = r.labelled_by(l.id);
        name_widget(ui, &r, label, Some(&text));
        if r.lost_focus() && text != value {
            out = Some(text.clone());
        }
    });
    // Once given, the field shows the value again (SketchUp sets the text back after a value
    // it refuses; an accepted one changes the value).
    let keep = if out.is_some() { value.to_string() } else { text };
    ui.data_mut(|d| d.insert_temp(egui::Id::unique(("ei_field", id)), (value.to_string(), keep)));
    out
}

fn text_attr(a: &esq_core::model::Attributes, dict: &str, key: &str) -> String {
    match a.get(dict).and_then(|d| d.get(key)) {
        Some(AttrValue::Text(t)) => t.clone(),
        Some(AttrValue::Number(n)) => format!("{n}"),
        Some(AttrValue::Json(j)) => j.as_str().map(String::from).unwrap_or_else(|| j.to_string()),
        Some(v) => format!("{v:?}"),
        None => String::new(),
    }
}

/// Faces a definition holds once its copies are laid out (each definition counted once,
/// saturating): how much flattening it would take. Worked out without recursion (an explicit
/// stack, children before parents), so that a chain of nested definitions however long cannot
/// overflow the stack; a cycle counts for nothing.
pub fn flattened_faces(m: &Model, d: esq_core::model::DefId, memo: &mut HashMap<esq_core::model::DefId, u64>) -> u64 {
    use esq_core::model::DefId;
    // (definition, its children already pushed)
    let mut stack: Vec<(DefId, bool)> = vec![(d, false)];
    let mut busy = std::collections::HashSet::new();
    while let Some((x, expanded)) = stack.pop() {
        if memo.contains_key(&x) {
            continue;
        }
        let Some(def) = m.defs.get(x) else {
            memo.insert(x, 0);
            continue;
        };
        if expanded {
            let n =
                def.entities.instances.values().fold(def.entities.faces.len() as u64, |n, i| {
                    n.saturating_add(memo.get(&i.def).copied().unwrap_or(0))
                });
            busy.remove(&x);
            memo.insert(x, n);
        } else if busy.insert(x) {
            stack.push((x, true));
            for i in def.entities.instances.values() {
                if !memo.contains_key(&i.def) && !busy.contains(&i.def) {
                    stack.push((i.def, false));
                }
            }
        }
    }
    memo.get(&d).copied().unwrap_or(0)
}

/// How deep copies are nested in a definition (each definition looked at once, without
/// recursion); `limit` and beyond count as `limit`.
pub fn nesting_depth(m: &Model, d: esq_core::model::DefId, limit: usize) -> usize {
    let mut best: HashMap<esq_core::model::DefId, usize> = HashMap::new();
    let mut stack = vec![(d, 0usize)];
    let mut deepest = 0;
    while let Some((x, depth)) = stack.pop() {
        deepest = deepest.max(depth);
        if deepest >= limit {
            return limit;
        }
        // A definition already reached as deep or deeper adds nothing.
        if best.get(&x).is_some_and(|b| *b >= depth) {
            continue;
        }
        best.insert(x, depth);
        if let Some(def) = m.defs.get(x) {
            stack.extend(def.entities.instances.values().map(|i| (i.def, depth + 1)));
        }
    }
    deepest
}

/// The most faces a solid's volume is worked out from (beyond, the volume is not shown).
pub const MAX_VOLUME_FACES: u64 = 200_000;

/// « Volume : » of the selected group or component: worked out once per version of the model
/// (not at every frame), and not at all for an instance that would flatten into too many
/// faces (copies nested within copies) or whose copies nest deeper than the volume looks.
fn solid_volume(ui: &egui::Ui, app: &EsquisseApp, i: InstanceId, d: esq_core::model::DefId) -> Option<f64> {
    let key = egui::Id::unique(("ei_volume", i));
    let rev = app.doc.rev;
    if let Some((r, v)) = ui.data(|x| x.get_temp::<(u64, Option<f64>)>(key))
        && r == rev
    {
        return v;
    }
    // `Model::solid_volume` lays out 16 levels of copies at most: deeper, its volume would be
    // that of part of the content.
    let m = &app.doc.model;
    let small = nesting_depth(m, d, 17) <= 16 && flattened_faces(m, d, &mut HashMap::new()) <= MAX_VOLUME_FACES;
    let v = if small { app.doc.model.solid_volume(app.doc.ctx_def(), i) } else { None };
    ui.data_mut(|x| x.insert_temp(key, (rev, v)));
    v
}

/// A group or a component: its names, copies, class, volume and, for a component, the
/// advanced attributes (`CCompInstPage`).
fn instance(app: &mut EsquisseApp, ui: &mut egui::Ui, i: InstanceId) {
    let Some(inst) = app.doc.ents().instances.get(i).cloned() else { return };
    let model = &app.doc.model;
    let Some(def) = model.defs.get(inst.def).cloned() else { return };
    if model.image_of(app.doc.ctx_def(), i).is_some() {
        image_page(app, ui, i);
        return;
    }
    let units = model.units;
    let copies = model.instance_count(inst.def);
    let solid = solid_volume(ui, app, i, inst.def);
    let class = esq_core::classify::applied(model, inst.def).into_iter().map(|(_, t)| t).collect::<Vec<_>>().join(", ");
    if let Some(name) = field(ui, &format!("inst{i:?}"), "Instance :", &inst.name, "Saisir le nom de la copie") {
        app.doc.commit("Renommer", |m, d| {
            if let Some(x) = m.entities_mut(d).instances.get_mut(i) {
                x.name = name;
            }
        });
    }
    if !def.is_group {
        let d = inst.def;
        if let Some(name) = field(ui, &format!("def{d:?}"), "Définition:", &def.name, "Saisir le nom de la définition")
        {
            let name = name.trim().to_string();
            if !name.is_empty() {
                app.doc.commit("Renommer", |m, _| {
                    let taken: Vec<String> =
                        m.defs.iter().filter(|(k, _)| *k != d).map(|(_, x)| x.name.clone()).collect();
                    m.defs[d].name = Model::unique_name(&name, &taken);
                });
            }
        }
        ui.label(format!("Copies: {copies}"));
    }
    if !class.is_empty() {
        ui.label(format!("Classe: {class}"));
    }
    if let Some(v) = solid {
        ui.label(format!("Volume : {}", esq_core::units::format_volume(v, units)));
    }
    if def.is_group {
        return;
    }
    // Advanced attributes: the definition's price, size and URL, the copy's owner and status
    // (dictionaries `SU_DefinitionSet` and `SU_InstanceSet`, as in a .skp).
    egui::CollapsingHeader::new("Attributs avancés :").id_salt("ei_advanced").show(ui, |ui| {
        let d = inst.def;
        let rows: [(&str, &str, &str, bool); 5] = [
            ("Prix :", "Price", "Saisir le prix de la définition", true),
            ("Taille:", "Size", "Saisir la taille de la définition", true),
            ("URL:", "Url", "Saisir l'URL de la définition", true),
            ("Propriétaire :", "Owner", "Saisir le propriétaire de la copie", false),
            ("État:", "Status", "Saisir l'état de la copie", false),
        ];
        for (label, key, hint, of_def) in rows {
            let (dict, attrs) =
                if of_def { ("SU_DefinitionSet", &def.attributes) } else { ("SU_InstanceSet", &inst.attributes) };
            let now = text_attr(attrs, dict, key);
            if let Some(v) = field(ui, &format!("adv{key}{i:?}"), label, &now, hint) {
                let value = match (key, v.trim().replace(',', ".").parse::<f64>()) {
                    ("Price", Ok(n)) => AttrValue::Number(n),
                    _ => AttrValue::Text(v.trim().to_string()),
                };
                app.doc.commit("Propriétés", |m, ctx| {
                    let a = if of_def {
                        &mut m.defs[d].attributes
                    } else {
                        match m.entities_mut(ctx).instances.get_mut(i) {
                            Some(x) => &mut x.attributes,
                            None => return,
                        }
                    };
                    a.entry(dict.to_string()).or_default().insert(key.to_string(), value);
                });
            }
        }
    });
}

/// The page of an image (`CImagePP`): its file and size in pixels, Width and Height typed
/// like SketchUp (more than 0, else « La largeur de l’image
/// doit être supérieure à 0. »), and « Verrouiller les proportions »: ticked
/// when the image has the picture's proportions, it makes the other side follow, and ticking
/// it gives back the proportions from the side typed last. One operation « Propriétés »:
/// the image keeps its first corner and its directions.
fn image_page(app: &mut EsquisseApp, ui: &mut egui::Ui, i: InstanceId) {
    let ctx = app.doc.ctx_def();
    let Some(v) = app.doc.model.image_of(ctx, i) else { return };
    let Some(nat) = app.doc.model.image_natural_size(v.def) else { return };
    let aspect = nat.x / nat.y;
    let units = app.doc.model.units;
    let s = uniform_scale(&app.doc.ctx_xform()).unwrap_or(1.0);
    let (w, h) = (v.width * s, v.height * s);
    // (version of the model, lock ticked, Width typed last)
    let key = egui::Id::unique(("ei_image_lock", i));
    let rev = app.doc.rev;
    let same = |a: f64, b: f64| (a - b).abs() * 1.0e12 <= a.abs().min(b.abs());
    let (mut locked, mut width_last) = match ui.data(|d| d.get_temp::<(u64, bool, bool)>(key)) {
        Some((r, l, wl)) if r == rev => (l, wl),
        Some((_, _, wl)) => (same(w / h, aspect), wl),
        None => (same(w / h, aspect), false),
    };
    let name = std::path::Path::new(&v.path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if !name.is_empty() {
        ui.label(name);
    }
    if v.px[0] > 0 && v.px[1] > 0 {
        ui.label(format!("{} x {} px", v.px[0], v.px[1]));
    }
    let width = field(ui, &format!("imgw{i:?}"), "Largeur :", &format_length(w, units), "");
    let height = field(ui, &format!("imgh{i:?}"), "Hauteur :", &format_length(h, units), "");
    let was = locked;
    ui.checkbox(&mut locked, "Verrouiller les proportions").on_hover_text("Verrouiller/déverrouiller les proportions");
    let mut size = None;
    let typed = |t: &str| esq_core::units::parse_length(t, units).filter(|x| *x > 0.0);
    if let Some(t) = width {
        match typed(&t) {
            Some(nw) => {
                width_last = true;
                size = Some((nw, if locked { nw / aspect } else { h }));
            }
            None => crate::warnings::warn("La largeur de l’image doit être supérieure à 0."),
        }
    }
    if let Some(t) = height {
        match typed(&t) {
            Some(nh) => {
                width_last = false;
                size = Some((if locked { nh * aspect } else { w }, nh));
            }
            None => crate::warnings::warn("La hauteur de l’image doit être supérieure à 0."),
        }
    }
    if locked && !was {
        size = Some(if width_last { (w, w / aspect) } else { (h * aspect, h) });
    }
    ui.data_mut(|d| d.insert_temp(key, (rev, locked, width_last)));
    if let Some((nw, nh)) = size
        && !(same(nw, w) && same(nh, h))
    {
        app.doc.commit("Propriétés", |m, _| {
            m.set_image_size(ctx, i, nw / s, nh / s);
        });
        let rev = app.doc.rev;
        ui.data_mut(|d| d.insert_temp(key, (rev, locked, width_last)));
    }
}

/// A text or a dimension: its text, and for a text with a leader, the arrow and the leader.
fn annotation(app: &mut EsquisseApp, ui: &mut egui::Ui, a: esq_core::model::AnnotId) {
    let Some(an) = app.doc.ents().annots.get(a).cloned() else { return };
    if let Some(text) = field(ui, &format!("annot{a:?}"), "Texte:", &an.text, "") {
        app.doc.commit("Modifier le texte", |m, d| {
            if let Some(x) = m.entities_mut(d).annots.get_mut(a) {
                x.text = text;
            }
        });
    }
    // « Police : » and « Changer de police... » (`CTextPP` and `CDimensionPP`):
    // the font of this text or dimension alone, one
    // operation « Propriétés ».
    let st = crate::annot::style_of(&app.doc.model, app.doc.ents(), a);
    let mut font = st.font.clone();
    ui.label("Police :");
    let target = crate::dialogs::mi_annot::FontTarget::Selected;
    if crate::dialogs::mi_annot::font_row(app, ui, target, &mut font, "de l'entité", "Changer de police...") {
        let arrow = st.arrow;
        app.doc.commit("Propriétés", |m, d| crate::annot::apply_style(m.entities_mut(d), a, &font, arrow, &[]));
    }
    if !matches!(an.kind, AnnotKind::Text { .. }) {
        return;
    }
    use crate::context_menu::CtxCmd;
    let key = crate::annot::prop_key(a);
    let shown = !matches!(app.doc.ents().prop(&key, "display_leader"), Some(AttrValue::Bool(false)));
    let mut arrow = st.arrow;
    let arrows: Vec<(u8, String)> =
        [0u8, 2, 3, 4].iter().map(|k| (*k, CtxCmd::Arrow(*k).label().to_string())).collect();
    let mut leader = if shown { st.leader_type } else { 0 };
    let leaders: Vec<(u8, String)> = [1u8, 2, 0].iter().map(|k| (*k, CtxCmd::Leader(*k).label().to_string())).collect();
    let mut run = None;
    ui.horizontal(|ui| {
        ui.label("Flèche :");
        if crate::dialogs::combo(ui, "Flèche :", &mut arrow, &arrows) {
            run = Some(CtxCmd::Arrow(arrow));
        }
    });
    ui.horizontal(|ui| {
        ui.label("Légende :");
        if crate::dialogs::combo(ui, "Légende :", &mut leader, &leaders) {
            run = Some(CtxCmd::Leader(leader));
        }
    });
    if let Some(c) = run {
        let ctx = ui.ctx().clone();
        app.run(&ctx, crate::commands::Cmd::Ctx(c));
    }
}

fn section(app: &mut EsquisseApp, ui: &mut egui::Ui, s: esq_core::model::SectionId) {
    let Some(sp) = app.doc.ents().sections.get(s).cloned() else { return };
    let name = field(ui, &format!("secname{s:?}"), "Nom :", &sp.name, "");
    let symbol = field(ui, &format!("secsym{s:?}"), "Symbole :", &sp.symbol, "");
    if name.is_some() || symbol.is_some() {
        app.doc.commit("Propriétés du plan de section", |m, d| {
            let x = &mut m.entities_mut(d).sections[s];
            if let Some(n) = name {
                x.name = n;
            }
            if let Some(n) = symbol {
                x.symbol = n;
            }
        });
    }
    let mut active = app.doc.ents().active_section == Some(s);
    if ui.checkbox(&mut active, "Coupe active").changed() {
        app.doc.commit("Activer la coupe", |m, d| {
            m.entities_mut(d).active_section = if active { Some(s) } else { None };
        });
    }
}

fn space(app: &mut EsquisseApp, ui: &mut egui::Ui, id: u32) {
    let Some(sp) = app.doc.ents().space(id) else { return };
    let old = sp.name.clone();
    if let Some(name) = field(ui, &format!("space{id}"), "Nom :", &old, "") {
        app.doc.commit("Nom de l'espace", |m, d| {
            if let Some(s) = m.entities_mut(d).space_mut(id) {
                s.name = name;
            }
        });
    }
}

/// "Activer/désactiver :": hidden, locked (groups, components, images), cast and receive
/// shadows (faces, groups, components, images); each one an undoable operation named as in
/// SketchUp.
fn toggles(app: &mut EsquisseApp, ui: &mut egui::Ui, sel: &[EntityRef], locked: bool) {
    let has = sel
        .iter()
        .map(|e| toggles_of(*e))
        .fold((true, true, true, true), |a, b| (a.0 && b.0, a.1 && b.1, a.2 && b.2, a.3 && b.3));
    let ents = app.doc.ents();
    let mut hidden = sel.iter().all(|e| ents.is_hidden(*e));
    let mut lock = locked;
    let mut cast = sel.iter().all(|e| shadow_flag(app, *e, "casts"));
    let mut receive = sel.iter().all(|e| shadow_flag(app, *e, "receives"));
    let before = (hidden, lock, cast, receive);
    ui.label("Activer/désactiver :");
    ui.horizontal_wrapped(|ui| {
        ui.add_enabled_ui(!locked, |ui| {
            if has.0 {
                ui.checkbox(&mut hidden, "Caché");
            }
        });
        if has.1 {
            ui.checkbox(&mut lock, "Verrouillé");
        }
        ui.add_enabled_ui(!locked, |ui| {
            if has.2 {
                ui.checkbox(&mut cast, "Projeter des ombres");
            }
            if has.3 {
                ui.checkbox(&mut receive, "Recevoir des ombres");
            }
        });
    });
    if locked {
        ui.label(RichText::new("Verrouillé").color(Color32::from_rgb(190, 40, 40)));
    }
    let items = sel.to_vec();
    if hidden != before.0 {
        app.doc.commit(if hidden { "Masquer" } else { "Révéler" }, |m, d| {
            let e = m.entities_mut(d);
            for it in &items {
                e.set_hidden(*it, hidden);
            }
        });
    }
    if lock != before.1 {
        app.set_locked(lock, false);
    }
    for (name, on, was, label) in [
        ("casts", cast, before.2, if cast { "Projeter des ombres" } else { "Ne pas projeter d'ombres" }),
        ("receives", receive, before.3, if receive { "Recevoir des ombres" } else { "Ne pas recevoir d'ombres" }),
    ] {
        if on == was {
            continue;
        }
        app.doc.commit(label, |m, d| {
            let e = m.entities_mut(d);
            for it in &items {
                if let Some(key) = prop_key(*it) {
                    if on {
                        e.remove_prop(&key, name);
                    } else {
                        e.set_prop(&key, name, AttrValue::Bool(false));
                    }
                }
            }
        });
    }
}
