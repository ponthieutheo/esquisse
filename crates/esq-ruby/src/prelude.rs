//! The Ruby side of the SketchUp API, in domain files loaded in this order.

const FILES: &[(&str, &str)] = &[
    ("00_core.rb", include_str!("rb/00_core.rb")),
    ("10_geom.rb", include_str!("rb/10_geom.rb")),
    ("20_sketchup_base.rb", include_str!("rb/20_sketchup_base.rb")),
    ("21_entities.rb", include_str!("rb/21_entities.rb")),
    ("22_model.rb", include_str!("rb/22_model.rb")),
    ("30_ui.rb", include_str!("rb/30_ui.rb")),
    ("40_geom_extra.rb", include_str!("rb/40_geom_extra.rb")),
    ("41_entities_extra.rb", include_str!("rb/41_entities_extra.rb")),
    ("42_instances.rb", include_str!("rb/42_instances.rb")),
    ("43_model_extra.rb", include_str!("rb/43_model_extra.rb")),
    ("44_view_camera.rb", include_str!("rb/44_view_camera.rb")),
    ("45_ui_dialogs.rb", include_str!("rb/45_ui_dialogs.rb")),
    ("46_observers_tools.rb", include_str!("rb/46_observers_tools.rb")),
    ("47_misc.rb", include_str!("rb/47_misc.rb")),
];

/// Evaluates each file separately so a Ruby error names the file and line.
pub fn load(eval: &mut dyn FnMut(&str, &str) -> Result<(), String>) -> Result<(), String> {
    for (name, src) in FILES {
        eval(name, src).map_err(|e| format!("{name}: {e}"))?;
    }
    Ok(())
}
