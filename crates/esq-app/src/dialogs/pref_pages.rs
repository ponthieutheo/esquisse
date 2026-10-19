//! Preferences › Accessibility, Compatibility, Drawing, Files, Graphics, Applications and
//! Workspace. Each setting lives in `settings.rs` (or `toolbars.rs`), and the part of the
//! application it governs reads it there.

use eframe::egui;
use esq_core::glam::{DMat4, DVec3};

use super::{group, name_widget};
use crate::app::EsquisseApp;
use crate::pick::Snap;
use crate::settings::{self, AXIS_COLORS, ClickStyle, PARALLEL_COLOR, TANGENT_COLOR};
use crate::tools::{Overlay, ToolId};

fn color_row(ui: &mut egui::Ui, label: &str, c: &mut [u8; 3]) {
    ui.horizontal(|ui| {
        let r = ui.color_edit_button_srgb(c);
        name_widget(ui, &r, label, Some(&format!("{} {} {}", c[0], c[1], c[2])));
        ui.label(label);
    });
}

/// Preferences › Accessibility: the colours of the axes and of the inferences.
pub fn accessibility_page(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let before = settings::get();
    let mut s = before.clone();
    group(ui, "Couleurs Axe et Direction");
    for (i, name) in ["Axe rouge", "Axe vert", "Axe bleu"].iter().enumerate() {
        color_row(ui, name, &mut s.axis_colors[i]);
    }
    group(ui, "Autres couleurs");
    color_row(ui, "Parallèle / Perpendiculaire Magenta", &mut s.parallel_color);
    color_row(ui, "Tangente Cyan", &mut s.tangent_color);
    ui.add_space(6.0);
    if ui
        .button("Tout réinitialiser")
        .on_hover_text("Réinitialise les préférences d'accessibilité aux paramètres par défaut.")
        .clicked()
    {
        s.axis_colors = AXIS_COLORS;
        s.parallel_color = PARALLEL_COLOR;
        s.tangent_color = TANGENT_COLOR;
    }
    if s != before {
        super::preferences::apply(app, s);
    }
}

/// Preferences › Compatibility: how selected groups and components are highlighted, and
/// which way the mouse wheel zooms.
pub fn compatibility_page(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let before = settings::get();
    let mut s = before.clone();
    group(ui, "Composant | Mise en évidence du groupe");
    ui.checkbox(&mut s.bbox_highlight, "Boîte englobante uniquement");
    group(ui, "Molette de souris");
    ui.checkbox(&mut s.invert_wheel, "Inverser");
    ui.add_space(6.0);
    if ui.button("Tout réinitialiser").clicked() {
        let d = settings::Settings::default();
        (s.bbox_highlight, s.invert_wheel) = (d.bbox_highlight, d.invert_wheel);
    }
    if s != before {
        let redraw = s.bbox_highlight != before.bbox_highlight;
        super::preferences::apply(app, s);
        if redraw {
            app.doc.rev += 1;
        }
    }
}

/// Preferences › Drawing: click style, line chains, lasso direction, crosshairs, pre-pick of
/// Push/Pull and the rotation grips of the Move tool.
pub fn drawing_page(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let before = settings::get();
    let mut s = before.clone();
    group(ui, "Type de clic");
    for c in ClickStyle::ALL {
        ui.radio_value(&mut s.click_style, c, c.label());
    }
    ui.checkbox(&mut s.continue_lines, "Tracé de ligne continu");
    group(ui, "Direction du Lasso");
    ui.radio_value(&mut s.lasso_cw_crossing, true, "Sélection croisée dans le sens des aiguilles d'une montre");
    ui.radio_value(
        &mut s.lasso_cw_crossing,
        false,
        "Sélection croisée dans le sens inverse des aiguilles d'une montre",
    );
    group(ui, "Divers");
    ui.checkbox(&mut s.crosshairs, "Afficher les réticules");
    ui.checkbox(&mut s.no_prepick_pushpull, "Désactiver la présélection dans l’outil Pousser/Tirer");
    ui.checkbox(&mut s.rotation_grips, "Afficher les poignées de rotation des outils");
    ui.add_space(6.0);
    if ui
        .button("Tout réinitialiser")
        .on_hover_text("Réinitialise les préférences de dessin aux paramètres par défaut.")
        .clicked()
    {
        let d = settings::Settings::default();
        s.click_style = d.click_style;
        s.continue_lines = d.continue_lines;
        s.lasso_cw_crossing = d.lasso_cw_crossing;
        s.crosshairs = d.crosshairs;
        s.no_prepick_pushpull = d.no_prepick_pushpull;
        s.rotation_grips = d.rotation_grips;
    }
    if s != before {
        super::preferences::apply(app, s);
    }
}

/// The lines of the crosshair cursor: through the point under the cursor, along the drawing
/// axes, in the axis colours (drawing tools only).
pub fn crosshair_lines(snap: Option<&Snap>, tool: ToolId, axes: &DMat4, half: f64) -> Vec<(DVec3, DVec3, [u8; 3])> {
    let quiet = matches!(tool, ToolId::Select | ToolId::Orbit | ToolId::Pan | ToolId::Zoom | ToolId::Lasso);
    let Some(s) = snap.filter(|_| settings::with(|st| st.crosshairs) && !quiet) else { return Vec::new() };
    (0..3)
        .map(|i| {
            let d = axes.col(i).truncate().normalize_or_zero();
            (s.pos - d * half, s.pos + d * half, settings::axis_color(i))
        })
        .collect()
}

/// Draws them: whole lines across the view, as SketchUp does (`crosshairs.rs`).
pub fn crosshairs(o: &Overlay, snap: Option<&Snap>, tool: ToolId, axes: &DMat4) {
    let Some(s) = snap else { return };
    if !crosshair_lines(Some(s), tool, axes, 1.0).is_empty() {
        crate::crosshairs::paint(&o.painter, o.origin, o.cam, s, axes);
    }
}

/// Preferences › Files: the folder each kind of file is looked for and saved in (file
/// dialogs open there; templates are listed from there).
pub fn files_page(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let before = settings::get();
    let mut s = before.clone();
    group(ui, "Emplacements des fichiers");
    egui::Grid::new("files_grid").num_columns(3).spacing([8.0, 6.0]).show(ui, |ui| {
        for (kind, label) in settings::LOCATIONS {
            ui.label(label);
            let mut text = s.locations.get(kind).cloned().unwrap_or_default();
            let invalid = !text.is_empty() && !std::path::Path::new(&text).is_dir();
            let field = egui::TextEdit::singleline(&mut text)
                .desired_width(300.0)
                .hint_text("(par défaut)")
                .text_color_opt(invalid.then_some(egui::Color32::from_rgb(200, 30, 30)));
            let r = ui.add(field);
            name_widget(ui, &r, &format!("Dossier {label}"), None);
            if r.changed() {
                if text.trim().is_empty() {
                    s.locations.remove(kind);
                } else {
                    s.locations.insert(kind.to_string(), text.trim().to_string());
                }
            }
            let b = ui.button("📁").on_hover_text("Parcourir");
            name_widget(ui, &b, &format!("Parcourir {label}"), None);
            if b.clicked()
                && let Some(d) = settings::dialog(kind).pick_folder()
            {
                s.locations.insert(kind.to_string(), d.display().to_string());
            }
            ui.end_row();
        }
    });
    if s.locations.iter().any(|(_, p)| !std::path::Path::new(p).is_dir()) {
        ui.label(
            egui::RichText::new("Un ou plusieurs chemins d'accès ne sont pas valides (en rouge) : les dossiers par défaut servent à leur place.")
                .color(egui::Color32::from_rgb(200, 30, 30)),
        );
    }
    ui.add_space(6.0);
    if ui
        .button("Réinitialiser")
        .on_hover_text("Réinitialise les préférences d'emplacement des fichiers aux paramètres par défaut.")
        .clicked()
    {
        s.locations.clear();
    }
    if s != before {
        super::preferences::apply(app, s);
    }
}

/// The style the view is drawn with this frame: with Use fast feedback, no ambient occlusion
/// while the camera moves.
pub fn render_style(style: &crate::style::ViewStyle, moving: bool) -> crate::style::ViewStyle {
    let mut s = style.clone();
    if moving && settings::with(|st| st.fast_feedback) {
        s.ambient_occlusion = false;
    }
    s
}

/// Preferences › Graphics: anti-aliasing, fast feedback, texture size, shadows, the
/// automatic enhancement of materials and what the graphics card is.
pub fn graphics_page(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let before = settings::get();
    let mut s = before.clone();
    let gpu = crate::render::gpu_info();
    group(ui, "Paramètres des graphiques");
    ui.horizontal(|ui| {
        ui.label("Anti-crénelage sur échantillons multiples");
        let supported = gpu.as_ref().map(|g| g.samples.clone()).unwrap_or_else(|| vec![1, 4]);
        let choices: Vec<(u32, String)> = supported
            .iter()
            .map(|n| (if *n == 1 { 0 } else { *n }, format!("{}x", if *n == 1 { 0 } else { *n })))
            .collect();
        super::combo(ui, "Anti-crénelage", &mut s.msaa, &choices);
    });
    ui.checkbox(&mut s.fast_feedback, "Utiliser la réponse rapide");
    ui.checkbox(&mut s.max_texture, "Utiliser la taille maximale de texture");
    group(ui, "Moteur graphique");
    ui.radio(true, "Utiliser le nouveau moteur graphique").on_hover_text("Esquisse dessine avec wgpu.");
    ui.add_enabled(false, egui::RadioButton::new(false, "Utiliser le moteur graphique classique"));
    group(ui, "Sélection du GPU");
    ui.label("Sélectionnez le GPU à utiliser par le nouveau moteur graphique.");
    let gpu_name = gpu.as_ref().map(|g| g.name.clone()).unwrap_or_default();
    ui.label(format!(
        "Utiliser les paramètres par défaut du système{}",
        if gpu_name.is_empty() { String::new() } else { format!(" ({gpu_name})") }
    ));
    group(ui, "Ombres rapides");
    ui.checkbox(&mut s.faster_shadows, "Utiliser les ombres plus rapides");
    group(ui, "Améliorer automatiquement les matériaux");
    ui.horizontal(|ui| {
        ui.checkbox(&mut s.enhance_metalness, "Aspect métallique");
        ui.checkbox(&mut s.enhance_roughness, "Rugosité");
    });
    ui.label(
        egui::RichText::new(
            "L'effet de l'attribution automatique des propriétés Rugosité et Métal pour un matériau peut être \
             observé dans le style de face Textures photoréelles.",
        )
        .weak(),
    );
    group(ui, "Capacités");
    let st = &mut app.ui_state.extra.dialogs.prefs;
    if ui.selectable_label(st.gpu_details, "Moteur graphique : détails").clicked() {
        st.gpu_details = !st.gpu_details;
    }
    if st.gpu_details {
        egui::Frame::group(ui.style()).show(ui, |ui| match &gpu {
            Some(g) => {
                ui.label(format!("Carte graphique : {}", g.name));
                ui.label(format!("Interface : {}", g.backend));
                ui.label(format!("Pilote : {}", g.driver));
                ui.label(format!("Taille maximale de texture : {} px", g.max_texture));
                let n: Vec<String> = g.samples.iter().map(|n| format!("{n}x")).collect();
                ui.label(format!("Échantillons : {}", n.join(", ")));
            }
            None => {
                ui.label("Aucune vue 3D n'est encore ouverte.");
            }
        });
    }
    ui.add_space(6.0);
    if ui
        .button("Réinitialiser")
        .on_hover_text("Réinitialise les préférences graphiques aux paramètres par défaut.")
        .clicked()
    {
        let d = settings::Settings::default();
        s.msaa = d.msaa;
        s.fast_feedback = d.fast_feedback;
        s.max_texture = d.max_texture;
        s.faster_shadows = d.faster_shadows;
        s.enhance_metalness = d.enhance_metalness;
        s.enhance_roughness = d.enhance_roughness;
    }
    if s != before {
        let redraw = (s.enhance_metalness, s.enhance_roughness, s.max_texture)
            != (before.enhance_metalness, before.enhance_roughness, before.max_texture);
        super::preferences::apply(app, s);
        if redraw {
            app.doc.rev += 1;
        }
    }
}

/// Preferences › Applications: the image editor texture images open with.
pub fn applications_page(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let before = settings::get();
    let mut s = before.clone();
    super::preferences::image_editor_group(ui, &mut s);
    ui.add_space(6.0);
    if ui
        .button("Tout réinitialiser")
        .on_hover_text("Réinitialise les préférences de l'application aux paramètres d'usine.")
        .clicked()
    {
        s.image_editor.clear();
    }
    if s != before {
        super::preferences::apply(app, s);
    }
}

/// Preferences › Workspace: the size of the tool buttons, and the workspace back as
/// installed (trays, toolbars, preferences). As in SketchUp, the reset asks first, is done
/// when the window closes with OK, and Cancel forgets it.
pub fn workspace_page(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    group(ui, "Palette d'outils");
    crate::toolbars::ensure_loaded(&mut app.ui_state.extra.toolbars);
    let tb = &mut app.ui_state.extra.toolbars;
    if ui.checkbox(&mut tb.large_icons, "Utiliser les grands boutons d'outils").changed() {
        crate::toolbars::save(tb);
    }
    group(ui, "Espace de travail");
    let st = &mut app.ui_state.extra.dialogs.prefs;
    if ui
        .add_enabled(!st.reset_workspace, egui::Button::new("Rétablir les valeurs par défaut"))
        .on_hover_text("Réinitialisez votre espace de travail SketchUp sur la valeur par défaut livrée.")
        .clicked()
    {
        st.confirm_reset = true;
    }
    if st.reset_workspace {
        ui.label(egui::RichText::new("L'espace de travail sera rétabli en cliquant sur OK.").weak());
    }
    if st.confirm_reset {
        crate::ui::centered("Réinitialisation de l'espace de travail").show(ui.ctx(), |ui| {
            ui.set_max_width(420.0);
            ui.label(
                "En rétablissant les paramètres par défaut de l'espace de travail, vous réinitialisez tous les \
                 plateaux, barres d'outils, préférences et extensions aux paramètres et emplacements expédiés \
                 par défaut, en fermant les éventuelles barres d'outils d'extension.",
            );
            ui.add_space(6.0);
            ui.label("Êtes-vous sûr de vouloir continuer ?");
            ui.horizontal(|ui| {
                if ui.button("Oui").clicked() {
                    st.confirm_reset = false;
                    st.reset_workspace = true;
                }
                if ui.button("Non").clicked() {
                    st.confirm_reset = false;
                }
            });
        });
    }
}

/// The workspace as installed: the default tray alone, every toolbar shown with large
/// icons and tooltips, the preferences of a new installation, no extension toolbar.
pub fn reset_workspace(app: &mut EsquisseApp) {
    super::preferences::apply(app, settings::Settings::default());
    super::trays::reset(app);
    let open = app.ui_state.extra.toolbars.open;
    let mut tb = crate::toolbars::ToolbarConfig::from_text("");
    tb.open = open;
    crate::toolbars::save(&tb);
    app.ui_state.extra.toolbars = tb;
    let names: Vec<String> = app.ui_state.extra.ruby.toolbars.iter().map(|b| b.name.clone()).collect();
    for n in names {
        crate::ruby_host::set_plugin_toolbar_shown(&n, false);
    }
}

/// Bounds of a definition's geometry, nested groups and components included.
pub fn def_bounds(m: &esq_core::model::Model, d: esq_core::model::DefId, depth: usize) -> Option<(DVec3, DVec3)> {
    let e = &m.defs.get(d)?.entities;
    let mut lo = DVec3::INFINITY;
    let mut hi = DVec3::NEG_INFINITY;
    for v in e.verts.values() {
        lo = lo.min(v.pos);
        hi = hi.max(v.pos);
    }
    if depth < 32 {
        for i in e.instances.values() {
            if let Some((a, b)) = def_bounds(m, i.def, depth + 1) {
                for c in 0..8 {
                    let p = DVec3::new(
                        if c & 1 == 0 { a.x } else { b.x },
                        if c & 2 == 0 { a.y } else { b.y },
                        if c & 4 == 0 { a.z } else { b.z },
                    );
                    let w = i.xform.transform_point3(p);
                    lo = lo.min(w);
                    hi = hi.max(w);
                }
            }
        }
    }
    lo.is_finite().then_some((lo, hi))
}

/// The twelve edges of an instance's bounding box, in world coordinates.
pub fn box_edges(m: &esq_core::model::Model, d: esq_core::model::DefId, x: &DMat4) -> Vec<(DVec3, DVec3)> {
    let Some((a, b)) = def_bounds(m, d, 0) else { return Vec::new() };
    let corner = |c: usize| {
        x.transform_point3(DVec3::new(
            if c & 1 == 0 { a.x } else { b.x },
            if c & 2 == 0 { a.y } else { b.y },
            if c & 4 == 0 { a.z } else { b.z },
        ))
    };
    let mut out = Vec::new();
    for c in 0..8usize {
        for bit in [1, 2, 4] {
            if c & bit == 0 {
                out.push((corner(c), corner(c | bit)));
            }
        }
    }
    out
}
