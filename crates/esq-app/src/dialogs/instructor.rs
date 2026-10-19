//! Window › Instructor: help on the active tool, updated as the tool changes, laid out as
//! SketchUp's (the tool, what it does, how it works, its modifier keys), with the tool's
//! present prompt. The texts are Esquisse's own, written after its tools.

use eframe::egui::{self, RichText};

use crate::app::EsquisseApp;
use crate::tools::ToolId;

pub struct Help {
    pub what: &'static str,
    pub steps: &'static [&'static str],
    pub keys: &'static [(&'static str, &'static str)],
}

const LOCKS: (&str, &str) = ("Flèches", "verrouiller une direction d'axe (droite : rouge, gauche : vert, haut : bleu)");
const SHIFT_LOCK: (&str, &str) = ("Maj (maintenue)", "verrouiller l'inférence en cours");

/// The help of a tool.
pub fn help(t: ToolId) -> Help {
    let h = |what, steps, keys| Help { what, steps, keys };
    match t {
        ToolId::Select => h(
            "Sélectionner des entités pour les modifier avec d'autres outils ou commandes.",
            &[
                "Cliquez sur une entité pour la sélectionner.",
                "Glissez de gauche à droite pour sélectionner ce qui est entièrement dans le rectangle, de droite à gauche pour ce qu'il touche.",
                "Double-cliquez sur une face pour la prendre avec ses arêtes, triple-cliquez pour tout ce qui y est relié ; double-cliquez sur un groupe pour l'ouvrir.",
            ],
            &[
                ("Ctrl", "ajouter à la sélection"),
                ("Maj", "ajouter ou retirer"),
                ("Ctrl+Maj", "retirer de la sélection"),
            ],
        ),
        ToolId::Line => h(
            "Tracer des arêtes ; un contour fermé et plan devient une face.",
            &[
                "Cliquez pour placer le point de départ.",
                "Déplacez le curseur et cliquez pour finir le segment ; le suivant part de là.",
                "Tapez une longueur puis Entrée pour un segment précis. Échap arrête la ligne.",
            ],
            &[LOCKS, SHIFT_LOCK],
        ),
        ToolId::Freehand => h(
            "Dessiner une courbe à main levée.",
            &[
                "Appuyez et glissez pour tracer la courbe.",
                "Relâchez pour la terminer ; refermée et plane, elle donne une face.",
            ],
            &[LOCKS],
        ),
        ToolId::Rect | ToolId::RotRect => h(
            "Dessiner un rectangle (une face et ses quatre arêtes).",
            &[
                "Cliquez sur le premier coin (sur une face pour dessiner dans son plan).",
                "Cliquez sur le coin opposé, ou tapez « largeur;hauteur » puis Entrée.",
                "Le rectangle orienté se dessine en trois clics : un côté, puis la largeur.",
            ],
            &[("Ctrl", "dessiner depuis le centre"), LOCKS],
        ),
        ToolId::Circle | ToolId::Polygon => h(
            "Dessiner un cercle ou un polygone régulier.",
            &[
                "Cliquez sur le centre (sur une face pour dessiner dans son plan).",
                "Cliquez pour fixer le rayon, ou tapez-le puis Entrée.",
                "Tapez « 24s » pour changer le nombre de côtés.",
            ],
            &[("Ctrl + / Ctrl -", "un côté de plus ou de moins"), ("Ctrl", "polygone inscrit ou circonscrit"), LOCKS],
        ),
        ToolId::Arc | ToolId::ArcCenter | ToolId::Arc3 | ToolId::Pie => h(
            "Dessiner un arc (ou une portion de cercle).",
            &[
                "Cliquez sur le début de l'arc (ou sur son centre).",
                "Cliquez sur la fin de l'arc, ou tapez une longueur.",
                "Déplacez pour régler la flèche ou l'angle et cliquez, ou tapez la valeur.",
            ],
            &[("Alt", "verrouiller la tangente"), ("Ctrl + / Ctrl -", "un côté de plus ou de moins"), LOCKS],
        ),
        ToolId::PushPull => h(
            "Pousser ou tirer une face pour créer ou retirer du volume.",
            &[
                "Cliquez sur une face (ou sélectionnez-la d'abord).",
                "Déplacez le curseur et cliquez pour finir, ou tapez une distance.",
                "Double-cliquez sur une autre face pour répéter la dernière distance.",
            ],
            &[("Ctrl", "garder la face de départ"), ("Alt", "étirer les faces voisines")],
        ),
        ToolId::FollowMe => h(
            "Extruder une face le long d'un chemin.",
            &[
                "Sélectionnez le chemin (des arêtes, ou une face pour son contour).",
                "Cliquez sur la face à extruder (le profil).",
            ],
            &[("Alt", "suivre le contour de la face cliquée")],
        ),
        ToolId::Move => h(
            "Déplacer, copier ou étirer des entités.",
            &[
                "Cliquez sur un point de ce qu'il faut déplacer (la sélection, ou l'entité sous le curseur).",
                "Déplacez et cliquez pour finir, ou tapez une distance.",
                "Après une copie, tapez « 3x » ou « /3 » pour un réseau.",
            ],
            &[("Ctrl", "copier"), ("Alt", "plier automatiquement"), LOCKS, SHIFT_LOCK],
        ),
        ToolId::Rotate => h(
            "Faire pivoter des entités autour d'un point.",
            &[
                "Cliquez pour placer le centre du rapporteur (sur une face pour pivoter dans son plan).",
                "Cliquez sur le point de départ de la rotation.",
                "Déplacez et cliquez pour finir, ou tapez un angle en degrés.",
            ],
            &[("Ctrl", "faire pivoter une copie"), LOCKS],
        ),
        ToolId::Scale => h(
            "Mettre à l'échelle la sélection.",
            &[
                "Sélectionnez ce qu'il faut mettre à l'échelle ; des poignées l'entourent.",
                "Cliquez sur une poignée et déplacez-la, puis cliquez, ou tapez un facteur.",
            ],
            &[("Ctrl", "autour du centre"), ("Maj", "uniformément")],
        ),
        ToolId::Offset => h(
            "Décaler le contour d'une face, ou des arêtes coplanaires.",
            &["Cliquez sur une face (ou sélectionnez des arêtes).", "Déplacez et cliquez, ou tapez la distance."],
            &[],
        ),
        ToolId::Eraser => h(
            "Effacer des entités.",
            &[
                "Cliquez sur une entité pour l'effacer, ou glissez sur plusieurs : elles disparaissent quand le bouton est relâché.",
            ],
            &[
                ("Ctrl", "adoucir et lisser les arêtes"),
                ("Maj", "masquer les arêtes et objets"),
                ("Ctrl+Maj", "ne plus adoucir, révéler"),
            ],
        ),
        ToolId::Paint => h(
            "Appliquer une matière aux faces, groupes et composants.",
            &["Choisissez une matière dans le panneau Matières.", "Cliquez sur ce qu'il faut peindre."],
            &[
                ("Ctrl", "faces connectées de même matière"),
                ("Maj", "toutes les faces de même matière"),
                ("Ctrl+Maj", "même matière dans le même objet"),
                ("Alt", "prélever la matière sous le curseur"),
            ],
        ),
        ToolId::Tape => h(
            "Mesurer des distances et créer des lignes et points de construction.",
            &[
                "Cliquez sur le point de départ de la mesure.",
                "Cliquez sur le point d'arrivée : la distance s'affiche ; partant d'une arête, une ligne de construction parallèle est créée.",
                "Tapez une valeur pour placer le guide, ou une longueur pour redimensionner le modèle.",
            ],
            &[("Ctrl", "mesurer sans créer de guide"), LOCKS],
        ),
        ToolId::Protractor => h(
            "Mesurer des angles et créer des lignes de construction inclinées.",
            &[
                "Cliquez sur le sommet de l'angle (sur une face pour mesurer dans son plan).",
                "Cliquez sur un point de la ligne de base.",
                "Cliquez pour placer la ligne de construction, ou tapez l'angle (degrés, ou pente 1:2).",
            ],
            &[("Ctrl", "mesurer sans créer de guide"), LOCKS],
        ),
        ToolId::Dimension => h(
            "Coter une longueur entre deux points ou une arête.",
            &[
                "Cliquez sur le point de départ (ou sur une arête).",
                "Cliquez sur le point d'arrivée.",
                "Écartez la ligne de cote et cliquez pour la placer.",
            ],
            &[],
        ),
        ToolId::Text => h(
            "Ajouter une étiquette de texte.",
            &[
                "Cliquez sur une entité pour un texte avec légende, ou dans le vide pour un texte à l'écran.",
                "Cliquez pour placer le texte, puis tapez-le.",
            ],
            &[],
        ),
        ToolId::AxesTool => h(
            "Déplacer et orienter les axes de dessin.",
            &[
                "Cliquez pour placer l'origine.",
                "Cliquez pour orienter l'axe rouge.",
                "Cliquez pour orienter l'axe vert.",
            ],
            &[],
        ),
        ToolId::SectionPlane => h(
            "Placer un plan de section qui coupe le modèle.",
            &[
                "Placez le curseur sur une face : le plan s'aligne sur elle.",
                "Cliquez pour placer le plan ; il devient la coupe active.",
            ],
            &[("Maj", "garder l'orientation"), LOCKS],
        ),
        ToolId::Orbit => h(
            "Tourner la vue autour du modèle.",
            &[
                "Glissez pour tourner autour du point cliqué.",
                "La molette garde son rôle de zoom ; le bouton du milieu orbite avec n'importe quel outil.",
            ],
            &[("Maj", "panoramique")],
        ),
        ToolId::Pan => h("Faire glisser la vue.", &["Glissez dans la direction voulue."], &[]),
        ToolId::Zoom => h(
            "Rapprocher ou éloigner la caméra.",
            &[
                "Glissez vers le haut pour zoomer, vers le bas pour dézoomer.",
                "Tapez un angle (35) ou une focale (50mm) pour le champ de vision.",
            ],
            &[("Maj", "changer le champ de vision")],
        ),
        ToolId::ZoomWindow => {
            h("Remplir la vue avec une zone.", &["Glissez un rectangle autour de ce qui doit remplir la vue."], &[])
        }
        ToolId::Fov => h(
            "Changer le champ de vision de la caméra.",
            &["Glissez vers le haut ou le bas, ou tapez un angle (35) ou une focale (50mm)."],
            &[],
        ),
        ToolId::PositionCamera => h(
            "Placer la caméra à hauteur d'yeux.",
            &["Cliquez où se tenir, ou appuyez et glissez vers ce qu'il faut regarder.", "Tapez la hauteur des yeux."],
            &[],
        ),
        ToolId::LookAround => h(
            "Tourner la tête depuis la position de la caméra.",
            &["Glissez pour regarder autour.", "Tapez une hauteur des yeux."],
            &[],
        ),
        ToolId::Walk => h(
            "Marcher dans le modèle.",
            &["Cliquez et glissez : vers le haut pour avancer, sur les côtés pour tourner."],
            &[("Ctrl", "courir"), ("Maj", "monter ou se décaler")],
        ),
        ToolId::Lasso => h(
            "Sélectionner en entourant à main levée.",
            &[
                "Glissez autour des entités à sélectionner ; dans un sens le lasso prend ce qui est entièrement dedans, dans l'autre ce qu'il touche (Préférences › Dessin).",
            ],
            &[("Ctrl", "ajouter"), ("Maj", "ajouter ou retirer"), ("Ctrl+Maj", "retirer")],
        ),
        ToolId::TagTool => h(
            "Donner une étiquette aux entités.",
            &["Choisissez l'étiquette active.", "Cliquez sur les entités à étiqueter."],
            &[("Alt", "prendre l'étiquette d'une entité")],
        ),
        ToolId::Flip => h(
            "Retourner la sélection en miroir.",
            &["Sélectionnez ce qu'il faut retourner.", "Cliquez sur un des trois plans."],
            &[],
        ),
        ToolId::North => h(
            "Indiquer le nord du modèle (pour les ombres).",
            &["Cliquez pour l'origine, puis dans la direction du nord."],
            &[],
        ),
        ToolId::OuterShell
        | ToolId::SolidIntersect
        | ToolId::SolidUnion
        | ToolId::SolidSubtract
        | ToolId::SolidTrim
        | ToolId::SolidSplit => h(
            "Combiner des solides (groupes ou composants fermés).",
            &[
                "Cliquez sur le premier solide.",
                "Cliquez sur le second : le résultat remplace (ou découpe) les solides.",
            ],
            &[],
        ),
        ToolId::FromContours | ToolId::FromScratch => h(
            "Créer un terrain (Bac à sable).",
            &[
                "À partir des contours : sélectionnez les courbes de niveau puis lancez l'outil.",
                "À partir de zéro : cliquez deux coins puis la profondeur de la grille.",
            ],
            &[],
        ),
        ToolId::Smoove | ToolId::Stamp | ToolId::Drape | ToolId::AddDetail | ToolId::FlipEdge => h(
            "Modeler un terrain (Bac à sable).",
            &["Ouvrez le groupe du terrain, puis cliquez où modeler, tamponner ou projeter."],
            &[],
        ),
        ToolId::Component => h("Placer un composant.", &["Cliquez pour placer le composant. Échap pour annuler."], &[]),
        _ => h("", &[], &[]),
    }
}

/// The Instructor panel.
pub fn show(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let id = app.tool.id();
    let hp = help(id);
    let name = id.label();
    let title = if name.starts_with("Outil") { name.to_string() } else { format!("Outil {name}") };
    ui.label(RichText::new(title).strong().size(15.0));
    if !hp.what.is_empty() {
        ui.label(format!("•  {}", hp.what));
    }
    if !hp.steps.is_empty() {
        ui.add_space(4.0);
        ui.label(RichText::new("Fonctionnement de l'outil").strong());
        for (i, s) in hp.steps.iter().enumerate() {
            ui.label(format!("{}.  {s}", i + 1));
        }
    }
    if !hp.keys.is_empty() {
        ui.add_space(4.0);
        ui.label(RichText::new("Touches de modification").strong());
        for (k, v) in hp.keys {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(*k).strong().monospace());
                ui.label(format!("= {v}"));
            });
        }
    }
    ui.add_space(4.0);
    ui.label(RichText::new("Maintenant").strong());
    ui.label(RichText::new(app.tool.status()).italics());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_drawing_and_modifying_tools_are_explained() {
        for t in [
            ToolId::Line,
            ToolId::Rect,
            ToolId::PushPull,
            ToolId::Move,
            ToolId::Rotate,
            ToolId::Eraser,
            ToolId::Paint,
            ToolId::Tape,
        ] {
            let h = help(t);
            assert!(!h.what.is_empty() && !h.steps.is_empty(), "{t:?}");
        }
        assert!(help(ToolId::Move).keys.iter().any(|(k, _)| *k == "Ctrl"));
    }
}
