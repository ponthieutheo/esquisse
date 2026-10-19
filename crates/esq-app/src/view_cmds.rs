//! The display commands of SketchUp's `commands.json` that have no menu entry (they live on
//! toolbars, in the command search and in the Styles panel): edge style options, edge colour,
//! background, transparency, sun shading, watermarks, colour by tag.

use eframe::egui;

use crate::app::EsquisseApp;
use crate::style::EdgeColor;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewCmd {
    /// `view/edge style/dashes`
    Dashes,
    /// `view/edge style/endpoints`
    Endpoints,
    /// `view/edge style/jitter`
    Jitter,
    /// `view/rendering/edge/all same`, `by material`, `by axis`
    EdgeColor(EdgeColor),
    /// `view/rendering/ground`
    Ground,
    /// `view/rendering/sky`
    Sky,
    /// `view/rendering/transparent materials`
    TransparentMaterials,
    /// `view/rendering/use sun for shading`
    UseSunForShading,
    /// `view watermarks`
    Watermarks,
    /// `color tags`
    ColorByTag,
    /// `show new tags`
    ShowNewTags,
    /// `display crosshairs`
    Crosshairs,
    /// `use environment as skydome`
    EnvSkydome,
    /// `use environment for reflections`
    EnvReflections,
    /// `view/grid`
    Grid,
    /// `enable grid snapping`
    GridSnapping,
}

impl ViewCmd {
    /// The commands, for the command search.
    pub const ALL: [ViewCmd; 18] = [
        ViewCmd::Dashes,
        ViewCmd::Endpoints,
        ViewCmd::Jitter,
        ViewCmd::EdgeColor(EdgeColor::AllSame),
        ViewCmd::EdgeColor(EdgeColor::ByMaterial),
        ViewCmd::EdgeColor(EdgeColor::ByAxis),
        ViewCmd::Ground,
        ViewCmd::Sky,
        ViewCmd::TransparentMaterials,
        ViewCmd::UseSunForShading,
        ViewCmd::Watermarks,
        ViewCmd::ColorByTag,
        ViewCmd::ShowNewTags,
        ViewCmd::Crosshairs,
        ViewCmd::EnvSkydome,
        ViewCmd::EnvReflections,
        ViewCmd::Grid,
        ViewCmd::GridSnapping,
    ];

    /// SketchUp's titles (`commands.json`).
    pub fn label(self) -> &'static str {
        match self {
            ViewCmd::Dashes => "Afficher les tirets",
            ViewCmd::Endpoints => "Voir les extrémités",
            ViewCmd::Jitter => "Voir les lignes floues",
            ViewCmd::EdgeColor(EdgeColor::AllSame) => "Colorier toutes les arêtes de la même manière",
            ViewCmd::EdgeColor(EdgeColor::ByMaterial) => "Colorier les arêtes en fonction du matériau",
            ViewCmd::EdgeColor(EdgeColor::ByAxis) => "Colorier les arêtes en fonction de l'axe",
            ViewCmd::Ground => "Voir le sol",
            ViewCmd::Sky => "Voir le ciel",
            ViewCmd::TransparentMaterials => "Activer/Désactiver la transparence",
            ViewCmd::UseSunForShading => "Utiliser le soleil pour les ombres",
            ViewCmd::Watermarks => "Afficher les filigranes",
            ViewCmd::ColorByTag => "Colorer par étiquette",
            ViewCmd::ShowNewTags => "Afficher les nouvelles étiquettes",
            ViewCmd::Crosshairs => "Voir les réticules",
            ViewCmd::EnvSkydome => "Utiliser l'environnement comme Skydome",
            ViewCmd::EnvReflections => "Utiliser l'environnement pour la réflexion",
            ViewCmd::Grid => "Afficher la grille",
            ViewCmd::GridSnapping => "Mises en place de grilles",
        }
    }

    /// Where the command search shows it.
    pub fn path(self) -> &'static str {
        match self {
            ViewCmd::Dashes | ViewCmd::Endpoints | ViewCmd::Jitter | ViewCmd::EdgeColor(_) => {
                "Affichage › Style d'arêtes"
            }
            ViewCmd::Ground | ViewCmd::Sky | ViewCmd::Watermarks => "Affichage › Arrière-plan",
            ViewCmd::TransparentMaterials | ViewCmd::UseSunForShading => "Affichage › Style de faces",
            ViewCmd::ColorByTag | ViewCmd::ShowNewTags => "Fenêtre › Étiquettes",
            ViewCmd::Crosshairs => "Préférences › Dessin",
            ViewCmd::EnvSkydome | ViewCmd::EnvReflections => "Fenêtre › Environnements",
            ViewCmd::Grid | ViewCmd::GridSnapping => "Affichage › Grille",
        }
    }
}

impl EsquisseApp {
    pub fn view_checked(&self, c: ViewCmd) -> bool {
        let s = &self.style;
        match c {
            ViewCmd::Dashes => s.dashes,
            ViewCmd::Endpoints => s.endpoints,
            ViewCmd::Jitter => s.jitter,
            ViewCmd::EdgeColor(m) => s.edge_color_mode == m,
            ViewCmd::Ground => s.ground,
            ViewCmd::Sky => s.sky,
            ViewCmd::TransparentMaterials => s.transparency,
            ViewCmd::UseSunForShading => self.doc.model.shadow.use_sun_for_shading,
            ViewCmd::Watermarks => s.watermarks,
            ViewCmd::ColorByTag => s.color_by_tag,
            ViewCmd::ShowNewTags => crate::tags_panel::show_new_tags(&self.doc.model),
            ViewCmd::Crosshairs => crate::crosshairs::shown(),
            ViewCmd::EnvSkydome => crate::environments::current(&self.doc.model).is_some_and(|e| e.skydome),
            ViewCmd::EnvReflections => crate::environments::current(&self.doc.model).is_some_and(|e| e.reflections),
            ViewCmd::Grid => s.modeling_grid,
            ViewCmd::GridSnapping => crate::model_grid::settings(&self.doc.model).snap,
        }
    }

    /// The environment commands act on the current environment: there must be one.
    pub fn view_enabled(&self, c: ViewCmd) -> bool {
        match c {
            ViewCmd::EnvSkydome | ViewCmd::EnvReflections => crate::environments::current(&self.doc.model).is_some(),
            _ => true,
        }
    }

    pub fn run_view(&mut self, _ctx: &egui::Context, c: ViewCmd) {
        let s = &mut self.style;
        match c {
            ViewCmd::Dashes => s.dashes = !s.dashes,
            ViewCmd::Endpoints => s.endpoints = !s.endpoints,
            ViewCmd::Jitter => s.jitter = !s.jitter,
            ViewCmd::EdgeColor(m) => s.edge_color_mode = m,
            ViewCmd::Ground => s.ground = !s.ground,
            ViewCmd::Sky => s.sky = !s.sky,
            ViewCmd::TransparentMaterials => s.transparency = !s.transparency,
            ViewCmd::Watermarks => s.watermarks = !s.watermarks,
            ViewCmd::ColorByTag => s.color_by_tag = !s.color_by_tag,
            ViewCmd::ShowNewTags => self.toggle_show_new_tags(),
            ViewCmd::Crosshairs => crate::crosshairs::set_shown(!crate::crosshairs::shown()),
            ViewCmd::Grid => s.modeling_grid = !s.modeling_grid,
            ViewCmd::GridSnapping => {
                let mut g = crate::model_grid::settings(&self.doc.model);
                g.snap = !g.snap;
                crate::model_grid::save(&mut self.doc.model, &g);
            }
            // As SketchUp: the current environment's setting changes at once (not undoable).
            ViewCmd::EnvSkydome | ViewCmd::EnvReflections => {
                if let Some(mut e) = crate::environments::current(&self.doc.model) {
                    if c == ViewCmd::EnvSkydome {
                        e.skydome = !e.skydome;
                    } else {
                        e.reflections = !e.reflections;
                    }
                    crate::environments::update(&mut self.doc.model, &e);
                }
            }
            ViewCmd::UseSunForShading => {
                let on = !self.doc.model.shadow.use_sun_for_shading;
                self.doc.commit("Utiliser le soleil pour l'ombrage", |m, _| m.shadow.use_sun_for_shading = on);
            }
        }
        self.doc.rev += 1;
    }

    /// View › Fog. Shown for the first time, the fog spans the model as seen now: it starts
    /// where the model begins and is complete where it ends (as SketchUp sets it).
    pub fn toggle_fog(&mut self) {
        let s = &mut self.style;
        s.fog = !s.fog;
        if s.fog && s.fog_start < 0.0 && s.fog_end < 0.0 {
            let (near, far) = crate::fog::model_range(&self.doc.model, &self.cam);
            (s.fog_start, s.fog_end) = (near, far);
        }
        self.doc.rev += 1;
    }
}
