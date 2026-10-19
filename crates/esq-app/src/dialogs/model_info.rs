//! Window › Model Info: the settings that belong to the model (saved with it, read by the
//! Ruby API through `model.options`, `model.name`…).

use eframe::egui::{self, RichText};
use esq_core::model::Units;
use esq_core::units::{AREA_UNITS, FORMATS, SNAP_ANGLES, UnitsDisplay, VOLUME_UNITS};

use super::{InfoPage, combo, group, name_widget, page_list, text_field};
use crate::app::EsquisseApp;

/// What the pages keep between frames (texts being typed).
#[derive(Default)]
pub struct InfoState {
    /// Units › length snapping: the text being typed.
    pub snap_text: Option<String>,
    /// Text, Dimensions: the font being chosen ("Polices...").
    pub font_editor: Option<super::mi_annot::FontTarget>,
    /// Dimensions › Expert dimension settings shown.
    pub expert_dims: bool,
    pub stats: super::mi_stats::StatsState,
    /// File: the name and description being typed.
    pub name_text: Option<String>,
    pub desc_text: Option<String>,
    /// Geo-location › Set Manual Location shown.
    pub manual_location: bool,
    /// Classifications: the schema chosen in the list.
    pub schema: Option<String>,
}

pub fn window(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let pages: Vec<(InfoPage, &str)> = InfoPage::ALL.iter().map(|p| (*p, p.label())).collect();
    let mut page = app.ui_state.extra.dialogs.info_page;
    ui.horizontal_top(|ui| {
        page_list(ui, &pages, &mut page);
        ui.separator();
        egui::ScrollArea::vertical().id_salt("model_info_page").show(ui, |ui| {
            ui.set_min_width(420.0);
            ui.vertical(|ui| match page {
                InfoPage::Units => units_page(app, ui),
                InfoPage::Text => super::mi_annot::text_page(app, ui),
                InfoPage::Dimensions => super::mi_annot::dims_page(app, ui),
                InfoPage::Components => super::mi_misc::components_page(app, ui),
                InfoPage::Rendering => super::mi_misc::rendering_page(app, ui),
                InfoPage::Credits => super::mi_misc::credits_page(app, ui),
                InfoPage::Animation => super::mi_anim::animation_page(app, ui),
                InfoPage::Geolocation => super::mi_file::geolocation_page(app, ui),
                InfoPage::File => super::mi_file::file_page(app, ui),
                InfoPage::Classifications => super::mi_classif::classifications_page(app, ui),
                InfoPage::Statistics => super::mi_stats::statistics_page(app, ui),
            });
        });
    });
    app.ui_state.extra.dialogs.info_page = page;
}

// ------------------------------------------------------------------------------------------
// Units

/// Commits new unit settings as one undoable step.
pub fn set_units(app: &mut EsquisseApp, d: &UnitsDisplay) {
    let d = d.clone();
    app.doc.commit("Unités", |m, _| d.write(m));
    esq_core::units::set_display(Some(UnitsDisplay::of(&app.doc.model)));
}

/// What changing one setting implies, as in SketchUp: architectural and fractional lengths
/// are in inches, engineering ones in feet; a precision the format does not have is brought
/// back in range.
pub fn adjust(mut d: UnitsDisplay, before: &UnitsDisplay) -> UnitsDisplay {
    if d.format != before.format
        && let Some(u) = UnitsDisplay::format_unit(d.format)
    {
        d.unit = u;
        // Fractions start at 1/16", engineering feet at two decimals.
        if d.precision == before.precision {
            d.precision = if d.format == 2 { 2 } else { 4 };
        }
    }
    d.precision = d.precision.min(6);
    if d.unit != before.unit && d.format == 0 {
        // A new unit starts with the precision of its template.
        d.precision = esq_core::units::default_precision(d.unit);
    }
    d
}

fn area_name(i: usize) -> String {
    format!("{} ({})", Units::SKETCHUP_ORDER[i].name(), AREA_UNITS[code_of(i)].0)
}

fn units_page(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let before = UnitsDisplay::of(&app.doc.model);
    let mut d = before.clone();
    group(ui, "Unités de mesure");
    egui::Grid::new("units_grid").num_columns(3).spacing([12.0, 6.0]).show(ui, |ui| {
        ui.label("");
        ui.label("");
        ui.label("Afficher la précision");
        ui.end_row();

        ui.label("Format");
        let formats: Vec<(u8, String)> = FORMATS.iter().enumerate().map(|(i, f)| (i as u8, f.to_string())).collect();
        combo(ui, "Format", &mut d.format, &formats);
        ui.end_row();

        ui.label("Longueur");
        let units: Vec<(Units, String)> = Units::SKETCHUP_ORDER.iter().map(|u| (*u, u.name().to_string())).collect();
        ui.add_enabled_ui(d.format == 0, |ui| combo(ui, "Unité de longueur", &mut d.unit, &units));
        let precisions: Vec<(u32, String)> =
            UnitsDisplay::precision_choices(d.format).into_iter().enumerate().map(|(i, s)| (i as u32, s)).collect();
        combo(ui, "Précision de longueur", &mut d.precision, &precisions);
        ui.end_row();

        let decimals: Vec<(u32, String)> =
            UnitsDisplay::precision_choices(0).into_iter().enumerate().map(|(i, s)| (i as u32, s)).collect();
        ui.label("Surface");
        let areas: Vec<(usize, String)> = (0..AREA_UNITS.len()).map(|i| (code_of(i), area_name(i))).collect();
        combo(ui, "Unité de surface", &mut d.area_unit, &areas);
        combo(ui, "Précision de surface", &mut d.area_precision, &decimals);
        ui.end_row();

        ui.label("Volume");
        let mut volumes: Vec<(usize, String)> = (0..6)
            .map(|i| (code_of(i), format!("{} ({})", Units::SKETCHUP_ORDER[i].name(), VOLUME_UNITS[code_of(i)].0)))
            .collect();
        volumes.push((7, "Gallons".into()));
        volumes.push((6, "Litres".into()));
        combo(ui, "Unité de volume", &mut d.volume_unit, &volumes);
        combo(ui, "Précision de volume", &mut d.volume_precision, &decimals);
        ui.end_row();
    });
    let mut show_units = !d.suppress;
    ui.checkbox(&mut show_units, "Afficher le format des unités");
    d.suppress = !show_units;
    ui.add_enabled_ui(d.format == 1, |ui| ui.checkbox(&mut d.force_inch, "Forcer l'affichage de 0\""));
    ui.horizontal(|ui| {
        ui.checkbox(&mut d.length_snap, "Activer l'ancrage de longueur");
        let st = &mut app.ui_state.extra.dialogs.info;
        let mut text = st.snap_text.clone().unwrap_or_else(|| d.length(d.snap_length));
        ui.add_enabled_ui(d.length_snap, |ui| {
            let (done, focused) = text_field(ui, "Longueur d'ancrage", &mut text, 110.0);
            if focused {
                st.snap_text = Some(text.clone());
            }
            if done {
                if let Some(v) = esq_core::units::parse_length(&text, d.unit).filter(|v| *v > 0.0) {
                    d.snap_length = v;
                }
                st.snap_text = None;
            }
        });
    });

    group(ui, "Unités angulaires");
    egui::Grid::new("angle_grid").num_columns(3).spacing([12.0, 6.0]).show(ui, |ui| {
        ui.label("Angle");
        ui.label("Degrés");
        let angles: Vec<(u32, String)> =
            UnitsDisplay::precision_choices(0).into_iter().take(4).enumerate().map(|(i, s)| (i as u32, s)).collect();
        combo(ui, "Précision des angles", &mut d.angle_precision, &angles);
        ui.end_row();
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut d.angle_snap, "Activer l'ancrage angulaire");
        let snaps: Vec<(u64, String)> =
            SNAP_ANGLES.iter().map(|a| (a.to_bits(), format!("{a:.1}").replace('.', ","))).collect();
        let mut bits = d.snap_angle.to_bits();
        ui.add_enabled_ui(d.angle_snap, |ui| combo(ui, "Angle d'ancrage", &mut bits, &snaps));
        d.snap_angle = f64::from_bits(bits);
    });

    if d != before {
        d = adjust(d, &before);
        set_units(app, &d);
    }
    ui.add_space(6.0);
    let example = RichText::new(format!("Exemple : {}", d.length(1234.5678))).weak();
    let r = ui.label(example);
    name_widget(ui, &r, "Exemple", Some(&d.length(1234.5678)));
}

/// SketchUp's codes follow its list order (inches, feet, yards, mm, cm, m) only for lengths;
/// area and volume codes are 0 in, 1 ft, 2 mm, 3 cm, 4 m, 5 yd.
fn code_of(list_index: usize) -> usize {
    Units::SKETCHUP_ORDER[list_index].code() as usize
}
