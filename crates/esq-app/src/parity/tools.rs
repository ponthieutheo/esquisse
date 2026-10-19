//! The user-facing tool classes of SketchUp.exe (RTTI `C…Tool`; base classes and the
//! `CTest…Tool` debug tools left out). Fidelity means the modifiers, inferences, VCB inputs
//! and status texts of the class.

use super::Impl::{self, *};
use super::Item;
use crate::commands::Cmd as C;
use crate::tools::ToolId as T;

const fn it(id: &'static str, label: &'static str, imp: Impl) -> Item {
    Item { id, label, imp }
}

pub const TABLE: &[Item] = &[
    it("tool:C3DTextTool", "Texte 3D", Cmd(C::Text3d)),
    it("tool:CAddRayTool", "Outil Point photo (guide depuis une photo adaptée)", Tool(T::PhotoPoint)),
    it("tool:CArc3PointPieTool", "Portion de cercle", Tool(T::Pie)),
    // Read from their messages: CArc3PointTool asks centre, first and second arc points
    // (Arc), CArcTool start, end and bulge (2-Point Arc), CFit3PointArcTool start, second
    // and end points (3-Point Arc).
    it("tool:CArc3PointTool", "Arc (centre)", Tool(T::ArcCenter)),
    it("tool:CArcTool", "Arc par 2 points", Tool(T::Arc)),
    it("tool:CFit3PointArcTool", "Arc par 3 points", Tool(T::Arc3)),
    it("tool:CBooleanDifferenceTool", "Soustraire", Tool(T::SolidSubtract)),
    it("tool:CBooleanIntersectTool", "Intersection", Tool(T::SolidIntersect)),
    it("tool:CBooleanSplitTool", "Diviser", Tool(T::SolidSplit)),
    it("tool:CBooleanTrimTool", "Découper", Tool(T::SolidTrim)),
    it("tool:CBooleanUnionTool", "Union", Tool(T::SolidUnion)),
    it("tool:COuterShellTool", "Enveloppe externe", Tool(T::OuterShell)),
    // In SketchUp, "dolly" moves the camera (the hand, Panoramique) and "pan" turns it (Pivoter).
    it("tool:CCameraDollyTool", "Panoramique (dolly)", Tool(T::Pan)),
    it(
        "tool:CCameraEyeHeightTool",
        "Hauteur des yeux (base de Pivoter et Visite)",
        Feature("tools::camtools::set_eye_height (test: eye_height_typed_in_look_around_and_walk)"),
    ),
    it("tool:CCameraFOVTool", "Champ angulaire", Tool(T::Fov)),
    it("tool:CCameraOrbitTool", "Orbite", Tool(T::Orbit)),
    it("tool:CCameraPanTool", "Pivoter", Tool(T::LookAround)),
    it("tool:CCameraWalkTool", "Visite", Tool(T::Walk)),
    it("tool:CCameraZoomTool", "Zoom", Tool(T::Zoom)),
    it("tool:CCameraZoomWindowTool", "Fenêtre de zoom", Tool(T::ZoomWindow)),
    it("tool:CPositionCameraTool", "Positionner la caméra", Tool(T::PositionCamera)),
    it("tool:CCircleTool", "Cercle", Tool(T::Circle)),
    it("tool:CPolyTool", "Polygone", Tool(T::Polygon)),
    it("tool:CRectangleTool", "Rectangle", Tool(T::Rect)),
    it("tool:CRectangle3PointTool", "Rectangle orienté", Tool(T::RotRect)),
    it("tool:CSketchTool", "Ligne", Tool(T::Line)),
    it("tool:CFreehandTool", "Main levée", Tool(T::Freehand)),
    it("tool:CClassifierTool", "Classificateur", Tool(T::Classifier)),
    it("tool:CComponentTool", "Placer un composant", Tool(T::Component)),
    it("tool:CPasteTool", "Coller (placement)", Cmd(C::Paste)),
    it("tool:CCoordinateTool", "Axes", Tool(T::AxesTool)),
    it("tool:CDimensionTool", "Cotations", Tool(T::Dimension)),
    it("tool:CTextTool", "Texte", Tool(T::Text)),
    it("tool:CDivideLineTool", "Diviser une arête", Tool(T::Divide)),
    it("tool:CEraseTool", "Effacer", Tool(T::Eraser)),
    it("tool:CExtrudeTool", "Suivez-moi", Tool(T::FollowMe)),
    it("tool:CImageTool", "Image (placement)", Tool(T::Image)),
    it("tool:CMatchPhotoTool", "Adapter une photo", Tool(T::MatchPhoto)),
    it("tool:CMeasureTool", "Mètre", Tool(T::Tape)),
    it("tool:CProtractorTool", "Rapporteur", Tool(T::Protractor)),
    it("tool:CMoveTool", "Déplacer", Tool(T::Move)),
    it("tool:CRotateTool", "Faire pivoter", Tool(T::Rotate)),
    it("tool:CScaleTool", "Échelle", Tool(T::Scale)),
    it("tool:COffsetTool", "Décalage", Tool(T::Offset)),
    it("tool:CPushPullTool", "Pousser/Tirer", Tool(T::PushPull)),
    it("tool:FlipTool", "Retourner", Tool(T::Flip)),
    it("tool:CNorthTool", "Vrai Nord", Tool(T::North)),
    it("tool:CPaintTool", "Colorier", Tool(T::Paint)),
    it("tool:CPaintMatchTool", "Échantillon de matière", Tool(T::SampleMaterial)),
    it("tool:CPositionTextureTool", "Positionner la texture", Tool(T::PositionTexture)),
    it("tool:CSectionPlaneTool", "Plan de section", Tool(T::SectionPlane)),
    it("tool:CSelectionTool", "Sélectionner", Tool(T::Select)),
    it("tool:LassoSelectionTool", "Lasso", Tool(T::Lasso)),
    it("tool:CTagTool", "Étiquette", Tool(T::TagTool)),
    it("tool:CShowFaceEdgesTool", "Afficher les arêtes d'une face", Tool(T::ShowFaceEdges)),
    it("tool:CSnapTool", "Points d'accroche (Snaps)", Tool(T::AddSnap)),
    it("tool:LightTool", "Lumières", Tool(T::Light)),
    it("tool:IglooTool", "Dôme d'images (Caméra)", Tool(T::Igloo)),
    it("tool:CRubyTool", "Outil Ruby (plugins)", Tool(T::Ruby)),
    it("tool:CommentTool", "Commentaires (Trimble Connect)", Online),
];
