//! The right-click menu of the 3D view, built the way SketchUp.exe builds it:
//! what is right-clicked replaces the selection unless it is part
//! of it, then the menu follows the kind of entity — or the selection when
//! it holds several. Each menu starts with Entity Info, Erase, Hide and Lock;
//! inside an open group they end with Add Snaps; the Ruby extensions add
//! their entries last. An active tool with a menu of its own (a Ruby tool's `getMenu`,
//! Position Texture) keeps it.
//!
//! The menu is built once when it opens ([`build`], a list of [`Entry`] that the tests read)
//! and drawn by [`show`]. Shift+F10 and the Menu key open it from the keyboard, on what the
//! pointer is over, else on the selection.

use std::collections::HashSet;

use eframe::egui;
use esq_core::glam::{DMat4, DVec2, DVec3};
use esq_core::instance_ops::{is_scaled, is_skewed};
use esq_core::intersect::IntersectWith;
use esq_core::model::{
    AnnotKind, AttrValue, DefId, EdgeId, Entities, EntityRef, FaceId, Guide, InstanceId, LayerIdx, MaterialId, Model,
};

use crate::app::EsquisseApp;
use crate::commands::{Cmd, Panel};
use crate::edit_ops::Op;
use crate::texturing::TexCmd;
use crate::tools::ToolId;
use crate::view_cmds::ViewCmd;

/// Commands that only the right-click menu offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CtxCmd {
    /// Entity Info, Soften/Smooth Edges: shows the panel (the Window menu toggles it).
    ShowPanel(Panel),
    SelectBounding,
    SelectConnectedFaces,
    SelectAllConnected,
    SelectSameTag,
    SelectSameMaterial,
    SelectAllInstances,
    /// Soften (true) or Unsoften the selected edges.
    Soften(bool),
    ExplodeCurve,
    ConvertToPolygon,
    FindCenter,
    AreaTag,
    AreaMaterial,
    AlignViewFace,
    AlignAxesFace,
    /// A group's Make Component.
    GroupToComponent,
    MakeUnique,
    Unglue,
    ResetScale,
    ResetSkew,
    ScaleDefinition,
    ChangeAxes,
    /// Save Out › To Device...
    SaveOut,
    /// Reload › From Device...
    ReloadComponent,
    ExportImage,
    ReloadImage,
    UseAsMaterial,
    /// Shadows › Cast (true) / Receive of an image.
    Shadows(bool),
    EditTextureImage,
    EditText,
    /// Dimension › Text Position: 0 outside start, 1 centred, 2 outside end (Ruby's
    /// `DimensionLinear#text_position`).
    TextPosition(u8),
    /// Radial dimension › Type: radius (false) or diameter.
    Diameter(bool),
    /// Text › Arrow: Ruby's `arrow_type` (0 none, 2 dot, 3 closed, 4 open).
    Arrow(u8),
    /// Text › Leader: 1 view based, 2 pushpin, 0 hidden.
    Leader(u8),
    /// Outer Shell of the solids a group or component holds.
    OuterShellInside,
    /// The model's axes › Reset.
    ResetAxes,
    /// The model's axes › Align View: the view from above their red-green plane
    AlignViewAxes,
    /// Select › Connected with Snaps.
    ConnectedSnaps,
    /// A section plane's Troubleshoot Section Fill.
    TroubleshootFill,
    /// The matched photo's Project Photo.
    ProjectPhoto,
    /// The model's axes › Move: the Move Sketching Context dialog (`CMoveSketchCSDlg`).
    MoveAxes,
}

impl CtxCmd {
    pub fn label(self) -> &'static str {
        use CtxCmd as C;
        match self {
            C::ShowPanel(Panel::EntityInfo) => "Infos sur l'entité",
            C::ShowPanel(Panel::Soften) => "Adoucir/Lisser les arêtes",
            C::ShowPanel(p) => p.label(),
            C::SelectBounding => "Arêtes englobantes",
            C::SelectConnectedFaces => "Faces connectées",
            C::SelectAllConnected => "Tous les éléments connectés",
            C::SelectSameTag => "Tous les éléments avec la même étiquette",
            C::SelectSameMaterial => "Tous avec le même matériel",
            C::SelectAllInstances => "Toutes les copies",
            C::Soften(true) => "Adoucir",
            C::Soften(false) => "Supprimer l’adoucissement",
            C::ExplodeCurve => "Éclater la courbe",
            C::ConvertToPolygon => "Convertir en polygone",
            C::FindCenter => "Rechercher le centre",
            C::AreaTag => "Étiquette",
            C::AreaMaterial => "Matière",
            C::AlignViewFace => "Aligner la vue",
            C::AlignAxesFace => "Aligner les axes",
            C::GroupToComponent => "Créer composant...",
            C::MakeUnique => "Rendre unique",
            C::Unglue => "Détacher",
            C::ResetScale => "Réinitialiser l'échelle",
            C::ResetSkew => "Réinitialiser l'inclinaison",
            C::ScaleDefinition => "Mettre la définition à l'échelle",
            C::ChangeAxes => "Modifier les axes",
            C::SaveOut | C::ReloadComponent => "Localement...",
            C::ExportImage => "Exporter",
            C::ReloadImage => "Recharger...",
            C::UseAsMaterial => "Utiliser comme matière",
            C::Shadows(true) => "Projeter",
            C::Shadows(false) => "Recevoir",
            C::EditTextureImage => "Modifier l'image de texture...",
            C::EditText => "Modifier le texte",
            C::TextPosition(0) => "Extérieur au début",
            C::TextPosition(1) => "Centré",
            C::TextPosition(_) => "Extérieur à la fin",
            C::Diameter(false) => "Rayon",
            C::Diameter(true) => "Diamètre",
            C::Arrow(0) => "Aucune",
            C::Arrow(2) => "Point",
            C::Arrow(3) => "Fermée",
            C::Arrow(_) => "Ouverte",
            C::Leader(1) => "Suivant la vue",
            C::Leader(2) => "Punaise",
            C::Leader(_) => "Masqué",
            C::OuterShellInside => "Enveloppe externe",
            C::ResetAxes => "Réinitialiser",
            C::AlignViewAxes => "Aligner la vue",
            C::ConnectedSnaps => "Connecté avec Snaps",
            C::TroubleshootFill => "Résoudre les problèmes de remplissage de la coupe",
            C::ProjectPhoto => "Projeter la photo",
            C::MoveAxes => "Déplacer",
        }
    }
}

// ---------------------------------------------------------------------------------------
// The menu as data

/// One entry of the menu.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub label: String,
    pub cmd: Cmd,
    pub enabled: bool,
    /// `Some` for the entries shown with a tick when on.
    pub checked: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    Item(Item),
    Sep,
    Sub(String, Vec<Entry>),
    /// The entries of the Ruby extensions (`UI.add_context_menu_handler`).
    Plugins,
}

/// What was right-clicked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Nothing, at the top level.
    Nothing,
    /// Nothing, inside an open group or component: that one.
    Open,
    /// The model's axes (`CSketchCS`).
    Axes,
    Entity(EntityRef),
}

/// The open menu.
#[derive(Default)]
pub struct State {
    pub entries: Vec<Entry>,
    pub target: Option<Target>,
    /// Where the menu opens (screen points).
    pub at: egui::Pos2,
}

#[cfg(test)]
/// The labels in order, a submenu's entries after it ("Sélectionner › Faces connectées"),
/// separators as "---" and the extensions' entries as "<extensions>".
pub fn labels(entries: &[Entry]) -> Vec<String> {
    fn rec(entries: &[Entry], path: &str, out: &mut Vec<String>) {
        for e in entries {
            let s = match e {
                Entry::Item(i) => i.label.clone(),
                Entry::Sep => "---".into(),
                Entry::Plugins => "<extensions>".into(),
                Entry::Sub(l, _) => format!("{l} ›"),
            };
            out.push(format!("{path}{s}"));
            if let Entry::Sub(l, sub) = e {
                rec(sub, &format!("{path}{l} › "), out);
            }
        }
    }
    let mut out = Vec::new();
    rec(entries, "", &mut out);
    out
}

#[cfg(test)]
/// The entry at `path` ("Texture › Projetée").
pub fn find<'a>(entries: &'a [Entry], path: &str) -> Option<&'a Item> {
    let (head, rest) = match path.split_once(" › ") {
        Some((h, r)) => (h, Some(r)),
        None => (path, None),
    };
    entries.iter().find_map(|e| match (e, rest) {
        (Entry::Item(i), None) if i.label == head => Some(i),
        (Entry::Sub(l, sub), Some(r)) if l == head => find(sub, r),
        _ => None,
    })
}

#[cfg(test)]
/// Greyed entries (and the ones not in Esquisse yet), by path.
pub fn greyed(entries: &[Entry]) -> Vec<String> {
    fn rec(entries: &[Entry], path: &str, out: &mut Vec<String>) {
        for e in entries {
            match e {
                Entry::Item(i) if !i.enabled => out.push(format!("{path}{}", i.label)),
                Entry::Sub(l, sub) => rec(sub, &format!("{path}{l} › "), out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    rec(entries, "", &mut out);
    out
}

/// Qt hides separators at the start or end of a menu and runs of them; the extensions'
/// entries bring their own.
fn tidy(entries: Vec<Entry>) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for e in entries {
        match e {
            Entry::Sep if out.is_empty() || matches!(out.last(), Some(Entry::Sep)) => {}
            Entry::Plugins => {
                while matches!(out.last(), Some(Entry::Sep)) {
                    out.pop();
                }
                out.push(Entry::Plugins);
            }
            Entry::Sub(l, sub) => out.push(Entry::Sub(l, tidy(sub))),
            e => out.push(e),
        }
    }
    while matches!(out.last(), Some(Entry::Sep)) {
        out.pop();
    }
    out
}

// ---------------------------------------------------------------------------------------
// What the selection holds

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Group,
    Component,
    Image,
}

fn kind_of(m: &Model, ents: &Entities, i: InstanceId) -> Option<Kind> {
    let inst = ents.instances.get(i)?;
    let d = m.defs.get(inst.def)?;
    Some(if d.ext.image.is_some() {
        Kind::Image
    } else if d.is_group {
        Kind::Group
    } else {
        Kind::Component
    })
}

fn sel_edges(sel: &[EntityRef]) -> Vec<EdgeId> {
    sel.iter().filter_map(|e| if let EntityRef::Edge(x) = e { Some(*x) } else { None }).collect()
}

fn sel_faces(sel: &[EntityRef]) -> Vec<FaceId> {
    sel.iter().filter_map(|e| if let EntityRef::Face(x) = e { Some(*x) } else { None }).collect()
}

fn sel_insts(sel: &[EntityRef]) -> Vec<InstanceId> {
    sel.iter().filter_map(|e| if let EntityRef::Instance(x) = e { Some(*x) } else { None }).collect()
}

/// the selection is one entity — one element, all the edges of one curve,
/// or a face with exactly its edges (a double-click).
pub fn is_single(ents: &Entities, sel: &[EntityRef]) -> bool {
    match sel.len() {
        0 => false,
        1 => true,
        n => {
            let edges = sel_edges(sel);
            let faces = sel_faces(sel);
            if edges.len() == n {
                let c = ents.edges.get(edges[0]).and_then(|e| e.curve);
                return c.is_some_and(|c| {
                    edges.iter().all(|e| ents.edges.get(*e).is_some_and(|x| x.curve == Some(c)))
                        && esq_core::curves::curve_edges(ents, c).len() == n
                });
            }
            if let [f] = faces.as_slice()
                && edges.len() + 1 == n
            {
                let own: HashSet<EdgeId> = ents.face_edges(*f).into_iter().collect();
                return own.len() == edges.len() && edges.iter().all(|e| own.contains(e));
            }
            false
        }
    }
}

fn curve_key(c: u32) -> String {
    format!("c{c}")
}

fn is_polygon(ents: &Entities, c: u32) -> bool {
    matches!(ents.prop(&curve_key(c), "kind"), Some(AttrValue::Text(t)) if t == "polygon")
}

fn is_arc(ents: &Entities, c: u32) -> bool {
    !matches!(ents.prop(&curve_key(c), "kind"), Some(AttrValue::Text(t)) if t == "curve")
        && esq_core::curves::arc_fit(ents, c).is_some()
}

fn sel_curves(ents: &Entities, sel: &[EntityRef]) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    for e in sel_edges(sel) {
        if let Some(c) = ents.edges.get(e).and_then(|x| x.curve)
            && !out.contains(&c)
        {
            out.push(c);
        }
    }
    out
}

fn inst_key(i: InstanceId) -> String {
    format!("i{}", slotmap::Key::data(&i).as_ffi())
}

fn flag(ents: &Entities, key: &str, name: &str, default: bool) -> bool {
    match ents.prop(key, name) {
        Some(AttrValue::Bool(b)) => *b,
        Some(AttrValue::Json(j)) => j.as_bool().unwrap_or(default),
        _ => default,
    }
}

fn num(ents: &Entities, key: &str, name: &str) -> Option<f64> {
    match ents.prop(key, name)? {
        AttrValue::Number(n) => Some(*n),
        AttrValue::Json(j) => j.as_f64(),
        _ => None,
    }
}

/// Facts about the selection the menus test.
struct Sel<'a> {
    app: &'a EsquisseApp,
    ents: &'a Entities,
    sel: &'a [EntityRef],
}

impl<'a> Sel<'a> {
    fn new(app: &'a EsquisseApp) -> Self {
        Sel { app, ents: app.doc.ents(), sel: &app.doc.sel }
    }

    fn kind(&self, i: InstanceId) -> Option<Kind> {
        kind_of(&self.app.doc.model, self.ents, i)
    }

    fn locked(&self, i: InstanceId) -> bool {
        self.ents.instances.get(i).is_some_and(|x| x.locked)
    }

    /// everything selected is locked.
    fn all_locked(&self) -> bool {
        !self.sel.is_empty() && self.sel.iter().all(|e| matches!(e, EntityRef::Instance(i) if self.locked(*i)))
    }

    fn any_hidden(&self) -> bool {
        self.sel.iter().any(|e| self.ents.is_hidden(*e))
    }

    /// Groups and components (not images), for Lock / Unlock.
    fn lockables(&self) -> Vec<InstanceId> {
        sel_insts(self.sel)
            .into_iter()
            .filter(|i| matches!(self.kind(*i), Some(Kind::Group | Kind::Component)))
            .collect()
    }

    /// Unlocked groups, components and images.
    fn unlocked_insts(&self) -> Vec<InstanceId> {
        sel_insts(self.sel).into_iter().filter(|i| !self.locked(*i)).collect()
    }

    fn any_inst(&self, f: impl Fn(&esq_core::model::Instance) -> bool) -> bool {
        self.unlocked_insts().iter().any(|i| f(&self.ents.instances[*i]))
    }

    fn explodable(&self) -> bool {
        !self.unlocked_insts().is_empty()
    }

    /// Make Component is offered, except for a component alone.
    fn can_component(&self) -> bool {
        !self.sel.is_empty() && !matches!(self.sel, [EntityRef::Instance(i)] if self.kind(*i) == Some(Kind::Component))
    }

    /// Make Group, except for a group or a component alone.
    fn can_group(&self) -> bool {
        !self.sel.is_empty()
            && !matches!(self.sel, [EntityRef::Instance(i)] if matches!(self.kind(*i), Some(Kind::Group | Kind::Component)))
    }

    /// the selected components whose definition has copies left outside
    /// the selection.
    fn uniquable(&self) -> Vec<DefId> {
        let m = &self.app.doc.model;
        let mut defs: Vec<DefId> = Vec::new();
        for i in self.unlocked_insts() {
            if self.kind(i) != Some(Kind::Component) {
                continue;
            }
            let d = self.ents.instances[i].def;
            let selected = sel_insts(self.sel).iter().filter(|x| self.ents.instances[**x].def == d).count();
            if m.instance_count(d) > selected && !defs.contains(&d) {
                defs.push(d);
            }
        }
        defs
    }

    /// an edge between two faces, or an instance holding some.
    fn softenable(&self) -> bool {
        if self.sel.is_empty() || self.all_locked() {
            return false;
        }
        let two_faces = |e: &Entities| e.edges.values().any(|x| x.faces.len() == 2);
        sel_edges(self.sel).iter().any(|e| self.ents.edges.get(*e).is_some_and(|x| x.faces.len() == 2))
            || self.unlocked_insts().iter().any(|i| {
                self.kind(*i) != Some(Kind::Image)
                    && two_faces(&self.app.doc.model.defs[self.ents.instances[*i].def].entities)
            })
    }

    /// how many of the selected instances are solids, if all are.
    fn solids(&self) -> usize {
        let insts = sel_insts(self.sel);
        if insts.len() != self.sel.len() {
            return 0;
        }
        let ctx = self.app.doc.ctx_def();
        let n = insts.iter().filter(|i| self.app.doc.model.is_solid_instance(ctx, **i)).count();
        if n == insts.len() { n } else { 0 }
    }

    /// The same for the instances a group or component holds (Outer Shell of its content).
    fn solids_inside(&self, i: InstanceId) -> usize {
        let m = &self.app.doc.model;
        let Some(d) = self.ents.instances.get(i).map(|x| x.def) else { return 0 };
        let kids: Vec<InstanceId> = m.defs[d].entities.instances.keys().collect();
        let n = kids.iter().filter(|k| m.is_solid_instance(Some(d), **k)).count();
        if n == kids.len() { n } else { 0 }
    }
}

// ---------------------------------------------------------------------------------------
// Building the menu

struct B<'a> {
    app: &'a EsquisseApp,
    out: Vec<Entry>,
}

impl<'a> B<'a> {
    fn item(&mut self, label: &str, cmd: Cmd, enabled: bool) {
        self.out.push(Entry::Item(Item { label: label.into(), cmd, enabled, checked: None }));
    }

    fn check(&mut self, label: &str, cmd: Cmd, enabled: bool, on: bool) {
        self.out.push(Entry::Item(Item { label: label.into(), cmd, enabled, checked: Some(on) }));
    }

    fn ctx(&mut self, c: CtxCmd, enabled: bool) {
        self.item(c.label(), Cmd::Ctx(c), enabled);
    }

    fn sep(&mut self) {
        self.out.push(Entry::Sep);
    }

    fn sub(&mut self, label: &str, f: impl FnOnce(&mut B<'a>)) {
        let mut b = B { app: self.app, out: Vec::new() };
        f(&mut b);
        self.out.push(Entry::Sub(label.into(), b.out));
    }

    /// Entity Info, Erase, Hide or Unhide, Lock or Unlock.
    fn header(&mut self) {
        let s = Sel::new(self.app);
        let locked = s.all_locked();
        self.ctx(CtxCmd::ShowPanel(Panel::EntityInfo), true);
        self.item("Effacer", Cmd::Delete, !locked);
        if s.any_hidden() {
            self.item("Révéler", Cmd::RevealSelection, !locked);
        } else {
            self.item("Masquer", Cmd::Hide, !locked);
        }
        let lockables = s.lockables();
        if !lockables.is_empty() {
            if lockables.iter().any(|i| s.locked(*i)) {
                self.item("Déverrouiller", Cmd::UnlockSelection, true);
            } else {
                self.item("Verrouiller", Cmd::Lock, true);
            }
        }
        self.sep();
    }

    fn select_all_or_invert(&mut self) {
        if self.app.doc.sel.is_empty() {
            self.item("Tout sélectionner", Cmd::SelectAll, true);
        } else {
            self.item("Inverser la sélection", Cmd::InvertSelection, true);
        }
    }

    /// the Select submenu with the entries `on` lists.
    fn select_sub(&mut self, on: [bool; 10]) {
        let one_inst = sel_insts(&self.app.doc.sel).len() == 1;
        self.sub("Sélectionner", |b| {
            let cmds = [
                CtxCmd::SelectBounding,
                CtxCmd::SelectConnectedFaces,
                CtxCmd::SelectAllConnected,
                CtxCmd::SelectSameTag,
                CtxCmd::SelectSameMaterial,
                CtxCmd::SelectAllInstances,
            ];
            for (k, c) in cmds.into_iter().enumerate() {
                if on[k] {
                    b.ctx(c, true);
                }
            }
            if on[6] {
                b.item(Op::DeselectEdges.label(), Cmd::Op(Op::DeselectEdges), true);
            }
            if on[7] {
                b.item(Op::DeselectFaces.label(), Cmd::Op(Op::DeselectFaces), true);
            }
            if on[8] {
                b.select_all_or_invert();
            }
            if on[9] && one_inst {
                b.ctx(CtxCmd::ConnectedSnaps, true);
            }
        });
    }

    fn make_items(&mut self, s: &Sel) {
        if s.can_component() {
            self.item("Créer composant...", Cmd::MakeComponent, true);
        }
        if s.can_group() {
            self.item("Créer un groupe", Cmd::MakeGroup, true);
        }
    }

    fn zoom(&mut self) {
        self.item("Zoom sur la sélection", Cmd::ZoomSelection, !self.app.doc.sel.is_empty());
    }

    /// Intersect Faces › With Model, With Context (inside a group), With Selection.
    fn intersect(&mut self, with_selection: bool) {
        let inside = !self.app.doc.ctx.is_empty();
        self.sub("Intersection des faces", |b| {
            b.item("Avec le modèle", Cmd::Intersect(IntersectWith::Model), true);
            if inside {
                b.item("Avec le contexte", Cmd::Intersect(IntersectWith::Context), true);
            }
            if with_selection {
                b.item("Avec la sélection", Cmd::Intersect(IntersectWith::Selection), true);
            }
        });
    }

    fn area(&mut self) {
        self.sub("Surface", |b| {
            b.item("Sélection", Cmd::Op(Op::AreaSelection), true);
            b.ctx(CtxCmd::AreaTag, true);
            b.ctx(CtxCmd::AreaMaterial, true);
        });
    }

    fn soften_panel(&mut self, s: &Sel) {
        if s.softenable() {
            self.ctx(CtxCmd::ShowPanel(Panel::Soften), true);
        }
    }

    /// Weld Edges (several edges), then for curves Convert to Polygon, Find
    /// Center (arcs) and Explode Curve.
    fn curve_items(&mut self) {
        let s = Sel::new(self.app);
        if sel_edges(s.sel).len() > 1 {
            self.item(Op::WeldEdges.label(), Cmd::Op(Op::WeldEdges), true);
        }
        let curves = sel_curves(s.ents, s.sel);
        if curves.is_empty() {
            return;
        }
        if curves.iter().any(|c| !is_polygon(s.ents, *c)) {
            self.ctx(CtxCmd::ConvertToPolygon, true);
        }
        if curves.iter().any(|c| is_arc(s.ents, *c)) {
            self.ctx(CtxCmd::FindCenter, true);
        }
        self.ctx(CtxCmd::ExplodeCurve, true);
    }

    // ----------------------------------------------------------------------- the menus

    /// with nothing under the cursor.
    fn nothing(&mut self) {
        if self.app.photo_cmd_enabled() {
            // the matched photo's menu.
            self.item("Modifier la photo adaptée", Cmd::PhotoEdit, true);
            let faces = !sel_faces(&self.app.doc.sel).is_empty();
            self.ctx(CtxCmd::ProjectPhoto, faces);
            self.item("Zoom sur la photo adaptée", Cmd::PhotoZoom, true);
            return;
        }
        self.select_all_or_invert();
        self.sep();
        // Enabled whatever the selection (the menu's flags are 0).
        self.item("Créer composant...", Cmd::MakeComponent, true);
        self.item("Créer un groupe", Cmd::MakeGroup, true);
        // (feature "ModelingGrid").
        self.sep();
        if self.app.cmd_checked(Cmd::View(ViewCmd::Grid)) == Some(true) {
            let on = self.app.cmd_checked(Cmd::View(ViewCmd::GridSnapping)) == Some(true);
            self.check("Accrochage à la grille", Cmd::View(ViewCmd::GridSnapping), true, on);
        }
        // Both start the Axes tool (→ `CSketchCSTool`).
        self.item("Principaux intervalles entre les grilles", Cmd::Tool(ToolId::AxesTool), true);
        self.item("Espacement des grilles mineures", Cmd::Tool(ToolId::AxesTool), true);
    }

    /// The open group or component, right-clicked in the void (no header).
    fn open(&mut self) {
        let s = Sel::new(self.app);
        let is_group = self.app.doc.ctx_def().is_some_and(|d| self.app.doc.model.defs[d].is_group);
        self.item(if is_group { "Fermer le groupe" } else { "Fermer le composant" }, Cmd::CloseGroup, true);
        self.sep();
        self.item("Créer composant...", Cmd::MakeComponent, s.can_component());
        self.item("Créer un groupe", Cmd::MakeGroup, s.can_group());
        if !is_group {
            self.sep();
            self.ctx(CtxCmd::ChangeAxes, true);
            self.ctx(CtxCmd::ShowPanel(Panel::EntityInfo), true);
        }
    }

    fn edge(&mut self, e: EdgeId) {
        let s = Sel::new(self.app);
        self.header();
        self.select_sub([false, true, true, true, false, false, true, false, true, false]);
        self.make_items(&s);
        let edge = &s.ents.edges[e];
        self.ctx(CtxCmd::Soften(!edge.soft), true);
        if is_single(s.ents, s.sel) && crate::edit_ops::can_divide(&self.app.doc) {
            self.item("Diviser", Cmd::Op(Op::Divide), true);
        }
        // (not the multiple selection's order).
        if let Some(c) = edge.curve {
            self.ctx(CtxCmd::ExplodeCurve, true);
            if !is_polygon(s.ents, c) {
                self.ctx(CtxCmd::ConvertToPolygon, true);
            }
            if sel_curves(s.ents, s.sel).iter().any(|c| is_arc(s.ents, *c)) {
                self.ctx(CtxCmd::FindCenter, true);
            }
        }
        self.zoom();
    }

    fn face(&mut self) {
        let s = Sel::new(self.app);
        self.header();
        self.select_sub([true, true, true, true, true, false, false, true, true, false]);
        self.make_items(&s);
        self.area();
        self.intersect(sel_faces(s.sel).len() > 1);
        self.ctx(CtxCmd::AlignViewFace, true);
        self.ctx(CtxCmd::AlignAxesFace, true);
        self.item("Inverser les faces", Cmd::ReverseFaces, true);
        self.item("Orienter les faces", Cmd::Op(Op::OrientFaces), true);
        self.zoom();
        let app = self.app;
        if crate::texturing::one_face(&app.doc, &app.cam, &app.style).is_some() {
            self.sub("Texture", |b| {
                for (c, label) in [(TexCmd::Position, "Position"), (TexCmd::Reset, "Réinitialiser la position")] {
                    b.item(label, Cmd::Tex(c), crate::texturing::enabled(app, c));
                }
                b.sep();
                let on = crate::texturing::checked(app, TexCmd::Project) == Some(true);
                b.check("Projetée", Cmd::Tex(TexCmd::Project), true, on);
                b.ctx(CtxCmd::EditTextureImage, true);
            });
        }
        let distorted = sel_faces(s.sel).iter().any(|f| {
            s.ents
                .faces
                .get(*f)
                .and_then(|x| x.tex.as_ref())
                .is_some_and(|t| t.front.or(t.back).is_some_and(|m| !m.is_affine()))
        });
        if distorted {
            self.sep();
            self.item(
                TexCmd::MakeUnique.label(),
                Cmd::Tex(TexCmd::MakeUnique),
                crate::texturing::enabled(app, TexCmd::MakeUnique),
            );
        }
    }

    /// Unglue, Reset Scale, Reset Skew of groups and components.
    fn transform_items(&mut self, s: &Sel, change_axes: bool) {
        self.ctx(CtxCmd::Unglue, s.any_inst(|i| i.ext.glued_to.is_some()));
        if change_axes {
            let locked = sel_insts(s.sel).first().is_some_and(|i| s.locked(*i));
            self.ctx(CtxCmd::ChangeAxes, !locked);
        }
        self.ctx(CtxCmd::ResetScale, s.any_inst(|i| is_scaled(&i.xform)));
        self.ctx(CtxCmd::ResetSkew, s.any_inst(|i| is_skewed(&i.xform)));
        if change_axes {
            self.ctx(CtxCmd::ScaleDefinition, s.any_inst(|i| is_scaled(&i.xform)));
        }
    }

    /// Intersect Faces (no With Selection) and Outer Shell of the content.
    fn intersect_shell(&mut self, s: &Sel, i: InstanceId) {
        self.intersect(false);
        if s.solids_inside(i) > 1 {
            self.ctx(CtxCmd::OuterShellInside, true);
        }
    }

    fn group(&mut self, i: InstanceId) {
        let s = Sel::new(self.app);
        let locked = s.locked(i);
        self.header();
        self.item("Modifier le groupe", Cmd::EditGroup, !locked);
        self.item("Éclater", Cmd::Explode, s.explodable());
        self.ctx(CtxCmd::GroupToComponent, !locked);
        self.sep();
        self.select_sub([false, false, false, true, false, false, false, false, true, true]);
        self.transform_items(&s, false);
        self.sep();
        self.intersect_shell(&s, i);
        self.soften_panel(&s);
        self.zoom();
    }

    fn component(&mut self, i: InstanceId) {
        let s = Sel::new(self.app);
        let locked = s.locked(i);
        self.header();
        let one = matches!(s.sel, [EntityRef::Instance(x)] if s.kind(*x) == Some(Kind::Component) && !s.locked(*x));
        self.item("Modifier le composant", Cmd::EditGroup, one);
        self.ctx(CtxCmd::MakeUnique, !s.uniquable().is_empty());
        self.item("Éclater", Cmd::Explode, s.explodable());
        // (Trimble Connect and 3D Warehouse left out).
        self.sep();
        if s.sel.len() < 2 {
            self.sub("Sauvegarder", |b| b.ctx(CtxCmd::SaveOut, true));
            self.sub("Recharger", |b| b.ctx(CtxCmd::ReloadComponent, !locked));
            self.sep();
        }
        self.select_sub([false, false, false, true, false, true, false, false, true, true]);
        self.transform_items(&s, true);
        self.sep();
        self.intersect_shell(&s, i);
        self.soften_panel(&s);
        self.zoom();
    }

    fn image(&mut self, i: InstanceId) {
        let s = Sel::new(self.app);
        let locked = s.locked(i);
        self.header();
        self.select_all_or_invert();
        self.sep();
        self.item("Créer composant...", Cmd::MakeComponent, true);
        self.item("Créer un groupe", Cmd::MakeGroup, true);
        self.item("Éclater", Cmd::Explode, s.explodable());
        self.ctx(CtxCmd::ExportImage, true);
        self.ctx(CtxCmd::ReloadImage, true);
        self.zoom();
        let key = inst_key(i);
        let (cast, receive) = (flag(s.ents, &key, "casts", true), flag(s.ents, &key, "receives", true));
        self.sub("Ombres", |b| {
            b.check(CtxCmd::Shadows(true).label(), Cmd::Ctx(CtxCmd::Shadows(true)), !locked, cast);
            b.check(CtxCmd::Shadows(false).label(), Cmd::Ctx(CtxCmd::Shadows(false)), !locked, receive);
        });
        self.ctx(CtxCmd::Unglue, s.any_inst(|x| x.ext.glued_to.is_some()));
        self.ctx(CtxCmd::UseAsMaterial, material_of_image(&self.app.doc.model, &s.ents.instances[i]).is_none());
    }

    fn section(&mut self) {
        self.header();
        self.select_all_or_invert();
        self.sep();
        self.item("Inverser", Cmd::ReverseSection, true);
        let on = self.app.cmd_checked(Cmd::ActiveCut) == Some(true);
        self.check("Activer la coupe", Cmd::ActiveCut, true, on);
        self.item("Aligner la vue", Cmd::AlignView, true);
        self.item("Créer une tranche de section groupée", Cmd::GroupFromSlice, slice_has_content(self.app));
        // Ticked when on, greyed while the style shows no section fill.
        let on = checked(self.app, CtxCmd::TroubleshootFill) == Some(true);
        let label = CtxCmd::TroubleshootFill.label();
        self.check(label, Cmd::Ctx(CtxCmd::TroubleshootFill), self.app.style.section_fill, on);
    }

    /// (linear and radial dimensions) and (texts).
    fn annot(&mut self, a: esq_core::model::AnnotId) {
        let ents = self.app.doc.ents();
        let Some(an) = ents.annots.get(a) else { return };
        let key = crate::annot::prop_key(a);
        self.header();
        self.select_all_or_invert();
        self.sep();
        self.ctx(CtxCmd::EditText, true);
        match an.kind {
            AnnotKind::Linear { .. } if ents.prop(&key, "radial").is_some() => {
                let dia = flag(ents, &key, "diameter", false);
                self.sub("Type", |b| {
                    for d in [false, true] {
                        b.check(CtxCmd::Diameter(d).label(), Cmd::Ctx(CtxCmd::Diameter(d)), true, dia == d);
                    }
                });
            }
            AnnotKind::Linear { .. } => {
                let pos = num(ents, &key, "text_position").map_or(1, |v| v as u8);
                self.sub("Position du texte", |b| {
                    for p in 0..3 {
                        b.check(CtxCmd::TextPosition(p).label(), Cmd::Ctx(CtxCmd::TextPosition(p)), true, pos == p);
                    }
                });
            }
            AnnotKind::Text { .. } => {
                let st = crate::annot::style_of(&self.app.doc.model, ents, a);
                let hidden = !flag(ents, &key, "display_leader", true);
                self.sub("Flèche", |b| {
                    for k in [0, 2, 3, 4] {
                        b.check(CtxCmd::Arrow(k).label(), Cmd::Ctx(CtxCmd::Arrow(k)), true, st.arrow == k);
                    }
                });
                self.sub("Légende", |b| {
                    for k in [1, 2] {
                        let on = !hidden && st.leader_type == k;
                        b.check(CtxCmd::Leader(k).label(), Cmd::Ctx(CtxCmd::Leader(k)), true, on);
                    }
                    b.check(CtxCmd::Leader(0).label(), Cmd::Ctx(CtxCmd::Leader(0)), true, hidden);
                });
            }
            // A text on the screen has no leader.
            AnnotKind::ScreenText { .. } => {}
        }
    }

    /// Guide lines and points (types 7 and 8).
    fn guide(&mut self) {
        self.header();
        self.select_all_or_invert();
        self.sep();
    }

    /// the model's axes (no header).
    fn axes(&mut self) {
        let placing = self.app.tool.id() == ToolId::AxesTool;
        self.check("Placer", Cmd::Tool(ToolId::AxesTool), true, placing);
        // "Move" opens SketchUp's Move Sketching Context dialog.
        self.ctx(CtxCmd::MoveAxes, true);
        self.ctx(CtxCmd::ResetAxes, self.app.doc.model.axes != DMat4::IDENTITY);
        self.ctx(CtxCmd::AlignViewAxes, true);
        self.item("Masquer", Cmd::Axes, true);
    }

    fn multi(&mut self, picked: EntityRef) {
        let s = Sel::new(self.app);
        self.header();
        self.item("Éclater", Cmd::Explode, s.explodable());
        let any_comp = sel_insts(s.sel).iter().any(|i| s.kind(*i) == Some(Kind::Component));
        let edges = sel_edges(s.sel);
        let faces = sel_faces(s.sel);
        self.select_sub([true, true, true, true, true, any_comp, !edges.is_empty(), !faces.is_empty(), true, false]);
        self.area();
        self.make_items(&s);
        if !s.uniquable().is_empty() {
            self.ctx(CtxCmd::MakeUnique, true);
        }
        self.intersect(true);
        let solids = s.solids();
        if solids > 1 {
            self.sub("Solides", |b| {
                b.item("Intersection", Cmd::Tool(ToolId::SolidIntersect), true);
                b.item("Union", Cmd::Tool(ToolId::SolidUnion), true);
                if solids == 2 {
                    b.item("Soustraire", Cmd::Tool(ToolId::SolidSubtract), true);
                    b.item("Découper", Cmd::Tool(ToolId::SolidTrim), true);
                    b.item("Diviser", Cmd::Tool(ToolId::SolidSplit), true);
                }
            });
            self.item("Enveloppe externe", Cmd::Tool(ToolId::OuterShell), true);
        }
        if matches!(picked, EntityRef::Face(_)) {
            self.ctx(CtxCmd::AlignViewFace, true);
        }
        if !faces.is_empty() {
            self.item("Inverser les faces", Cmd::ReverseFaces, true);
        }
        self.curve_items();
        self.soften_panel(&s);
        self.zoom();
    }

    /// Inside an open group: Add Snaps; then the extensions.
    fn tail(&mut self) {
        if !self.app.doc.ctx.is_empty() {
            self.sep();
            self.item("Ajouter des ancrages", Cmd::Tool(ToolId::AddSnap), true);
        }
        if self.app.ui_state.extra.ruby.has_context_items() {
            self.out.push(Entry::Plugins);
        }
    }
}

/// The menu for `target`, with the selection as it is now.
pub fn build(app: &EsquisseApp, target: Target) -> Vec<Entry> {
    let mut b = B { app, out: Vec::new() };
    match target {
        Target::Nothing => b.nothing(),
        Target::Open => {
            b.open();
            b.tail();
        }
        Target::Axes => {
            b.axes();
            b.tail();
        }
        Target::Entity(e) => {
            let s = Sel::new(app);
            if !is_single(s.ents, s.sel) {
                b.multi(e);
            } else {
                match e {
                    EntityRef::Edge(x) if s.ents.edges.contains_key(x) => b.edge(x),
                    EntityRef::Face(_) => b.face(),
                    EntityRef::Instance(i) => match s.kind(i) {
                        Some(Kind::Group) => b.group(i),
                        Some(Kind::Component) => b.component(i),
                        Some(Kind::Image) => b.image(i),
                        None => {}
                    },
                    EntityRef::Guide(_) => b.guide(),
                    EntityRef::Annot(a) => b.annot(a),
                    EntityRef::Section(_) => b.section(),
                    // Snaps and spaces became selectable (ruby-audit-2): their own entries
                    // (« Supprimer ancrage », « Réorienter l'ancrage »…) are for this menu's owner.
                    EntityRef::Edge(_) | EntityRef::Snap(_) | EntityRef::Space(_) => {}
                }
            }
            b.tail();
        }
    }
    tidy(b.out)
}

/// the right-clicked entity (in the active context) replaces the selection
/// unless it is part of it; nothing under the cursor empties it.
pub fn right_click(app: &mut EsquisseApp, hit: Option<EntityRef>) -> Target {
    match hit {
        Some(e) => {
            if !app.doc.sel.contains(&e) {
                app.doc.select(vec![e]);
            }
            Target::Entity(e)
        }
        None => {
            if !app.doc.sel.is_empty() {
                app.doc.select(Vec::new());
            }
            if app.doc.ctx.is_empty() { Target::Nothing } else { Target::Open }
        }
    }
}

/// What the right-click at `m` (view points) is on, in the active context.
pub fn hit_at(app: &EsquisseApp, m: DVec2) -> Option<EntityRef> {
    crate::tools::hit_under(&app.pick, &app.cam, m).and_then(|h| h.selectable(&app.doc.ctx))
}

/// The model's axes, shown, pass within a click's reach of `m` (at the top level).
pub fn axes_at(app: &EsquisseApp, m: DVec2) -> bool {
    if !app.style.axes || !app.doc.ctx.is_empty() {
        return false;
    }
    let x = app.doc.model.axes;
    let o = x.transform_point3(DVec3::ZERO);
    let (p0, u) = app.cam.ray(m);
    [DVec3::X, DVec3::Y, DVec3::Z].into_iter().any(|axis| {
        // The point of the axis nearest the ray under the cursor, back on the screen.
        let v = x.transform_vector3(axis).normalize_or_zero();
        let w0 = p0 - o;
        let (a, b, c, d, e) = (u.dot(u), u.dot(v), v.dot(v), u.dot(w0), v.dot(w0));
        let den = a * c - b * b;
        if den.abs() < 1e-12 || (b * e - c * d) / den <= 0.0 {
            return false;
        }
        let s = (a * e - b * d) / den;
        app.cam.project(o + v * s).is_some_and(|q| q.distance(m) < crate::pick::EDGE_PX)
    })
}

/// Opens the menu: `at` is where (view points); `pointer` is false when it comes from the
/// keyboard without the pointer over the view (the selection's menu then).
pub fn open(app: &mut EsquisseApp, at: DVec2, pointer: bool) {
    app.ensure_pick();
    app.context_point = pointer.then_some(at);
    app.ui_state.extra.ctx_menu.entries.clear();
    app.ui_state.extra.ctx_menu.target = None;
    // The active tool's own menu comes first (it does not change the selection).
    if app.tool.id() == ToolId::PositionTexture {
        return;
    }
    if app.tool.ruby().is_some() {
        crate::ruby_ui::collect_context_menu(app);
        if crate::ruby_tools::tool_menu_shown(app) {
            return;
        }
    }
    let target = if pointer {
        let hit = hit_at(app, at);
        // The axes come before faces, not before edges or instances; they
        // are not selected, the selection is emptied.
        if matches!(hit, None | Some(EntityRef::Face(_))) && axes_at(app, at) {
            right_click(app, None);
            Target::Axes
        } else {
            right_click(app, hit)
        }
    } else {
        match app.doc.sel.first() {
            Some(e) => Target::Entity(*e),
            None if app.doc.ctx.is_empty() => Target::Nothing,
            None => Target::Open,
        }
    };
    // The extensions' handlers see the new selection.
    crate::ruby_ui::collect_context_menu(app);
    let entries = build(app, target);
    let st = &mut app.ui_state.extra.ctx_menu;
    st.target = Some(target);
    st.entries = entries;
}

/// The viewport's right-click and Shift+F10 / Menu key; then the menu while it is open.
pub fn viewport(app: &mut EsquisseApp, ui: &mut egui::Ui, response: &egui::Response, local: Option<DVec2>) {
    let key = !ui.ctx().egui_wants_keyboard_input()
        && ui.input(|i| {
            i.key_pressed(egui::Key::Menu)
                || (i.modifiers.matches_exact(egui::Modifiers::SHIFT) && i.key_pressed(egui::Key::F10))
        });
    let clicked = response.secondary_clicked();
    if clicked || key {
        let rect = response.rect;
        let centre = DVec2::new(rect.width() as f64 * 0.5, rect.height() as f64 * 0.5);
        let m = local.unwrap_or(centre);
        open(app, m, local.is_some());
        app.ui_state.extra.ctx_menu.at = rect.min + egui::vec2(m.x as f32, m.y as f32);
    }
    let set = if clicked || key {
        Some(egui::SetOpenCommand::Bool(true))
    } else if response.clicked() {
        Some(egui::SetOpenCommand::Bool(false))
    } else {
        None
    };
    let at = app.ui_state.extra.ctx_menu.at;
    egui::Popup::menu(response).open_memory(set).at_position(at).show(|ui| app.context_menu(ui));
}

/// Draws the menu built by [`open`].
pub fn show(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let entries = app.ui_state.extra.ctx_menu.entries.clone();
    let mut run = None;
    draw(app, ui, &entries, &mut run);
    if let Some(cmd) = run {
        let ctx = ui.ctx().clone();
        app.run(&ctx, cmd);
    }
}

/// A label with room for a tick before it (shown when `tick` is `Some(true)`).
fn ticked(ui: &egui::Ui, label: &str, enabled: bool, tick: Option<bool>) -> egui::WidgetText {
    let Some(on) = tick else { return label.into() };
    let style = ui.style();
    let font = egui::TextStyle::Button.resolve(style);
    let color = if enabled { style.visuals.text_color() } else { style.visuals.weak_text_color() };
    let mut job = egui::text::LayoutJob::default();
    job.append("✔  ", 0.0, egui::TextFormat::simple(font.clone(), if on { color } else { egui::Color32::TRANSPARENT }));
    job.append(label, 0.0, egui::TextFormat::simple(font, color));
    job.into()
}

/// The name screen readers and UI Automation get: the label without the tick's room, and
/// the tick as a toggled state.
fn name(r: &egui::Response, label: &str, enabled: bool, checked: Option<bool>) {
    use egui::{Role, WidgetInfo};
    r.widget_info(|| match checked {
        Some(on) => WidgetInfo::selected(Role::Button, enabled, on, label),
        None => WidgetInfo::labeled(Role::Button, enabled, label),
    });
}

fn draw(app: &mut EsquisseApp, ui: &mut egui::Ui, entries: &[Entry], run: &mut Option<Cmd>) {
    // Every entry of SketchUp's menus is checkable: when one shows a tick, the others keep
    // its room so that the labels line up.
    let room = entries.iter().any(|e| matches!(e, Entry::Item(i) if i.checked.is_some())).then_some(false);
    for e in entries {
        match e {
            Entry::Item(it) => {
                let text = ticked(ui, &it.label, it.enabled, it.checked.or(room));
                let button = egui::Button::new(text).shortcut_text(it.cmd.shortcut_text());
                let r = ui.add_enabled(it.enabled, button);
                name(&r, &it.label, it.enabled, it.checked);
                if r.clicked() {
                    ui.close();
                    *run = Some(it.cmd);
                }
            }
            Entry::Sep => {
                ui.separator();
            }
            Entry::Sub(label, sub) => {
                let r = ui.menu_button(ticked(ui, label, true, room), |ui| draw(app, ui, sub, run)).response;
                name(&r, label, true, None);
            }
            Entry::Plugins => crate::ruby_ui::context_items(app, ui),
        }
    }
}

// ---------------------------------------------------------------------------------------
// The commands

/// Whether a command of this menu can run now (also for the shortcuts and the search).
pub fn enabled(app: &EsquisseApp, c: CtxCmd) -> bool {
    use CtxCmd as C;
    let s = Sel::new(app);
    let edges = || !sel_edges(s.sel).is_empty();
    let faces = || !sel_faces(s.sel).is_empty();
    let one_inst = || matches!(s.sel, [EntityRef::Instance(_)]);
    match c {
        C::ShowPanel(_) => true,
        C::SelectBounding | C::AreaTag | C::AreaMaterial => faces(),
        C::SelectConnectedFaces | C::SelectAllConnected => edges() || faces(),
        C::SelectSameTag | C::SelectSameMaterial => !s.sel.is_empty(),
        C::SelectAllInstances => !sel_insts(s.sel).is_empty(),
        C::Soften(_) => edges(),
        C::ExplodeCurve | C::ConvertToPolygon => !sel_curves(s.ents, s.sel).is_empty(),
        C::FindCenter => sel_curves(s.ents, s.sel).iter().any(|c| is_arc(s.ents, *c)),
        C::AlignViewFace | C::AlignAxesFace => faces(),
        C::GroupToComponent => {
            matches!(s.sel, [EntityRef::Instance(i)] if s.kind(*i) == Some(Kind::Group) && !s.locked(*i))
        }
        C::MakeUnique => !s.uniquable().is_empty(),
        C::Unglue => s.any_inst(|i| i.ext.glued_to.is_some()),
        C::ResetScale | C::ScaleDefinition => s.any_inst(|i| is_scaled(&i.xform)),
        C::ResetSkew => s.any_inst(|i| is_skewed(&i.xform)),
        C::ChangeAxes => change_axes_target(app).is_some(),
        C::SaveOut | C::ReloadComponent => {
            one_inst() && sel_insts(s.sel).iter().all(|i| s.kind(*i) == Some(Kind::Component))
        }
        C::ExportImage | C::ReloadImage | C::UseAsMaterial | C::Shadows(_) => selected_image(app).is_some(),
        C::EditTextureImage => crate::texturing::one_face(&app.doc, &app.cam, &app.style).is_some(),
        C::EditText => matches!(s.sel, [EntityRef::Annot(_)]),
        C::TextPosition(_) | C::Diameter(_) | C::Arrow(_) | C::Leader(_) => matches!(s.sel, [EntityRef::Annot(_)]),
        C::OuterShellInside => matches!(s.sel, [EntityRef::Instance(i)] if s.solids_inside(*i) > 1),
        C::ResetAxes => app.doc.model.axes != DMat4::IDENTITY,
        C::AlignViewAxes => true,
        // One group or component selected.
        C::ConnectedSnaps => one_inst(),
        C::TroubleshootFill => app.sel_section().is_some() && app.style.section_fill,
        C::ProjectPhoto => app.photo_cmd_enabled() && faces(),
        C::MoveAxes => true,
    }
}

/// `Some(ticked)` for the entries shown with a tick.
pub fn checked(app: &EsquisseApp, c: CtxCmd) -> Option<bool> {
    let ents = app.doc.ents();
    let annot = match app.doc.sel.as_slice() {
        [EntityRef::Annot(a)] => Some(*a),
        _ => None,
    };
    match c {
        CtxCmd::Shadows(cast) => {
            let i = selected_image(app)?;
            Some(flag(ents, &inst_key(i), if cast { "casts" } else { "receives" }, true))
        }
        CtxCmd::TextPosition(p) => {
            let k = crate::annot::prop_key(annot?);
            Some(num(ents, &k, "text_position").map_or(1, |v| v as u8) == p)
        }
        CtxCmd::Diameter(d) => Some(flag(ents, &crate::annot::prop_key(annot?), "diameter", false) == d),
        CtxCmd::TroubleshootFill => Some(ents.sections.get(app.sel_section()?)?.troubleshoot),
        CtxCmd::Arrow(k) => Some(crate::annot::style_of(&app.doc.model, ents, annot?).arrow == k),
        CtxCmd::Leader(k) => {
            let a = annot?;
            let hidden = !flag(ents, &crate::annot::prop_key(a), "display_leader", true);
            Some(if k == 0 {
                hidden
            } else {
                !hidden && crate::annot::style_of(&app.doc.model, ents, a).leader_type == k
            })
        }
        _ => None,
    }
}

pub fn run(app: &mut EsquisseApp, ctx: &egui::Context, c: CtxCmd) {
    use CtxCmd as C;
    match c {
        C::ShowPanel(p) => {
            app.ui_state.panels.insert(p);
            app.ui_state.tray = true;
            app.ui_state.unfold = Some(p);
        }
        C::SelectBounding
        | C::SelectConnectedFaces
        | C::SelectAllConnected
        | C::SelectSameTag
        | C::SelectSameMaterial
        | C::SelectAllInstances => {
            let back = |f: FaceId| crate::tools::postex::back_seen(&app.doc, &app.cam, f);
            let add = selection_to_add(&app.doc.model, app.doc.ents(), &app.doc.sel, c, &back);
            let mut sel = app.doc.sel.clone();
            for e in add {
                if !sel.contains(&e) {
                    sel.push(e);
                }
            }
            app.doc.select(sel);
        }
        C::Soften(on) => {
            let edges = sel_edges(&app.doc.sel);
            app.doc.commit("Adoucir les arêtes", |m, d| {
                let e = m.entities_mut(d);
                for x in edges {
                    if let Some(edge) = e.edges.get_mut(x) {
                        // soft on or off, never smooth.
                        edge.soft = on;
                        edge.smooth = false;
                    }
                }
            });
        }
        C::ExplodeCurve => {
            let curves = sel_curves(app.doc.ents(), &app.doc.sel);
            app.doc.commit("Éclater les courbes", |m, d| explode_curves(m.entities_mut(d), &curves));
        }
        C::ConvertToPolygon => {
            let curves = sel_curves(app.doc.ents(), &app.doc.sel);
            app.doc.commit("Convertir la courbe en polygone", |m, d| {
                let e = m.entities_mut(d);
                for c in curves {
                    e.set_prop(&curve_key(c), "kind", AttrValue::Text("polygon".into()));
                }
            });
        }
        C::FindCenter => {
            let ents = app.doc.ents();
            let centres: Vec<DVec3> = sel_curves(ents, &app.doc.sel)
                .into_iter()
                .filter(|c| is_arc(ents, *c))
                .filter_map(|c| esq_core::curves::arc_fit(ents, c).map(|a| a.center))
                .collect();
            if !centres.is_empty() {
                app.doc.commit("Rechercher le centre de cet arc", |m, d| {
                    let e = m.entities_mut(d);
                    for p in centres {
                        e.guides.insert(Guide::point(p));
                    }
                });
            }
        }
        C::AreaTag | C::AreaMaterial => {
            let back = |f: FaceId| crate::tools::postex::back_seen(&app.doc, &app.cam, f);
            let a = if c == C::AreaTag {
                area_by_tag(&app.doc.model, app.doc.ents(), &app.doc.sel)
            } else {
                area_by_material(&app.doc.model, app.doc.ents(), &app.doc.sel, &back)
            };
            app.ui_state.extra.ops.area = Some(esq_core::units::format_area(a, app.doc.model.units));
        }
        C::AlignViewFace => {
            if let Some(f) = clicked_face(app) {
                align_view_to_face(app, f);
            }
        }
        C::AlignAxesFace => {
            if let Some(f) = clicked_face(app)
                && let Some(axes) = axes_on_face(app, f)
            {
                app.doc.commit("Déplacer les axes", |m, _| m.axes = axes);
            }
        }
        C::GroupToComponent => {
            if let [EntityRef::Instance(i)] = app.doc.sel.as_slice() {
                let i = *i;
                let mut made = None;
                app.doc.commit("Créer un composant", |m, d| made = m.group_to_component(d, i));
                if let Some(n) = made {
                    app.doc.select(vec![EntityRef::Instance(n)]);
                }
            }
        }
        C::MakeUnique => {
            let defs = Sel::new(app).uniquable();
            let insts = sel_insts(&app.doc.sel);
            app.doc.commit("Rendre unique", |m, d| {
                for def in defs {
                    let copy = m.defs[def].clone();
                    let name = m.unique_def_name(&copy.name);
                    let nd = m.defs.insert(esq_core::model::Definition { name, ..copy });
                    let e = m.entities_mut(d);
                    for i in &insts {
                        if let Some(x) = e.instances.get_mut(*i).filter(|x| x.def == def && !x.locked) {
                            x.def = nd;
                        }
                    }
                }
            });
        }
        C::Unglue | C::ResetScale | C::ResetSkew | C::ScaleDefinition => {
            let insts = sel_insts(&app.doc.sel);
            let label = c.label();
            app.doc.commit(label, |m, d| {
                for i in insts {
                    match c {
                        C::Unglue => m.unglue(d, i),
                        C::ResetScale => m.reset_scale(d, i),
                        C::ResetSkew => m.reset_skew(d, i),
                        _ => m.scale_definition(d, i),
                    };
                }
            });
        }
        C::ChangeAxes => {
            if let Some((path, inst)) = change_axes_target(app) {
                app.set_tool(ToolId::AxesTool);
                app.tool = Box::new(crate::tools::AxesTool::for_component(path, inst));
            }
        }
        C::SaveOut => save_out(app),
        C::ReloadComponent => reload_component(app),
        C::ExportImage => export_image(app),
        C::ReloadImage => reload_image(app),
        C::UseAsMaterial => {
            if let Some(i) = selected_image(app) {
                let inst = app.doc.ents().instances[i].clone();
                app.doc.commit("Utiliser comme matière", |m, _| {
                    use_as_material(m, &inst);
                });
            }
        }
        C::Shadows(cast) => {
            if let Some(i) = selected_image(app) {
                let name = if cast { "casts" } else { "receives" };
                let on = !flag(app.doc.ents(), &inst_key(i), name, true);
                app.doc.commit(if cast { "Projeter des ombres" } else { "Recevoir des ombres" }, |m, d| {
                    m.entities_mut(d).set_prop(&inst_key(i), name, AttrValue::Bool(on));
                });
            }
        }
        C::EditTextureImage => {
            let face = crate::texturing::one_face(&app.doc, &app.cam, &app.style);
            if let Some((f, back, _)) = face {
                let fc = &app.doc.ents().faces[f];
                if let Some(m) = if back { fc.back } else { fc.front } {
                    crate::mat_panel::edit_externally(app, m);
                }
            }
        }
        C::EditText => {
            if let [EntityRef::Annot(a)] = app.doc.sel.as_slice() {
                let a = *a;
                let text = app.doc.ents().annots.get(a).map(|x| x.text.clone()).unwrap_or_default();
                app.ui_state.edit_text = Some((a, text));
            }
        }
        C::TextPosition(_) | C::Diameter(_) | C::Arrow(_) | C::Leader(_) => {
            let [EntityRef::Annot(a)] = app.doc.sel.as_slice() else { return };
            let key = crate::annot::prop_key(*a);
            let hidden = !flag(app.doc.ents(), &key, "display_leader", true);
            let label = match c {
                C::TextPosition(_) | C::Diameter(_) => "Modifier la cotation",
                _ => "Modifier le texte",
            };
            app.doc.commit(label, |m, d| {
                let e = m.entities_mut(d);
                match c {
                    C::TextPosition(p) => e.set_prop(&key, "text_position", AttrValue::Number(p as f64)),
                    C::Diameter(on) => e.set_prop(&key, "diameter", AttrValue::Bool(on)),
                    C::Arrow(k) => e.set_prop(&key, "arrow_type", AttrValue::Number(k as f64)),
                    // Hidden turns the leader off and on.
                    C::Leader(0) => e.set_prop(&key, "display_leader", AttrValue::Bool(hidden)),
                    C::Leader(k) => {
                        e.set_prop(&key, "leader_type", AttrValue::Number(k as f64));
                        e.set_prop(&key, "display_leader", AttrValue::Bool(true));
                    }
                    _ => {}
                }
            });
        }
        C::OuterShellInside => {
            if let [EntityRef::Instance(i)] = app.doc.sel.as_slice() {
                let d = app.doc.ents().instances[*i].def;
                let kids: Vec<InstanceId> = app.doc.model.defs[d].entities.instances.keys().collect();
                let r = app.doc.try_commit("Enveloppe externe", |m, _| m.outer_shell(Some(d), &kids).map(|_| ()));
                if let Err(e) = r {
                    app.say(ctx, e);
                }
            }
        }
        C::ResetAxes => app.doc.commit("Réinitialiser les axes", |m, _| m.axes = DMat4::IDENTITY),
        C::AlignViewAxes => align_view_to_axes(app),
        C::ConnectedSnaps => {
            if let [EntityRef::Instance(i)] = app.doc.sel.as_slice() {
                let found = connected_with_snaps(&app.doc.model, app.doc.ents(), *i);
                app.doc.select(found.into_iter().map(EntityRef::Instance).collect());
            }
        }
        // A viewing aid: no operation to undo (SketchUp flips the flag and redraws).
        C::TroubleshootFill => {
            if let Some(s) = app.sel_section() {
                let d = app.doc.ctx_def();
                if let Some(sp) = app.doc.model.entities_mut(d).sections.get_mut(s) {
                    sp.troubleshoot = !sp.troubleshoot;
                }
                app.doc.rev += 1;
            }
        }
        C::ProjectPhoto => crate::ctx_dialogs::project_photo(app),
        C::MoveAxes => crate::ctx_dialogs::open_move_axes(),
    }
}

/// Select › Connected with Snaps: from the selected group or component,
/// the instances of the same collection one of whose snaps lies on a snap of an instance
/// already reached (within 0.001", per axis and in distance), the two pointing at each other
/// (the dot product of their directions is −1 within 0.001); the selected one first.
pub fn connected_with_snaps(m: &Model, ents: &Entities, start: InstanceId) -> Vec<InstanceId> {
    const TOL_MM: f64 = 0.001 * 25.4;
    // Every snap of every instance of the collection, placed in the collection.
    let mut snaps: Vec<(InstanceId, DVec3, DVec3)> = Vec::new();
    for (i, inst) in ents.instances.iter() {
        let Some(d) = m.defs.get(inst.def) else { continue };
        for s in &d.entities.snaps {
            let pos = inst.xform.transform_point3(s.pos);
            let dir = inst.xform.transform_vector3(s.dir).normalize_or_zero();
            snaps.push((i, pos, dir));
        }
    }
    let mut out = vec![start];
    let mut todo = vec![start];
    while let Some(cur) = todo.pop() {
        let mine: Vec<(DVec3, DVec3)> = snaps.iter().filter(|s| s.0 == cur).map(|s| (s.1, s.2)).collect();
        for &(other, pos, dir) in &snaps {
            if out.contains(&other) {
                continue;
            }
            let touches = mine.iter().any(|(p, d)| {
                let delta = pos - *p;
                delta.abs().max_element() < TOL_MM
                    && delta.length_squared() < TOL_MM * TOL_MM
                    && (d.dot(dir) + 1.0).abs() < 0.001
            });
            if touches {
                out.push(other);
                todo.push(other);
            }
        }
    }
    out
}

/// Align View on the axes: looking down blue at the origin, green up the
/// screen, from the same distance.
pub fn align_view_to_axes(app: &mut EsquisseApp) {
    let x = app.doc.model.axes;
    let o = x.transform_point3(DVec3::ZERO);
    let z = x.transform_vector3(DVec3::Z).normalize_or(DVec3::Z);
    let y = x.transform_vector3(DVec3::Y).normalize_or(DVec3::Y);
    app.remember_view();
    let cam = &mut app.cam;
    cam.target = o;
    cam.roll = 0.0;
    cam.pitch = z.z.clamp(-1.0, 1.0).asin().clamp(-89.9f64.to_radians(), 89.9f64.to_radians());
    cam.yaw = if z.x.abs() + z.y.abs() < 1e-6 {
        // From above, the view's up is green: the eye leans the other way.
        (-y.y).atan2(-y.x)
    } else {
        z.y.atan2(z.x)
    };
}

// ---------------------------------------------------------------------------------------
// Helpers of the commands (pure where the tests need them)

/// Select › … : what the command adds to the selection.
/// `back_seen` tells
/// which side of a face the camera sees (its material is the one compared).
pub fn selection_to_add(
    m: &Model,
    ents: &Entities,
    sel: &[EntityRef],
    c: CtxCmd,
    back_seen: &dyn Fn(FaceId) -> bool,
) -> Vec<EntityRef> {
    let mut out: Vec<EntityRef> = Vec::new();
    let push = |e: EntityRef, out: &mut Vec<EntityRef>| {
        if !out.contains(&e) {
            out.push(e);
        }
    };
    match c {
        CtxCmd::SelectBounding => {
            for f in sel_faces(sel) {
                for e in ents.face_edges(f) {
                    push(EntityRef::Edge(e), &mut out);
                }
            }
        }
        CtxCmd::SelectConnectedFaces => {
            let mut edges = sel_edges(sel);
            for f in sel_faces(sel) {
                edges.extend(ents.face_edges(f));
            }
            for e in edges {
                for f in ents.edges.get(e).map(|x| x.faces.clone()).unwrap_or_default() {
                    push(EntityRef::Face(f), &mut out);
                }
            }
        }
        CtxCmd::SelectAllConnected => {
            for e in sel.iter().filter(|e| matches!(e, EntityRef::Edge(_) | EntityRef::Face(_))) {
                for x in ents.connected(*e) {
                    push(x, &mut out);
                }
            }
        }
        CtxCmd::SelectSameTag => {
            let tags: HashSet<LayerIdx> = sel.iter().filter_map(|e| ents.layer_of(*e)).collect();
            for e in all_entities(ents) {
                if ents.layer_of(e).is_some_and(|l| tags.contains(&l)) && !ents.is_hidden(e) {
                    push(e, &mut out);
                }
            }
        }
        CtxCmd::SelectSameMaterial => {
            let mut mats: HashSet<MaterialId> = HashSet::new();
            for e in sel {
                let m = match e {
                    EntityRef::Face(f) => ents.faces.get(*f).and_then(|x| if back_seen(*f) { x.back } else { x.front }),
                    EntityRef::Instance(i) => ents.instances.get(*i).and_then(|x| x.material),
                    _ => None,
                };
                mats.extend(m);
            }
            for (f, x) in &ents.faces {
                if !x.hidden
                    && (x.front.is_some_and(|m| mats.contains(&m)) || x.back.is_some_and(|m| mats.contains(&m)))
                {
                    push(EntityRef::Face(f), &mut out);
                }
            }
            for (i, x) in &ents.instances {
                if !x.hidden && x.material.is_some_and(|m| mats.contains(&m)) {
                    push(EntityRef::Instance(i), &mut out);
                }
            }
        }
        CtxCmd::SelectAllInstances => {
            let defs: HashSet<DefId> = sel_insts(sel)
                .iter()
                .filter_map(|i| ents.instances.get(*i).map(|x| x.def))
                .filter(|d| m.defs.get(*d).is_some_and(|x| x.ext.image.is_none()))
                .collect();
            for (i, x) in &ents.instances {
                if defs.contains(&x.def) {
                    push(EntityRef::Instance(i), &mut out);
                }
            }
        }
        _ => {}
    }
    out
}

fn all_entities(e: &Entities) -> Vec<EntityRef> {
    let mut v: Vec<EntityRef> = e.edges.keys().map(EntityRef::Edge).collect();
    v.extend(e.faces.keys().map(EntityRef::Face));
    v.extend(e.instances.keys().map(EntityRef::Instance));
    v.extend(e.guides.keys().map(EntityRef::Guide));
    v.extend(e.annots.keys().map(EntityRef::Annot));
    v.extend(e.sections.keys().map(EntityRef::Section));
    v
}

/// Explode Curve: the edges of these curves become plain edges.
pub fn explode_curves(e: &mut Entities, curves: &[u32]) {
    for x in e.edges.values_mut() {
        if x.curve.is_some_and(|c| curves.contains(&c)) {
            x.curve = None;
        }
    }
    for c in curves {
        e.props.remove(&curve_key(*c));
    }
}

/// Area of a face seen through `x` (outer loop minus holes, Newell).
fn face_area_xf(e: &Entities, f: FaceId, x: &DMat4) -> f64 {
    let face = &e.faces[f];
    let ring = |l: &[esq_core::model::VertexId]| -> f64 {
        let pts: Vec<DVec3> = l.iter().map(|v| x.transform_point3(e.pos(*v))).collect();
        let mut n = DVec3::ZERO;
        for i in 0..pts.len() {
            n += pts[i].cross(pts[(i + 1) % pts.len()]);
        }
        n.length() * 0.5
    };
    ring(&face.outer) - face.holes.iter().map(|h| ring(h)).sum::<f64>()
}

/// Sum of the areas of the faces `pick` accepts, in the whole model (inside groups and
/// components too, at their size).
fn area_where(m: &Model, pick: &dyn Fn(&Entities, FaceId) -> bool) -> f64 {
    fn rec(m: &Model, e: &Entities, x: DMat4, pick: &dyn Fn(&Entities, FaceId) -> bool, depth: usize) -> f64 {
        let mut a: f64 = e.faces.keys().filter(|f| pick(e, *f)).map(|f| face_area_xf(e, f, &x)).sum();
        if depth < 32 {
            for i in e.instances.values() {
                if let Some(d) = m.defs.get(i.def) {
                    a += rec(m, &d.entities, x * i.xform, pick, depth + 1);
                }
            }
        }
        a
    }
    rec(m, &m.root, DMat4::IDENTITY, pick, 0)
}

/// Area › Tag: the faces on the tags of the selected faces.
pub fn area_by_tag(m: &Model, ents: &Entities, sel: &[EntityRef]) -> f64 {
    let tags: HashSet<LayerIdx> = sel_faces(sel).iter().filter_map(|f| ents.faces.get(*f).map(|x| x.layer)).collect();
    area_where(m, &|e, f| tags.contains(&e.faces[f].layer))
}

/// Area › Material: the faces painted (either side) with the materials
/// the selected faces show; a face without material counts the unpainted ones.
pub fn area_by_material(m: &Model, ents: &Entities, sel: &[EntityRef], back_seen: &dyn Fn(FaceId) -> bool) -> f64 {
    let mut mats: HashSet<Option<MaterialId>> = HashSet::new();
    for f in sel_faces(sel) {
        if let Some(x) = ents.faces.get(f) {
            mats.insert(if back_seen(f) { x.back } else { x.front });
        }
    }
    area_where(m, &|e, f| {
        let x = &e.faces[f];
        mats.contains(&x.front) || mats.contains(&x.back)
    })
}

/// The face Align View and Align Axes work on: the right-clicked one, else the first
/// selected.
fn clicked_face(app: &EsquisseApp) -> Option<FaceId> {
    match app.ui_state.extra.ctx_menu.target {
        Some(Target::Entity(EntityRef::Face(f))) if app.doc.ents().faces.contains_key(f) => Some(f),
        _ => sel_faces(&app.doc.sel).first().copied(),
    }
}

/// SketchUp's axes of a plane: red horizontal, green up the slope; for a
/// horizontal plane, red and green.
fn plane_axes(n: DVec3) -> (DVec3, DVec3) {
    let u = if n.x.abs() < 1.0 / 64.0 && n.y.abs() < 1.0 / 64.0 { DVec3::Y.cross(n) } else { DVec3::Z.cross(n) };
    let u = u.normalize();
    (u, n.cross(u).normalize())
}

/// Align View on a face (`FaceCmds::AddAlign`): the camera looks straight
/// at the face from the side it is on, at the centre of its extent, from the same distance.
pub fn align_view_to_face(app: &mut EsquisseApp, f: FaceId) {
    let x = app.doc.ctx_xform();
    let e = app.doc.ents();
    let face = &e.faces[f];
    let pts: Vec<DVec3> = face.outer.iter().map(|v| x.transform_point3(e.pos(*v))).collect();
    let mut n = x.transform_vector3(face.normal).normalize_or(DVec3::Z);
    let eye = app.cam.eye();
    if n.dot(eye - pts[0]) < 0.0 {
        n = -n;
    }
    let (u, v) = plane_axes(n);
    let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
    for p in &pts {
        let q = DVec2::new((*p - pts[0]).dot(u), (*p - pts[0]).dot(v));
        lo = lo.min(q);
        hi = hi.max(q);
    }
    let c = (lo + hi) * 0.5;
    let target = pts[0] + u * c.x + v * c.y;
    let dist = target.distance(eye).max(1.0);
    app.remember_view();
    let cam = &mut app.cam;
    cam.target = target;
    cam.distance = dist;
    cam.roll = 0.0;
    cam.pitch = n.z.clamp(-1.0, 1.0).asin().clamp(-89.9f64.to_radians(), 89.9f64.to_radians());
    cam.yaw = if n.x.abs() + n.y.abs() < 1e-9 { -std::f64::consts::FRAC_PI_2 } else { n.y.atan2(n.x) };
}

/// Align Axes on a face: blue along the face's normal toward the camera,
/// the origin at the corner nearest the lower left of the face on screen, red along the
/// face's edge from there that is the most horizontal on screen.
pub fn axes_on_face(app: &EsquisseApp, f: FaceId) -> Option<DMat4> {
    let x = app.doc.ctx_xform();
    let e = app.doc.ents();
    let face = e.faces.get(f)?;
    let world = |v: esq_core::model::VertexId| x.transform_point3(e.pos(v));
    let mut n = x.transform_vector3(face.normal).normalize_or_zero();
    if n.dot(app.cam.eye() - world(face.outer[0])) < 0.0 {
        n = -n;
    }
    let screen: Vec<(esq_core::model::VertexId, DVec2)> =
        face.outer.iter().filter_map(|v| app.cam.project(world(*v)).map(|s| (*v, s))).collect();
    if screen.is_empty() {
        return None;
    }
    let min_x = screen.iter().map(|(_, s)| s.x).fold(f64::INFINITY, f64::min);
    let max_y = screen.iter().map(|(_, s)| s.y).fold(f64::NEG_INFINITY, f64::max);
    let corner = DVec2::new(min_x, max_y);
    let (o, _) = screen.iter().min_by(|a, b| a.1.distance_squared(corner).total_cmp(&b.1.distance_squared(corner)))?;
    let right = app.cam.forward().cross(app.cam.up()).normalize_or(DVec3::X);
    let own: HashSet<EdgeId> = e.face_edges(f).into_iter().collect();
    let p = world(*o);
    let xdir = e.verts[*o]
        .edges
        .iter()
        .filter(|ed| own.contains(ed))
        .map(|ed| {
            let v = e.edges[*ed].v;
            let other = if v[0] == *o { v[1] } else { v[0] };
            (world(other) - p).normalize_or_zero()
        })
        .max_by(|a, b| a.dot(right).abs().total_cmp(&b.dot(right).abs()))?;
    let y = n.cross(xdir).normalize_or_zero();
    if y == DVec3::ZERO {
        return None;
    }
    Some(DMat4::from_cols(xdir.extend(0.0), y.extend(0.0), n.extend(0.0), p.extend(1.0)))
}

/// Create Group from Slice has something to put in the group.
fn slice_has_content(app: &EsquisseApp) -> bool {
    let Some(s) = app.sel_section() else { return false };
    let mut m = app.doc.model.clone();
    m.group_from_slice(app.doc.ctx_def(), s).is_some()
}

/// Change Axes works on the selected component, or on the open one: (path of the
/// collection that holds it, the instance).
fn change_axes_target(app: &EsquisseApp) -> Option<(Vec<InstanceId>, InstanceId)> {
    let s = Sel::new(app);
    match s.sel {
        [EntityRef::Instance(i)] if s.kind(*i) == Some(Kind::Component) && !s.locked(*i) => {
            Some((app.doc.ctx.clone(), *i))
        }
        [] => {
            let (last, path) = app.doc.ctx.split_last()?;
            let d = app.doc.ctx_def()?;
            (!app.doc.model.defs[d].is_group).then(|| (path.to_vec(), *last))
        }
        _ => None,
    }
}

/// The world transformation of the collection at `path`.
pub fn path_xform(m: &Model, path: &[InstanceId]) -> DMat4 {
    let mut x = DMat4::IDENTITY;
    let mut e = &m.root;
    for i in path {
        let Some(inst) = e.instances.get(*i) else { break };
        x *= inst.xform;
        e = &m.defs[inst.def].entities;
    }
    x
}

fn selected_image(app: &EsquisseApp) -> Option<InstanceId> {
    match app.doc.sel.as_slice() {
        [EntityRef::Instance(i)] if Sel::new(app).kind(*i) == Some(Kind::Image) => Some(*i),
        _ => None,
    }
}

/// The plain material made from an image's picture, if Use As Material made one already
fn material_of_image(m: &Model, inst: &esq_core::model::Instance) -> Option<MaterialId> {
    let own = m.image_material(inst.def)?;
    let tex = m.materials[own].ext.texture.as_ref()?;
    m.materials.iter().find_map(|(id, x)| {
        (id != own && !x.ext.owner_image && x.ext.texture.as_ref().is_some_and(|t| t.image == tex.image)).then_some(id)
    })
}

/// Use As Material: a material of the model (in the Materials panel) with the picture.
pub fn use_as_material(m: &mut Model, inst: &esq_core::model::Instance) -> Option<MaterialId> {
    if material_of_image(m, inst).is_some() {
        return None;
    }
    let own = m.image_material(inst.def)?;
    let src = m.materials[own].clone();
    let taken: Vec<String> = m.materials.values().map(|x| x.name.clone()).collect();
    let base = m.defs[inst.def].name.clone();
    let mut ext = src.ext.clone();
    ext.owner_image = false;
    Some(m.materials.insert(esq_core::model::Material { name: Model::unique_name(&base, &taken), ext, ..src }))
}

fn save_out(app: &mut EsquisseApp) {
    let [EntityRef::Instance(i)] = app.doc.sel.as_slice() else { return };
    let d = app.doc.ents().instances[*i].def;
    let Some(p) = crate::settings::dialog("components")
        .add_filter("Esquisse", &["esq"])
        .add_filter("SketchUp", &["skp"])
        .set_file_name(format!("{}.esq", app.doc.model.defs[d].name))
        .save_file()
    else {
        return;
    };
    match crate::dialogs::comp_edit::save_definition(&app.doc.model, d, &p) {
        Ok(()) => {
            let s = p.display().to_string();
            app.doc.commit("Enregistrer le composant", |m, _| m.defs[d].ext.path = s);
        }
        Err(e) => app.message = Some((format!("Échec de l'enregistrement : {e}"), 0.0)),
    }
}

/// Reload › From Device: the definition takes the content of a model file.
pub fn reload_definition(m: &mut Model, d: DefId, src: &Model, path: &str) {
    let mut map = esq_core::groups::ImportMap::default();
    let ents = m.import_entities(src, &src.root, &mut map);
    m.defs[d].entities = ents;
    m.defs[d].ext.path = path.to_string();
    m.rebuild_topology();
}

fn reload_component(app: &mut EsquisseApp) {
    let [EntityRef::Instance(i)] = app.doc.sel.as_slice() else { return };
    let d = app.doc.ents().instances[*i].def;
    let Some(p) = crate::settings::dialog("components").add_filter("Modèles", &["skp", "esq"]).pick_file() else {
        return;
    };
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    let src = match ext.as_str() {
        "skp" => esq_skp::import_skp(&p).map_err(|e| e.to_string()),
        _ => esq_core::io::load_esq(&p).map_err(|e| e.to_string()),
    };
    match src {
        Ok(src) => {
            let s = p.display().to_string();
            app.doc.commit("Recharger", |m, _| reload_definition(m, d, &src, &s));
        }
        Err(e) => app.message = Some((format!("Impossible d'ouvrir : {e}"), 0.0)),
    }
}

fn export_image(app: &mut EsquisseApp) {
    let Some(i) = selected_image(app) else { return };
    let m = &app.doc.model;
    let def = app.doc.ents().instances[i].def;
    let Some(tex) = m.image_material(def).and_then(|x| m.materials[x].ext.texture.clone()) else { return };
    let ext = std::path::Path::new(&tex.filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_else(|| if tex.image.0.starts_with(b"\x89PNG") { "png".into() } else { "jpg".into() });
    let Some(p) = crate::settings::dialog("images")
        .add_filter("Image", &[ext.as_str()])
        .set_file_name(format!("{}.{ext}", m.defs[def].name))
        .save_file()
    else {
        return;
    };
    if let Err(e) = std::fs::write(&p, &tex.image.0[..]) {
        app.message = Some((format!("Échec de l'export : {e}"), 0.0));
    }
}

fn reload_image(app: &mut EsquisseApp) {
    let Some(i) = selected_image(app) else { return };
    let def = app.doc.ents().instances[i].def;
    let Some(p) = crate::settings::dialog("images")
        .add_filter("Images", &["png", "jpg", "jpeg", "bmp", "tga", "tif", "tiff"])
        .pick_file()
    else {
        return;
    };
    match crate::mat_panel::load_texture(&p) {
        Ok(t) => {
            let path = p.display().to_string();
            app.doc.commit("Recharger", |m, _| reload_image_def(m, def, t, &path));
        }
        Err(e) => app.message = Some((format!("Impossible de charger le fichier image « {} ». {e}", p.display()), 0.0)),
    }
}

/// Reload of an image: its picture changes, the image keeps its place and size.
pub fn reload_image_def(m: &mut Model, def: DefId, t: esq_core::meta::TextureData, path: &str) {
    let Some(mat) = m.image_material(def) else { return };
    let size = m.materials[mat].ext.texture.as_ref().map(|x| x.size);
    let px = t.px;
    m.materials[mat].ext.texture = Some(esq_core::meta::TextureData { size: size.unwrap_or(t.size), ..t });
    if let Some(img) = m.defs[def].ext.image.as_mut() {
        img.path = path.to_string();
        img.px = px;
    }
}
