//! Panels of the tray (Window menu), Model Info and Preferences pages, and the other
//! dialogs of SketchUp.exe (`C…Page`, `…Dialog` classes). A page counts as done when it
//! offers the same settings as SketchUp's and they take effect.

use super::Impl::{self, *};
use super::Item;
use crate::commands::{Cmd as C, Panel as P};

const fn it(id: &'static str, label: &'static str, imp: Impl) -> Item {
    Item { id, label, imp }
}

pub const TABLE: &[Item] = &[
    // --- tray panels (Window menu)
    it(
        "panel:entity-info",
        "Infos sur l'entité (toutes les pages : face, arête, groupe, composant, solide, section, cote, texte, image…)",
        Panel(P::EntityInfo),
    ),
    it("panel:materials", "Matières : parcourir", Panel(P::Materials)),
    it(
        "panel:materials-edit",
        "Matières : modifier (couleur, texture, taille, opacité, coloriser, PBR)",
        Feature("mat_panel::show (test: mat_panel::tests::colour_of_a_textured_material_colorizes_it)"),
    ),
    it("panel:components", "Composants : parcourir et options", Panel(P::Components)),
    it(
        "panel:components-edit",
        "Composants : modifier la définition (collage, découpe, face à la caméra, ombres)",
        Feature("dialogs::comp_edit::edit_section (test: components_edit_names_describes_and_aligns_a_definition)"),
    ),
    it("panel:styles", "Styles : parcourir", Panel(P::Styles)),
    it(
        "panel:styles-edit",
        "Styles : modifier (arêtes, faces, arrière-plan, filigrane, modélisation)",
        Feature(
            "styles_panel::edit_tab, watermarks::edit (tests: display::*, watermarks_are_drawn_behind_or_over_the_model)",
        ),
    ),
    it(
        "panel:styles-mix",
        "Styles : mélanger",
        Feature(
            "styles_panel::mix_tab, EsquisseApp::mix_style (test: styles_are_used_created_updated_mixed_and_purged)",
        ),
    ),
    it("panel:tags", "Étiquettes (dossiers, couleurs, tirets, visibilité)", Panel(P::Tags)),
    it("panel:scenes", "Scènes", Panel(P::Scenes)),
    it("panel:shadows", "Ombres", Panel(P::Shadows)),
    it("panel:fog", "Brouillard", Panel(P::Fog)),
    it("panel:match-photo", "Adapter une photo", Panel(P::MatchPhoto)),
    it("panel:soften", "Adoucir les arêtes", Panel(P::Soften)),
    it("panel:outliner", "Structure", Panel(P::Outliner)),
    it("panel:instructor", "Instructeur", Panel(P::Instructor)),
    it("panel:environments", "Environnements (parcourir, modifier)", Panel(P::Environments)),
    it("panel:overlays", "Incrustations", Panel(P::Overlays)),
    it("panel:hatch-patterns", "Motifs de hachures (parcourir, modifier)", Panel(P::HatchPatterns)),
    it("panel:model-grid", "Grille du modèle", Panel(P::ModelGrid)),
    it(
        "panel:tray-manage",
        "Plateaux : nouveau, renommer, ancrer",
        Feature("dialogs::trays::windows (test: trays_are_made_renamed_docked_and_deleted)"),
    ),
    it("panel:ruby-console", "Console Ruby", Cmd(C::RubyConsole)),
    it("panel:extension-manager", "Gestionnaire d'extensions", Cmd(C::ExtensionManager)),
    // --- Model Info pages
    it("modelinfo:animation", "Infos sur le modèle › Animation", Cmd(C::AnimSettings)),
    it(
        "modelinfo:classifications",
        "Infos sur le modèle › Classifications",
        Feature(
            "dialogs::mi_classif::classifications_page (test: classifications_are_read_from_skc_files_and_shared_with_ruby)",
        ),
    ),
    it(
        "modelinfo:components",
        "Infos sur le modèle › Composants (estomper le reste)",
        Feature("dialogs::mi_misc::components_page (test: components_page_fades_hides_and_shows_axes)"),
    ),
    it(
        "modelinfo:credits",
        "Infos sur le modèle › Crédits",
        Feature("dialogs::mi_misc::credits_page (test: credits_page_claims_the_model_and_its_components)"),
    ),
    it(
        "modelinfo:dimensions",
        "Infos sur le modèle › Cotations (police, flèches, alignement)",
        Feature("dialogs::mi_annot::dims_page (test: dimensions_page_aligns_text_and_hides_what_is_unreadable)"),
    ),
    it(
        "modelinfo:file",
        "Infos sur le modèle › Fichier",
        Feature("dialogs::mi_file::file_page (test: file_page_names_describes_and_aligns_the_model)"),
    ),
    it(
        "modelinfo:geolocation",
        "Infos sur le modèle › Géolocalisation",
        Feature("dialogs::mi_file::geolocation_page (test: geolocation_page_shows_sets_and_clears_the_location)"),
    ),
    it(
        "modelinfo:rendering",
        "Infos sur le modèle › Rendu (anticrénelage des textures)",
        Feature("dialogs::mi_misc::rendering_page (test: rendering_page_turns_anti_aliased_textures_on_and_off)"),
    ),
    it(
        "modelinfo:statistics",
        "Infos sur le modèle › Statistiques (purger, corriger)",
        Feature("dialogs::mi_stats::statistics_page (test: statistics_page_counts_purges_and_fixes_problems)"),
    ),
    it(
        "modelinfo:text",
        "Infos sur le modèle › Texte",
        Feature("dialogs::mi_annot::text_page (test: text_page_sets_new_texts_and_updates_the_selected_ones)"),
    ),
    it(
        "modelinfo:units",
        "Infos sur le modèle › Unités (format, précision, accrochage, angles)",
        Feature("dialogs::model_info::units_page (test: model_info_units_page_sets_how_lengths_show_and_snap)"),
    ),
    // --- Preferences pages
    it(
        "pref:accessibility",
        "Préférences › Accessibilité (couleurs des axes et inférences)",
        Feature("dialogs::pref_pages::accessibility_page (test: accessibility_page_colours_axes_and_inferences)"),
    ),
    it(
        "pref:applications",
        "Préférences › Applications (éditeur d'images)",
        Feature(
            "dialogs::pref_pages::applications_page (test: applications_page_picks_the_image_editor_textures_open_with)",
        ),
    ),
    it(
        "pref:compatibility",
        "Préférences › Compatibilité (sélection, molette)",
        Feature("dialogs::pref_pages::compatibility_page (test: compatibility_page_box_highlight_and_wheel)"),
    ),
    it(
        "pref:drawing",
        "Préférences › Dessin (clic-glisser, réticules, prépositionnement)",
        Feature(
            "dialogs::pref_pages::drawing_page (test: drawing_page_click_style_lines_lasso_crosshairs_and_prepick)",
        ),
    ),
    it(
        "pref:files",
        "Préférences › Fichiers (emplacements)",
        Feature("dialogs::pref_pages::files_page (test: files_page_sets_where_models_and_templates_go)"),
    ),
    it(
        "pref:general",
        "Préférences › Général (sauvegarde auto, copie de sauvegarde, vérifications)",
        Feature("dialogs::pref_general::general_page (test: general_page_backs_up_auto_saves_purges_and_fixes)"),
    ),
    it(
        "pref:graphics",
        "Préférences › Graphiques (anticrénelage, OpenGL)",
        Feature(
            "dialogs::pref_pages::graphics_page (test: graphics_page_sets_antialiasing_shadows_textures_and_materials)",
        ),
    ),
    it(
        "pref:shortcuts",
        "Préférences › Raccourcis (affecter, importer, exporter, réinitialiser)",
        Feature("dialogs::pref_shortcuts::shortcuts_page (test: shortcuts_page_assigns_moves_removes_and_resets)"),
    ),
    it(
        "pref:template",
        "Préférences › Modèle type par défaut",
        Feature("dialogs::preferences::template_page (test: template_page_chooses_what_file_new_starts_from)"),
    ),
    it(
        "pref:workspace",
        "Préférences › Espace de travail (taille des icônes, réinitialiser)",
        Feature(
            "dialogs::pref_pages::workspace_page (test: workspace_page_sizes_tool_buttons_and_resets_trays_and_toolbars_on_ok)",
        ),
    ),
    // --- other dialogs
    it("dialog:search", "Recherche de commandes (Omnibar)", Cmd(C::Search)),
    it("dialog:welcome", "Bienvenue (modèles types, fichiers récents)", Cmd(C::Welcome)),
    it("dialog:save-as-template", "Enregistrer comme modèle type", Cmd(C::SaveAsTemplate)),
    it("dialog:toolbars", "Barres d'outils (afficher, personnaliser)", Cmd(C::Toolbars)),
    it(
        "dialog:workspace-config",
        "Configuration de l'espace de travail",
        Feature("dialogs::workspaces::windows (test: workspaces_keep_a_layout_and_bring_it_back)"),
    ),
    it(
        "dialog:export-options",
        "Options d'exportation (par format)",
        Feature("formats3d::show_options (test: tests::formats3d::every_format_with_options_has_a_dialog)"),
    ),
    it(
        "dialog:import-options",
        "Options d'importation (par format)",
        Feature("formats3d::show_options (test: tests::formats3d::every_format_with_options_has_a_dialog)"),
    ),
    it(
        "dialog:page-options",
        "Propriétés de scène (éléments à enregistrer)",
        Feature("dialogs::scene_props::properties_ui (test: scenes_save_and_restore_the_properties_ticked)"),
    ),
    it(
        "dialog:tag-dialog",
        "Étiquette d'un nouvel objet / réaffectation",
        Feature("dialogs::tag_dialog::windows (test: tags_have_an_active_one_for_new_objects_and_ask_before_deleting)"),
    ),
];
