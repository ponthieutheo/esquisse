//! Length units offered by the importers and exporters ("Unités du modèle;Pouces;Pieds;…" of
//! SketchUp's `*_exporter.strings`). Esquisse stores millimetres.

use esq_core::model::Units;
use serde::{Deserialize, Serialize};

pub const INCH: f64 = 25.4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LenUnit {
    /// The unit of the model (Model Info › Units).
    #[default]
    Model,
    Inches,
    Feet,
    Yards,
    Miles,
    Millimeters,
    Centimeters,
    Meters,
    Kilometers,
}

impl LenUnit {
    /// The list of the 3DS, OBJ, FBX and XSI dialogs, in their order.
    pub const ALL: [LenUnit; 9] = [
        LenUnit::Model,
        LenUnit::Inches,
        LenUnit::Feet,
        LenUnit::Yards,
        LenUnit::Miles,
        LenUnit::Millimeters,
        LenUnit::Centimeters,
        LenUnit::Meters,
        LenUnit::Kilometers,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LenUnit::Model => "Unités du modèle",
            LenUnit::Inches => "Pouces",
            LenUnit::Feet => "Pieds",
            LenUnit::Yards => "Yards",
            LenUnit::Miles => "Miles",
            LenUnit::Millimeters => "Millimètres",
            LenUnit::Centimeters => "Centimètres",
            LenUnit::Meters => "Mètres",
            LenUnit::Kilometers => "Kilomètres",
        }
    }

    /// Length of one unit in millimetres (`model` resolves "model units").
    pub fn mm(self, model: Units) -> f64 {
        match self {
            LenUnit::Model => model.factor(),
            LenUnit::Inches => INCH,
            LenUnit::Feet => INCH * 12.0,
            LenUnit::Yards => INCH * 36.0,
            LenUnit::Miles => INCH * 63_360.0,
            LenUnit::Millimeters => 1.0,
            LenUnit::Centimeters => 10.0,
            LenUnit::Meters => 1000.0,
            LenUnit::Kilometers => 1_000_000.0,
        }
    }

    /// The unit written in a file whose own unit is `u` (`Model` stays `Model`).
    pub fn from_units(u: Units) -> LenUnit {
        match u {
            Units::Millimeters => LenUnit::Millimeters,
            Units::Centimeters => LenUnit::Centimeters,
            Units::Meters => LenUnit::Meters,
            Units::Inches => LenUnit::Inches,
            Units::Feet => LenUnit::Feet,
            Units::Yards => LenUnit::Yards,
        }
    }

    /// The concrete unit (never `Model`).
    pub fn resolve(self, model: Units) -> LenUnit {
        if self == LenUnit::Model { LenUnit::from_units(model) } else { self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_units_follow_the_model() {
        assert_eq!(LenUnit::Model.mm(Units::Meters), 1000.0);
        assert_eq!(LenUnit::Model.resolve(Units::Inches), LenUnit::Inches);
        assert_eq!(LenUnit::Yards.mm(Units::Millimeters), 914.4);
        assert_eq!(LenUnit::ALL.len(), 9);
    }
}
