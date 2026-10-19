//! What Window › Model Info › File sets for the model as a whole: its behaviour once inserted
//! as a component in another model (glue, cut opening, face camera, shadows face sun), kept
//! with the model and given to the component made from it.

use crate::model::{Behavior, Model};

const KEY: &str = "behavior";

impl Model {
    /// The model's own component behaviour (`Model#behavior` of the Ruby API).
    pub fn behavior(&self) -> Behavior {
        self.view_ext.get(KEY).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
    }

    pub fn set_behavior(&mut self, b: Behavior) {
        if b == Behavior::default() {
            self.view_ext.remove(KEY);
        } else {
            self.view_ext.insert(KEY.into(), serde_json::to_value(b).unwrap_or_default());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_model_inserted_as_a_component_keeps_its_behaviour_and_description() {
        let mut src = Model::new();
        src.set_behavior(Behavior { is2d: true, snapto: 2, cuts_opening: true, ..Default::default() });
        src.meta.description = "Fenêtre à deux vantaux".into();
        let mut m = Model::new();
        let d = m.import_as_component(&src, "Fenêtre");
        assert!(m.defs[d].ext.behavior.cuts_opening && m.defs[d].ext.behavior.snapto == 2);
        assert_eq!(m.defs[d].description, "Fenêtre à deux vantaux");
        m.set_behavior(Behavior::default());
        assert!(!m.view_ext.contains_key(KEY));
    }
}
