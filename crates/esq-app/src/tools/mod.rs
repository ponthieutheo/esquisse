//! Interactive tools. Each tool receives mouse/keyboard events in viewport coordinates
//! (egui points) and edits the document through `Document::commit`.

mod annot_tools;
mod camtools;
pub mod classifier;
mod divide;
mod draw;
mod draw2;
mod face_edges;
mod followme;
pub mod hints;
mod igloo;
mod image;
mod light;
mod misc;
mod misc2;
mod modify;
pub mod move_grips;
pub mod move_handles;
mod outils_icons;
mod photo;
mod photo_point;
mod place;
pub mod postex;
pub mod ruby;
pub mod sample;
pub mod sandbox;
mod scale;
mod sculpt;
mod sculpt_mesh;
mod section;
mod select;
pub mod snap;
mod solid;

pub use annot_tools::{DimensionTool, TextTool};
pub use camtools::{FovTool, LookAroundTool, PositionCameraTool, WalkTool, ZoomWindowTool};
pub use divide::DivideTool;
pub use draw::{ArcTool, CircleTool, LineTool, RectTool};
pub use draw2::{Arc3Tool, ArcCenterTool, FreehandTool, RotRectTool};
pub use face_edges::FaceEdgesTool;
pub use followme::FollowMeTool;
pub use image::ImageTool;
pub use misc::{EraserTool, PaintTool, TapeTool};
pub use misc2::{AxesTool, FlipTool, LassoTool, NorthTool, ProtractorTool, TagTool};
pub use modify::{MoveTool, OffsetTool, PushPullTool, RotateTool};
pub use outils_icons::outils_icon;
#[cfg(test)]
pub use photo::tests as photo_tests;
pub use photo::{Editor as PhotoEditor, MatchPhotoTool};
pub use place::PlaceTool;
pub use sandbox::{ActionTool, AddDetailTool, FlipEdgeTool, FromScratchTool};
pub use scale::ScaleTool;
pub use sculpt::SmooveTool;
pub use sculpt_mesh::{DrapeTool, StampTool};
pub use section::SectionPlaneTool;
pub use select::{SelectTool, hit_under};
pub use solid::{SolidKind, SolidTool};

use eframe::egui::{self, Color32, Pos2, Stroke};
use esq_core::glam::{DMat4, DVec2, DVec3};
use esq_core::math::{Plane, basis_for};
use esq_core::model::{AnnotId, FaceId, LayerIdx, MaterialId, Units};
use esq_core::tess::face_triangles;
use esq_core::units::{format_length, parse_length};

use crate::camera::Camera;
use crate::doc::Document;
use crate::pick::{InferCx, Lock, Snap, SnapKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToolId {
    Select,
    Line,
    Rect,
    Circle,
    Polygon,
    Arc,
    PushPull,
    FollowMe,
    Move,
    Rotate,
    Scale,
    Offset,
    Eraser,
    Paint,
    Tape,
    Orbit,
    Pan,
    Zoom,
    /// Placing a component from Window › Components.
    Component,
    Freehand,
    ArcCenter,
    Arc3,
    Pie,
    RotRect,
    Lasso,
    TagTool,
    Flip,
    Protractor,
    AxesTool,
    North,
    Fov,
    ZoomWindow,
    PositionCamera,
    LookAround,
    Walk,
    Dimension,
    Text,
    SectionPlane,
    OuterShell,
    SolidIntersect,
    SolidUnion,
    SolidSubtract,
    SolidTrim,
    SolidSplit,
    FromScratch,
    FromContours,
    FlipEdge,
    AddDetail,
    Smoove,
    Drape,
    Stamp,
    /// Camera › Match New Photo.
    MatchPhoto,
    /// A tool written in Ruby (`Model#select_tool`, `Tools#push_tool`), see `tools/ruby.rs`.
    Ruby,
    /// Placing an imported image (`CImageTool`), see `tools/image.rs`.
    Image,
    /// Right-click on an edge › Divide (`CDivideLineTool`).
    Divide,
    /// Extensions › Developer › Show Face Edges (`CShowFaceEdgesTool`).
    ShowFaceEdges,
    /// Classifier (`CClassifierTool`).
    Classifier,
    /// Window › Match Photo › Photo Point Tool (`CAddRayTool`).
    PhotoPoint,
    /// Tools › Add Snap (`CSnapTool`).
    AddSnap,
    /// Places a point light (`LightTool`).
    Light,
    /// Camera › Image Igloo (`IglooTool`).
    Igloo,
    /// Tools › Sample Material (`CPaintMatchTool`).
    SampleMaterial,
    /// Texture › Position (`CPositionTextureTool`), started on a face by `texturing.rs`.
    PositionTexture,
    /// Tools › Comment (`CommentTool`), see `comments/tool.rs`.
    Comment,
}

impl ToolId {
    pub fn label(self) -> &'static str {
        match self {
            ToolId::Select => "Sélectionner",
            ToolId::Line => "Ligne",
            ToolId::Rect => "Rectangle",
            ToolId::Circle => "Cercle",
            ToolId::Polygon => "Polygone",
            ToolId::Arc => "Arc par 2 points",
            ToolId::PushPull => "Pousser/Tirer",
            ToolId::FollowMe => "Suivez-moi",
            ToolId::Move => "Déplacer",
            ToolId::Rotate => "Faire pivoter",
            ToolId::Scale => "Mettre à l'échelle",
            ToolId::Offset => "Décalage",
            ToolId::Eraser => "Effacer",
            ToolId::Paint => "Colorier",
            ToolId::Tape => "Mètre",
            ToolId::Orbit => "Orbite",
            ToolId::Pan => "Panoramique",
            ToolId::Zoom => "Zoom",
            ToolId::Component => "Composant",
            ToolId::Freehand => "Main levée",
            ToolId::ArcCenter => "Arc",
            ToolId::Arc3 => "Arc par 3 points",
            ToolId::Pie => "Portion de Cercle",
            ToolId::RotRect => "Rectangle orienté",
            ToolId::Lasso => "Lasso",
            ToolId::TagTool => "Étiquette",
            ToolId::Flip => "Retourner",
            ToolId::Protractor => "Rapporteur",
            ToolId::AxesTool => "Axes",
            ToolId::North => "Vrai Nord",
            ToolId::Fov => "Champ angulaire",
            ToolId::ZoomWindow => "Fenêtre de zoom",
            ToolId::PositionCamera => "Positionner la caméra",
            ToolId::LookAround => "Pivoter",
            ToolId::Walk => "Visite",
            ToolId::Dimension => "Cotations",
            ToolId::Text => "Texte",
            ToolId::SectionPlane => "Plan de section",
            ToolId::OuterShell => "Enveloppe externe",
            ToolId::SolidIntersect => "Intersection",
            ToolId::SolidUnion => "Union",
            ToolId::SolidSubtract => "Soustraire",
            ToolId::SolidTrim => "Découper",
            ToolId::SolidSplit => "Diviser",
            ToolId::FromScratch => "À partir de zéro",
            ToolId::FromContours => "À partir des contours",
            ToolId::FlipEdge => "Retourner l'arête",
            ToolId::AddDetail => "Ajouter des détails",
            ToolId::Smoove => "Modeler",
            ToolId::Drape => "Projeter",
            ToolId::Stamp => "Tamponner",
            ToolId::MatchPhoto => "Adapter une photo",
            ToolId::Ruby => "Outil Ruby",
            ToolId::Image => "Image",
            ToolId::Divide => "Diviser",
            ToolId::ShowFaceEdges => "Afficher les arêtes de la face",
            ToolId::Classifier => "Classificateur",
            ToolId::PhotoPoint => "Outil Point photo",
            ToolId::AddSnap => "Ajouter un ancrage",
            ToolId::Light => "Lumière",
            ToolId::Igloo => "Dôme d'images",
            ToolId::SampleMaterial => "Échantillon Matière",
            ToolId::PositionTexture => "Positionner la texture",
            ToolId::Comment => "Ajouter un commentaire",
        }
    }

    pub fn make(self) -> Box<dyn Tool> {
        match self {
            ToolId::Select => Box::new(SelectTool::default()),
            ToolId::Line => Box::new(LineTool::default()),
            ToolId::Rect => Box::new(RectTool::default()),
            ToolId::Circle => Box::new(CircleTool::new(false)),
            ToolId::Polygon => Box::new(CircleTool::new(true)),
            ToolId::Arc => Box::new(ArcTool::default()),
            ToolId::PushPull => Box::new(PushPullTool::default()),
            ToolId::FollowMe => Box::new(FollowMeTool::default()),
            ToolId::Move => Box::new(MoveTool::default()),
            ToolId::Rotate => Box::new(RotateTool::default()),
            ToolId::Scale => Box::new(ScaleTool::default()),
            ToolId::Offset => Box::new(OffsetTool::default()),
            ToolId::Eraser => Box::new(EraserTool::default()),
            ToolId::Paint => Box::new(PaintTool::default()),
            ToolId::Tape => Box::new(TapeTool::default()),
            ToolId::Orbit | ToolId::Pan | ToolId::Zoom => Box::new(CameraTool(self)),
            ToolId::Component => Box::new(SelectTool::default()),
            ToolId::Freehand => Box::new(FreehandTool::default()),
            ToolId::ArcCenter => Box::new(ArcCenterTool::new(false)),
            ToolId::Arc3 => Box::new(Arc3Tool::default()),
            ToolId::Pie => Box::new(ArcCenterTool::new(true)),
            ToolId::RotRect => Box::new(RotRectTool::default()),
            ToolId::Lasso => Box::new(LassoTool::default()),
            ToolId::TagTool => Box::new(TagTool::default()),
            ToolId::Flip => Box::new(FlipTool::default()),
            ToolId::Protractor => Box::new(ProtractorTool::default()),
            ToolId::AxesTool => Box::new(AxesTool::default()),
            ToolId::North => Box::new(NorthTool::default()),
            ToolId::Fov => Box::new(FovTool::default()),
            ToolId::ZoomWindow => Box::new(ZoomWindowTool::default()),
            ToolId::PositionCamera => Box::new(PositionCameraTool::default()),
            ToolId::LookAround => Box::new(LookAroundTool::default()),
            ToolId::Walk => Box::new(WalkTool::default()),
            ToolId::Dimension => Box::new(DimensionTool::default()),
            ToolId::Text => Box::new(TextTool::default()),
            ToolId::FromScratch => Box::new(FromScratchTool::default()),
            ToolId::FromContours => Box::new(ActionTool(self)),
            ToolId::FlipEdge => Box::new(FlipEdgeTool::default()),
            ToolId::AddDetail => Box::new(AddDetailTool::default()),
            ToolId::SectionPlane => Box::new(SectionPlaneTool::default()),
            ToolId::Smoove => Box::new(SmooveTool::default()),
            ToolId::Drape => Box::new(DrapeTool::default()),
            ToolId::Stamp => Box::new(StampTool::default()),
            ToolId::MatchPhoto => Box::new(MatchPhotoTool::default()),
            // A Ruby tool is made by `Model#select_tool` with its Ruby object, never from its id.
            ToolId::Ruby => Box::new(SelectTool::default()),
            // The Image tool is made with its picture (`ImageTool::new`).
            ToolId::Image => Box::new(SelectTool::default()),
            ToolId::Divide => Box::new(DivideTool::default()),
            ToolId::ShowFaceEdges => Box::new(FaceEdgesTool::default()),
            ToolId::Classifier => Box::new(classifier::ClassifierTool::default()),
            ToolId::PhotoPoint => Box::new(photo_point::PhotoPointTool::default()),
            ToolId::AddSnap => Box::new(snap::SnapTool::default()),
            ToolId::Light => Box::new(light::LightTool::default()),
            ToolId::Igloo => Box::new(igloo::IglooTool::default()),
            ToolId::SampleMaterial => Box::new(sample::SampleTool::default()),
            // Position Texture needs a face: `texturing::position` makes it.
            ToolId::PositionTexture => Box::new(SelectTool::default()),
            ToolId::Comment => Box::new(crate::comments::CommentTool::default()),
            ToolId::OuterShell
            | ToolId::SolidIntersect
            | ToolId::SolidUnion
            | ToolId::SolidSubtract
            | ToolId::SolidTrim
            | ToolId::SolidSplit => Box::new(SolidTool::new(SolidKind::of(self).expect("solid tool"))),
        }
    }
}

pub struct ToolCx<'a> {
    pub doc: &'a mut Document,
    pub infer: InferCx<'a>,
    pub lock: Option<Lock>,
    pub mods: egui::Modifiers,
    pub units: Units,
    pub material: &'a mut Option<MaterialId>,
    /// Tag given by the Tag tool (Window › Tags, current tag).
    pub tag: &'a mut LayerIdx,
    pub message: &'a mut Option<String>,
    /// A camera tool sets this to change the view.
    pub new_cam: &'a mut Option<Camera>,
    /// The Text tool and a double-click on a text ask to edit it.
    pub edit_text: &'a mut Option<AnnotId>,
    /// The Section Plane tool turns View › Section Planes on.
    pub show_sections: &'a mut bool,
}

impl ToolCx<'_> {
    pub fn cam(&self) -> &Camera {
        self.infer.cam
    }

    pub fn snap(&self, m: DVec2, reference: Option<DVec3>, plane: Option<Plane>) -> Snap {
        self.snap_linear(m, reference, plane, crate::pick::Linear::All)
    }

    /// [`ToolCx::snap`] with only some linear inferences (the Line tool's Alt).
    pub fn snap_linear(
        &self,
        m: DVec2,
        reference: Option<DVec3>,
        plane: Option<Plane>,
        linear: crate::pick::Linear,
    ) -> Snap {
        let mut s = self.infer.infer_linear(m, reference, self.lock, plane, linear);
        // Model Info › Units › Enable length snapping: along a direction from the reference
        // point, the distance goes by the snapping length.
        if let Some(r) = reference
            && matches!(s.kind, SnapKind::Axis(_) | SnapKind::Parallel | SnapKind::Perpendicular)
        {
            let v = s.pos - r;
            let len = v.length();
            let snapped = esq_core::units::snap_length(len);
            if len > 1e-9 && snapped > 0.0 && (snapped - len).abs() > 1e-12 {
                s.pos = r + v * (snapped / len);
            }
        }
        s
    }

    pub fn to_local(&self, p: DVec3) -> DVec3 {
        self.doc.ctx_xform().inverse().transform_point3(p)
    }

    /// World transform `m` expressed in the active context's coordinates.
    pub fn local_xform(&self, m: DMat4) -> DMat4 {
        let x = self.doc.ctx_xform();
        x.inverse() * m * x
    }

    /// Ground plane of the active context.
    pub fn ground(&self) -> Plane {
        let a = self.infer.axes;
        Plane::from_point_normal(a.transform_point3(DVec3::ZERO), a.col(2).truncate())
    }

    pub fn axis(&self, i: usize) -> DVec3 {
        self.infer.axes.col(i).truncate().normalize()
    }

    /// Drawing plane for a click: the face under the cursor, else the ground.
    pub fn plane_at(&self, s: &Snap) -> Plane {
        match &s.face {
            Some((_, n))
                if matches!(s.kind, SnapKind::OnFace | SnapKind::Endpoint | SnapKind::Midpoint | SnapKind::OnEdge) =>
            {
                Plane::from_point_normal(s.pos, *n)
            }
            _ => {
                let g = self.ground();
                Plane::from_point_normal(s.pos, g.n)
            }
        }
    }

    /// In-plane axes aligned with the context axes when possible (u horizontal).
    pub fn plane_axes(&self, n: DVec3) -> (DVec3, DVec3) {
        let z = self.axis(2);
        if n.dot(z).abs() > 1.0 - 1e-6 {
            (self.axis(0), self.axis(1))
        } else if n.dot(z).abs() < 1e-6 {
            let u = z.cross(n).normalize();
            (u, z)
        } else {
            basis_for(n)
        }
    }

    pub fn len(&self, v: f64) -> String {
        format_length(v, self.units)
    }

    pub fn parse_len(&self, s: &str) -> Result<f64, String> {
        parse_length(s, self.units).ok_or_else(|| format!("Longueur invalide : « {s} »"))
    }
}

/// Splits "a;b" (or "a,b" when no decimal comma is possible).
pub fn split_values(s: &str) -> Vec<String> {
    if s.contains(';') { s.split(';').map(|x| x.trim().to_string()).collect() } else { vec![s.trim().to_string()] }
}

/// Distinguishes a click from a press-drag-release, which tools treat as two clicks.
#[derive(Default)]
pub struct ClickDrag {
    down: Option<DVec2>,
    armed: bool,
}

impl ClickDrag {
    pub fn press(&mut self, m: DVec2, started: bool) {
        self.down = Some(m);
        self.armed = started;
    }

    /// True if this release ends a drag that should count as the second click.
    pub fn release(&mut self, m: DVec2) -> bool {
        // Preferences › Drawing › Click style: "auto detect" ends the operation on a release
        // after a drag, "click-drag-release" on any release, "click-move-click" never.
        let dragged = self.down.is_some_and(|d| d.distance(m) > 6.0);
        let r = self.armed
            && match crate::settings::click_style() {
                crate::settings::ClickStyle::AutoDetect => dragged,
                crate::settings::ClickStyle::ClickDragRelease => true,
                crate::settings::ClickStyle::ClickMoveClick => false,
            };
        self.down = None;
        self.armed = false;
        r
    }
}

pub trait Tool {
    fn id(&self) -> ToolId;
    fn status(&self) -> String;
    fn vcb_label(&self) -> &'static str {
        ""
    }
    /// Live value shown in the Measurements box.
    fn vcb_value(&self, _units: Units) -> String {
        String::new()
    }
    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2);
    fn press(&mut self, _cx: &mut ToolCx, _m: DVec2) {}
    fn release(&mut self, _cx: &mut ToolCx, _m: DVec2) {}
    fn double_click(&mut self, _cx: &mut ToolCx, _m: DVec2) {}
    fn triple_click(&mut self, _cx: &mut ToolCx, _m: DVec2) {}
    /// Ctrl pressed on its own: copy / keep-base toggles.
    fn ctrl_tap(&mut self, _cx: &mut ToolCx) {}
    /// Alt pressed on its own: auto-fold toggle (Move).
    fn alt_tap(&mut self, _cx: &mut ToolCx) {}
    /// Shift pressed and released on its own (Classifier: make unique and assign; Position
    /// Texture: Fixed Pins; Eraser, Scale: mode toggles).
    fn shift_tap(&mut self, _cx: &mut ToolCx) {}
    /// Ctrl '+' / Ctrl '-': one more or one less segment (circle, polygon, arc).
    fn step(&mut self, _cx: &mut ToolCx, _delta: i32) {}
    fn vcb(&mut self, _cx: &mut ToolCx, _text: &str) -> Result<(), String> {
        Err("Rien à mesurer pour l'instant".into())
    }
    fn cancel(&mut self, cx: &mut ToolCx) {
        cx.doc.set_preview(None);
    }
    /// True while the tool is in the middle of an operation (Escape cancels it).
    fn busy(&self) -> bool {
        false
    }
    fn overlay(&self, _o: &mut Overlay) {}
    /// Snap to show (marker + tooltip).
    fn snap(&self) -> Option<&Snap> {
        None
    }

    // Match Photo (the other tools keep these defaults).

    /// Paints something behind the 3D view; true if it did, which makes the view transparent.
    fn backdrop(&mut self, _o: &Overlay) -> bool {
        false
    }
    /// The view this tool imposes (the viewport size is given), if any.
    fn camera(&mut self, _viewport: DVec2) -> Option<Camera> {
        None
    }
    /// Mouse wheel over the viewport; true if the tool used it.
    fn scroll(&mut self, _cx: &mut ToolCx, _m: DVec2, _delta: f64) -> bool {
        false
    }
    /// Floating controls drawn over the viewport `rect`.
    fn panel(&mut self, _ctx: &egui::Context, _rect: egui::Rect, _cx: &mut ToolCx) {}
    /// `Some` once when the tool is done: the index of the scene it made, or `None` if cancelled.
    fn finished(&mut self) -> Option<Option<usize>> {
        None
    }
    /// The tool is over and gives way to Select (SketchUp's "pop tool": Divide, Photo Point…).
    fn done(&self) -> bool {
        false
    }

    // Ruby tools (`tools/ruby.rs`); the other tools keep these defaults.

    /// The state shared with Ruby when this is a tool written in Ruby.
    fn ruby(&self) -> Option<std::rc::Rc<std::cell::RefCell<ruby::RubyShared>>> {
        None
    }
    /// Label of the Measurements box (a Ruby tool sets it with `Sketchup.vcb_label=`).
    fn vcb_label_text(&self) -> String {
        self.vcb_label().to_string()
    }
    /// Enter with an empty Measurements box.
    fn enter(&mut self, _cx: &mut ToolCx) {}
    /// A key went down or up (Windows virtual-key code, repeat count).
    fn key(&mut self, _cx: &mut ToolCx, _vk: u32, _down: bool, _repeat: u32) {}
    /// Right (2) or middle (3) button pressed, released or double-clicked over the viewport.
    fn button(&mut self, _cx: &mut ToolCx, _m: DVec2, _button: u8, _what: ButtonEvent) {}
    /// True if the tool takes the middle button (then it does not orbit).
    fn wants_middle(&self) -> bool {
        false
    }
    /// True if the tool hears the keys (`key`, `enter`) like a Ruby tool, arrows included
    /// (then they do not lock an axis): Image Igloo.
    fn wants_keys(&self) -> bool {
        false
    }

    // Position Texture and Sample Material (`tools/postex.rs`, `tools/sample.rs`).

    /// The tool hands over to another one (positioning done, a material sampled), once.
    fn switch_to(&mut self) -> Option<ToolId> {
        None
    }
    /// Escape, before the usual handling: true if the tool took it.
    fn escape(&mut self, _cx: &mut ToolCx) -> bool {
        false
    }
    /// A key the tool uses itself (the arrows nudge a texture): true if it took it.
    fn own_key(&mut self, _cx: &mut ToolCx, _key: egui::Key, _mods: egui::Modifiers) -> bool {
        false
    }
    /// The tool's own right-click menu, if it has one (it replaces the usual entries).
    fn menu(&mut self, _ui: &mut egui::Ui, _cx: &mut ToolCx) -> bool {
        false
    }
}

/// What happened to a mouse button (see [`Tool::button`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonEvent {
    Down,
    Up,
    DoubleClick,
}

/// Orbit / pan / zoom with the left button; handled by the app, which owns the camera.
pub struct CameraTool(pub ToolId);

impl Tool for CameraTool {
    fn id(&self) -> ToolId {
        self.0
    }
    fn status(&self) -> String {
        match self.0 {
            // CCameraOrbitTool.
            ToolId::Orbit => "Faites glisser pour naviguer. Maj = panoramique, Ctrl =  suspend la gravité.".into(),
            // CCameraDollyTool (the hand: "dolly" moves the camera, "pan" turns it).
            ToolId::Pan => "Déplacez le curseur dans la direction voulue pour effectuer un panoramique.".into(),
            // CCameraZoomTool.
            _ => "Faites glisser le curseur  Vers le haut = zoom avant ; vers le bas = zoom arrière. \
                  Touche Maj pour modifier le champ angulaire."
                .into(),
        }
    }
    fn mouse_move(&mut self, _cx: &mut ToolCx, _m: DVec2) {}
}

/// 2D drawing over the 3D view.
pub struct Overlay<'a> {
    pub painter: egui::Painter,
    pub origin: Pos2,
    pub cam: &'a Camera,
    pub doc: &'a Document,
    pub units: Units,
}

pub fn c32(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

impl Overlay<'_> {
    pub fn screen(&self, p: DVec3) -> Option<Pos2> {
        self.cam.project(p).map(|s| self.origin + egui::vec2(s.x as f32, s.y as f32))
    }

    pub fn line(&self, a: DVec3, b: DVec3, color: Color32, width: f32) {
        if let (Some(a), Some(b)) = (self.screen(a), self.screen(b)) {
            self.painter.line_segment([a, b], Stroke::new(width, color));
        }
    }

    pub fn dashed(&self, a: DVec3, b: DVec3, color: Color32, width: f32) {
        if let (Some(a), Some(b)) = (self.screen(a), self.screen(b)) {
            self.painter.extend(egui::Shape::dashed_line(&[a, b], Stroke::new(width, color), 6.0, 4.0));
        }
    }

    pub fn polyline(&self, pts: &[DVec3], closed: bool, color: Color32, width: f32) {
        let n = pts.len();
        let segs = if closed { n } else { n.saturating_sub(1) };
        for i in 0..segs {
            self.line(pts[i], pts[(i + 1) % n], color, width);
        }
    }

    pub fn fill_polygon(&self, pts: &[DVec3], color: Color32) {
        let s: Option<Vec<Pos2>> = pts.iter().map(|p| self.screen(*p)).collect();
        if let Some(s) = s {
            let mut mesh = egui::Mesh::default();
            for p in &s {
                mesh.colored_vertex(*p, color);
            }
            for i in 1..s.len().saturating_sub(1) {
                mesh.add_triangle(0, i as u32, (i + 1) as u32);
            }
            self.painter.add(egui::Shape::mesh(mesh));
        }
    }

    /// Highlights a face of the active context.
    pub fn fill_face(&self, f: FaceId, color: Color32) {
        let ents = self.doc.ents();
        if !ents.faces.contains_key(f) {
            return;
        }
        let x = self.doc.ctx_xform();
        let mut mesh = egui::Mesh::default();
        for t in face_triangles(ents, f) {
            let s: Option<Vec<Pos2>> = t.iter().map(|p| self.screen(x.transform_point3(*p))).collect();
            if let Some(s) = s {
                let base = mesh.vertices.len() as u32;
                for p in s {
                    mesh.colored_vertex(p, color);
                }
                mesh.add_triangle(base, base + 1, base + 2);
            }
        }
        self.painter.add(egui::Shape::mesh(mesh));
    }

    pub fn marker(&self, s: &Snap) {
        let Some(p) = self.screen(s.pos) else { return };
        let c = c32(s.kind.color());
        match s.kind {
            SnapKind::Endpoint | SnapKind::Origin => {
                self.painter.circle(p, 5.0, c, Stroke::new(1.5, Color32::WHITE));
            }
            SnapKind::Midpoint => {
                self.painter.circle(p, 5.0, c, Stroke::new(1.5, Color32::WHITE));
            }
            SnapKind::OnEdge => {
                self.painter.rect_filled(egui::Rect::from_center_size(p, egui::vec2(8.0, 8.0)), 1.0, c);
            }
            SnapKind::OnFace => {
                let r = egui::Rect::from_center_size(p, egui::vec2(9.0, 9.0));
                self.painter.rect_stroke(r, 1.0, Stroke::new(2.0, c), egui::StrokeKind::Middle);
            }
            SnapKind::Handle(..) => {
                let r = egui::Rect::from_center_size(p, egui::vec2(10.0, 10.0));
                self.painter.rect_filled(r, 0.0, c);
                self.painter.rect_stroke(r, 0.0, Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Middle);
            }
            _ => {
                self.painter.circle_filled(p, 3.0, Color32::from_gray(40));
            }
        }
        if let Some((a, b, col)) = s.guide {
            self.dashed(a, b, c32(col), 2.0);
        }
        let label = s.kind.label();
        if !label.is_empty() {
            self.tooltip(p + egui::vec2(14.0, 14.0), label);
        }
    }

    pub fn tooltip(&self, at: Pos2, text: &str) {
        let galley =
            self.painter.layout_no_wrap(text.to_string(), egui::FontId::proportional(13.0), Color32::from_gray(20));
        let r = egui::Rect::from_min_size(at, galley.size()).expand(4.0);
        self.painter.rect_filled(r, 3.0, Color32::from_rgb(255, 252, 225));
        self.painter.rect_stroke(r, 3.0, Stroke::new(1.0, Color32::from_gray(150)), egui::StrokeKind::Inside);
        self.painter.galley(at, galley, Color32::BLACK);
    }

    pub fn text_at(&self, p: DVec3, text: &str) {
        if let Some(s) = self.screen(p) {
            self.tooltip(s + egui::vec2(10.0, -26.0), text);
        }
    }
}

/// Points of a regular polygon/circle in a plane.
pub fn circle_points(center: DVec3, n: DVec3, start: DVec3, radius: f64, segments: usize) -> Vec<DVec3> {
    let u = (start - n * start.dot(n)).normalize_or(basis_for(n).0);
    let v = n.cross(u);
    (0..segments)
        .map(|i| {
            let a = i as f64 / segments as f64 * std::f64::consts::TAU;
            center + (u * a.cos() + v * a.sin()) * radius
        })
        .collect()
}
