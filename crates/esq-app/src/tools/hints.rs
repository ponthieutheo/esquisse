//! Pieces of SketchUp's status-bar messages shared by several tools, in SketchUp's French
//! (`resources/fr/Localizable.strings`). SketchUp builds a message as "what to do | Key = what
//! the key toggles | …" (`"%s | %s"`, `"%s = %s"`; modifier names Ctrl, Alt, Maj).

/// the arrow keys of the drawing tools.
pub const ARROWS_INFERENCE: &str = "Flèches = Activer/Désactiver verrouiller la direction de l'inférence.";
/// Rectangle, freehand.
pub const ARROWS_DRAWING_PLANE: &str = "Flèches = Activer/Désactiver verrouiller le plan de dessin.";
/// Rotate, protractor.
pub const ARROWS_ROTATION_PLANE: &str = "Flèches = Activer/Désactiver verrouiller le plan de rotation.";
/// Arcs, circles, polygons.
pub const SEGMENTS: &str = "Utilisez Ctrl '+' ou Ctrl '-' pour changer le nombre de segments.";
/// Arcs that can follow a tangent.
pub const LOCK_TANGENCY: &str = "Alt = Activer/désactive Verrouiller la tangence.";
