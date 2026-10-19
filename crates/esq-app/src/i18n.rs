//! Esquisse's own French names for what SketchUp names in English in its files: styles and
//! their collections, environments, material collections, templates
//! (`assets/noms-sketchup.fr.strings`, built into the executable). The English names are
//! SketchUp's, the translations are ours.

use std::collections::HashMap;
use std::sync::OnceLock;

const TEXT: &str = include_str!("../assets/noms-sketchup.fr.strings");

fn table() -> &'static HashMap<String, String> {
    static T: OnceLock<HashMap<String, String>> = OnceLock::new();
    T.get_or_init(|| crate::style_file::parse_strings(TEXT))
}

/// The French name of an English name, if Esquisse has one.
pub fn fr(english: &str) -> Option<&'static str> {
    table().get(english).map(String::as_str)
}

/// The French name of an English name, else the name itself.
pub fn fr_or_same(english: &str) -> String {
    fr(english).map_or_else(|| english.to_string(), str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_translated_with_their_exact_english_keys() {
        assert_eq!(fr("Simple Style"), Some("Style simple"));
        // Keys keep SketchUp's spacing (two spaces, trailing space).
        assert_eq!(fr("00  Default Colors"), Some("00 Couleurs par défaut"));
        assert_eq!(fr("Sketchy blue pen. "), Some("Stylo bleu esquissé."));
        assert_eq!(fr("Sky - Mid-day 2K IBL"), Some("Ciel - Midi 2K IBL"));
        assert_eq!(fr("Asphalt & Concrete"), Some("Asphalte et béton"));
        assert_eq!(fr_or_same("Inconnu"), "Inconnu");
        // Every entry is translated (no empty value).
        assert!(table().len() > 250 && table().values().all(|v| !v.trim().is_empty()));
    }
}
