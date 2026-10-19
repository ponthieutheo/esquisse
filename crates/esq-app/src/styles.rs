//! The styles of the model (Window › Styles › In Model) and the rendering options kept with
//! the document. The list is the one the Ruby API keeps (`Model::view_ext["styles"]`:
//! `{list: [{id, name, description, path, ro}], selected}`), so that `Sketchup::Styles` and
//! the panel show the same styles; each style is a snapshot of the rendering options (`ro`)
//! plus its watermarks (`wm`).

use esq_core::model::Model;
use serde_json::{Value as J, json};

use crate::app::EsquisseApp;
use crate::style::{ViewStyle, Watermark};

/// Name of the style a new document has (as the Ruby API names it).
pub const DEFAULT_STYLE_NAME: &str = "Style par défaut";

/// One style of the model.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelStyle {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub ro: serde_json::Map<String, J>,
    pub watermarks: Vec<Watermark>,
}

impl ModelStyle {
    fn to_json(&self) -> J {
        json!({
            "id": self.id, "name": self.name, "description": self.description, "path": "",
            "ro": self.ro, "wm": serde_json::to_value(&self.watermarks).unwrap_or(J::Null),
        })
    }

    fn from_json(j: &J) -> Option<ModelStyle> {
        Some(ModelStyle {
            id: j["id"].as_i64()?,
            name: j["name"].as_str().unwrap_or("").to_string(),
            description: j["description"].as_str().unwrap_or("").to_string(),
            ro: j["ro"].as_object().cloned().unwrap_or_default(),
            watermarks: serde_json::from_value(j["wm"].clone()).unwrap_or_default(),
        })
    }

    /// The style these settings make (the model's own settings, such as fog, are not part of
    /// a style).
    pub fn of(id: i64, name: &str, style: &ViewStyle) -> ModelStyle {
        ModelStyle {
            id,
            name: name.to_string(),
            description: String::new(),
            ro: style.style_snapshot(),
            watermarks: style.watermark_list.clone(),
        }
    }

    /// Applies the style to the view settings.
    pub fn apply(&self, style: &mut ViewStyle) {
        style.restore(&self.ro);
        style.watermark_list = self.watermarks.clone();
    }

    /// Whether the view settings differ from this style (SketchUp then offers to update it).
    pub fn differs(&self, style: &ViewStyle) -> bool {
        let mut probe = style.clone();
        self.apply(&mut probe);
        probe != *style
    }
}

/// The styles of the model and the selected one's id (created when missing).
pub fn model_styles(model: &mut Model, style: &ViewStyle) -> (Vec<ModelStyle>, i64) {
    let st = model.view_ext.get("styles").cloned().unwrap_or(J::Null);
    let mut list: Vec<ModelStyle> =
        st["list"].as_array().map(|a| a.iter().filter_map(ModelStyle::from_json).collect()).unwrap_or_default();
    if list.is_empty() {
        list.push(ModelStyle::of(1, DEFAULT_STYLE_NAME, style));
        save_styles(model, &list, 1);
    }
    let selected = st["selected"].as_i64().filter(|s| list.iter().any(|x| x.id == *s)).unwrap_or(list[0].id);
    (list, selected)
}

pub fn save_styles(model: &mut Model, list: &[ModelStyle], selected: i64) {
    model.view_ext.insert(
        "styles".into(),
        json!({"list": list.iter().map(ModelStyle::to_json).collect::<Vec<_>>(), "selected": selected}),
    );
}

/// A style name not used yet in the model: "Name", "Name 1"…
pub fn unique_name(base: &str, list: &[ModelStyle]) -> String {
    if !list.iter().any(|s| s.name == base) {
        return base.to_string();
    }
    (1..).map(|i| format!("{base} {i}")).find(|n| !list.iter().any(|s| &s.name == n)).unwrap()
}

impl EsquisseApp {
    /// The document's rendering options become the view's (a document is opened or made).
    pub fn style_from_model(&mut self) {
        let keep = (self.style.scene_tabs, self.style.hide_rest, self.style.hide_similar);
        let mut s = ViewStyle::default();
        if let Some(ro) = self.doc.model.view_ext.get("ro").and_then(J::as_object) {
            s.restore(ro);
        }
        if let Some(wm) = self.doc.model.view_ext.get("watermarks") {
            s.watermark_list = serde_json::from_value(wm.clone()).unwrap_or_default();
        }
        (s.scene_tabs, s.hide_rest, s.hide_similar) = keep;
        self.style = s;
        self.doc.rev += 1;
    }

    /// The view's rendering options are written into the document (it is about to be saved).
    pub fn style_to_model(&mut self) {
        write_style(&mut self.doc.model, &self.style);
    }
}

/// Writes the rendering options and watermarks into the model, keeping the options the
/// style does not hold.
pub fn write_style(model: &mut Model, style: &ViewStyle) {
    let ro = model.view_ext.entry("ro".to_string()).or_insert_with(|| json!({}));
    if !ro.is_object() {
        *ro = json!({});
    }
    let map = ro.as_object_mut().unwrap();
    for (k, v) in style.snapshot() {
        map.insert(k, v);
    }
    if style.watermark_list.is_empty() {
        model.view_ext.remove("watermarks");
    } else {
        model.view_ext.insert("watermarks".into(), serde_json::to_value(&style.watermark_list).unwrap_or(J::Null));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_style_travels_with_the_document() {
        let mut m = Model::new();
        let s = ViewStyle { bg_color: [10, 20, 30], ground: true, fog: true, ..Default::default() };
        write_style(&mut m, &s);
        let json = serde_json::to_string(&m).unwrap();
        let back: Model = serde_json::from_str(&json).unwrap();
        let mut t = ViewStyle::default();
        t.restore(back.view_ext["ro"].as_object().unwrap());
        assert_eq!(t, s);
    }

    #[test]
    fn styles_are_listed_selected_and_compared() {
        let mut m = Model::new();
        let mut view = ViewStyle::default();
        let (list, sel) = model_styles(&mut m, &view);
        assert_eq!((list.len(), sel, list[0].name.as_str()), (1, 1, DEFAULT_STYLE_NAME));
        assert!(!list[0].differs(&view));
        view.edges = false;
        assert!(list[0].differs(&view));
        // Fog is the model's, not the style's.
        view.edges = true;
        view.fog = true;
        assert!(!list[0].differs(&view));
        let mut l2 = list.clone();
        l2.push(ModelStyle::of(2, &unique_name(DEFAULT_STYLE_NAME, &list), &view));
        assert_eq!(l2[1].name, "Style par défaut 1");
        save_styles(&mut m, &l2, 2);
        let (back, sel) = model_styles(&mut m, &view);
        assert_eq!((back.len(), sel), (2, 2));
    }
}
