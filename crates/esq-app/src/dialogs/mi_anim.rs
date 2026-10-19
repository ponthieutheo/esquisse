//! Model Info › Animation: scene transitions and the delay between scenes, kept in the
//! model's `PageOptions` (`ShowTransition`, `TransitionTime`) and `SlideshowOptions`
//! (`SlideTime`), the options the Ruby API reads.

use eframe::egui;
use esq_core::model::Model;
use serde_json::{Value, json};

use super::{group, name_widget};
use crate::app::EsquisseApp;

fn opt<'a>(m: &'a Model, provider: &str, key: &str) -> Option<&'a Value> {
    m.meta.options.get(provider)?.get(key)
}

/// (transitions on, transition seconds, delay seconds).
pub fn settings(m: &Model) -> (bool, f64, f64) {
    (
        opt(m, "PageOptions", "ShowTransition").and_then(Value::as_bool).unwrap_or(true),
        opt(m, "PageOptions", "TransitionTime").and_then(Value::as_f64).unwrap_or(2.0).max(0.0),
        opt(m, "SlideshowOptions", "SlideTime").and_then(Value::as_f64).unwrap_or(1.0).max(0.0),
    )
}

pub fn set(m: &mut Model, show: bool, transition: f64, delay: f64) {
    let p = m.meta.options.entry("PageOptions".into()).or_default();
    p.insert("ShowTransition".into(), json!(show));
    p.insert("TransitionTime".into(), json!(transition));
    m.meta.options.entry("SlideshowOptions".into()).or_default().insert("SlideTime".into(), json!(delay));
}

/// Every frame: the slideshow uses the model's settings.
pub fn sync(app: &mut EsquisseApp) {
    let (show, t, d) = settings(&app.doc.model);
    app.anim.transition = if show { t } else { 0.0 };
    app.anim.delay = d;
}

fn seconds(ui: &mut egui::Ui, name: &str, v: &mut f64, enabled: bool) -> bool {
    let mut changed = false;
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal(|ui| {
            let r = ui.add(egui::DragValue::new(v).range(0.0..=60.0).speed(0.1).max_decimals(2));
            name_widget(ui, &r, name, Some(&format!("{v}")));
            changed = r.changed();
            ui.label("secondes");
        });
    });
    changed
}

pub fn animation_page(app: &mut EsquisseApp, ui: &mut egui::Ui) {
    let (mut show, mut t, mut d) = settings(&app.doc.model);
    let before = (show, t, d);
    group(ui, "Transitions entre les scènes");
    ui.checkbox(&mut show, "Activer les transitions entre scènes");
    seconds(ui, "Durée de la transition", &mut t, show);
    group(ui, "Délai entre les scènes");
    seconds(ui, "Délai entre les scènes", &mut d, true);
    if (show, t, d) != before {
        app.doc.commit("Animation", |m, _| set(m, show, t, d));
        sync(app);
    }
}
