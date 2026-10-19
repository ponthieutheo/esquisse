//! What Esquisse remembers between sessions, in `%APPDATA%\Esquisse`: recent files,
//! templates (File › Save As Template) and the default template.

use std::path::{Path, PathBuf};

use esq_core::model::{Model, Units};

pub(crate) fn dir() -> Option<PathBuf> {
    crate::settings::user_dir()
}

pub fn load_recent() -> Vec<PathBuf> {
    let Some(d) = dir() else { return Vec::new() };
    std::fs::read_to_string(d.join("recents.txt"))
        .map(|s| s.lines().filter(|l| !l.trim().is_empty()).map(PathBuf::from).collect())
        .unwrap_or_default()
}

pub fn save_recent(files: &[PathBuf]) {
    if let Some(d) = dir() {
        let text: Vec<String> = files.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        let _ = std::fs::write(d.join("recents.txt"), text.join("\n"));
    }
}

/// A model to start from: one of the built-in "Simple" templates, or a file saved with
/// File › Save As Template.
#[derive(Clone, Debug, PartialEq)]
pub struct Template {
    pub name: String,
    pub units: Units,
    pub path: Option<PathBuf>,
}

impl Template {
    pub fn model(&self) -> Model {
        if let Some(p) = &self.path
            && let Ok(m) = load_model_file(p)
        {
            return m;
        }
        // The "Simple" templates: their unit, its usual precision, anti-aliased textures.
        let mut m = Model { units: self.units, ..Model::new() };
        let d = esq_core::units::UnitsDisplay::of(&m);
        d.write(&mut m);
        m.meta.mipmapping = true;
        m
    }

    /// Name shown on the tile: "Simple" for the built-in ones, as in SketchUp; a template
    /// that comes with Esquisse shows its name without its unit ("Architecture").
    pub fn title(&self) -> &str {
        if self.path.is_none() { "Simple" } else { self.name.split(" — ").next().unwrap_or(&self.name) }
    }

    pub fn subtitle(&self) -> &'static str {
        self.units.name()
    }
}

pub fn builtin_templates() -> Vec<Template> {
    [Units::Meters, Units::Centimeters, Units::Millimeters, Units::Inches, Units::Feet]
        .into_iter()
        .map(|units| {
            let t = Template { name: String::new(), units, path: None };
            Template { name: format!("Simple — {}", t.subtitle()), ..t }
        })
        .collect()
}

/// Preferences › Files › Templates, else `%APPDATA%\Esquisse\Modèles types`.
fn templates_dir() -> Option<PathBuf> {
    if let Some(d) = crate::settings::with(|s| s.location("templates")) {
        return Some(d);
    }
    let d = dir()?.join("Modèles types");
    std::fs::create_dir_all(&d).ok()?;
    Some(d)
}

/// Preferences › Template › Browse…: a model file becomes a template (an `.esq` copy in the
/// templates folder).
pub fn add_template_file(src: &Path) -> anyhow::Result<Template> {
    let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let model = if ext == "skp" { esq_skp::import_skp(src)? } else { esq_core::io::load_esq(src)? };
    let name = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Modèle type".into());
    let p = save_template(&model, &name)?;
    Ok(Template {
        name: p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or(name),
        units: model.units,
        path: Some(p),
    })
}

pub fn user_templates() -> Vec<Template> {
    let Some(d) = templates_dir() else { return Vec::new() };
    let Ok(rd) = std::fs::read_dir(d) else { return Vec::new() };
    let mut out: Vec<Template> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "esq"))
        .map(|p| {
            let units = esq_core::io::load_esq(&p).map(|m| m.units).unwrap_or_default();
            let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            Template { name, units, path: Some(p) }
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The templates that come with Esquisse (`Templates` of its content): SketchUp's 21, by name
/// and settings, named "Architecture — Centimètres"… The two "Simple" ones are the built-in
/// templates already listed, so they are left out of the list.
pub fn shipped_templates() -> Vec<Template> {
    static LIST: std::sync::OnceLock<Vec<Template>> = std::sync::OnceLock::new();
    LIST.get_or_init(|| {
        let Some(d) = esq_core::content::folder("Templates") else { return Vec::new() };
        let builtin: Vec<String> = builtin_templates().into_iter().map(|t| t.name).collect();
        let mut files: Vec<PathBuf> = std::fs::read_dir(d)
            .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "esq")).collect())
            .unwrap_or_default();
        files.sort();
        files
            .into_iter()
            .filter_map(|p| {
                let m = esq_core::io::load_esq(&p).ok()?;
                let name = format!("{} — {}", m.meta.name, m.units.name());
                (!builtin.contains(&name)).then_some(Template { name, units: m.units, path: Some(p) })
            })
            .collect()
    })
    .clone()
}

pub fn all_templates() -> Vec<Template> {
    let mut t = builtin_templates();
    t.extend(shipped_templates());
    t.extend(user_templates());
    t
}

/// Tests that change the default template take this (they share the process's folder).
#[cfg(test)]
pub static TEMPLATE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A model file: an .esq, or a .skp read through SketchUp's API.
fn load_model_file(p: &Path) -> anyhow::Result<Model> {
    if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("skp")) {
        esq_skp::import_skp(p)
    } else {
        esq_core::io::load_esq(p)
    }
}

/// Units of a template file outside the templates folder (`Sketchup.template=`), read once per
/// version of the file: the welcome screen asks every frame.
fn file_units(p: &Path) -> Units {
    type Cache = std::collections::HashMap<PathBuf, (Option<std::time::SystemTime>, Units)>;
    static CACHE: std::sync::Mutex<Option<Cache>> = std::sync::Mutex::new(None);
    let stamp = std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let c = c.get_or_insert_with(Default::default);
    if let Some((s, u)) = c.get(p)
        && *s == stamp
    {
        return *u;
    }
    let u = load_model_file(p).map(|m| m.units).unwrap_or_default();
    c.insert(p.to_path_buf(), (stamp, u));
    u
}

/// The template used by File › New (the "favourite" of SketchUp's welcome screen): a template
/// of the list, by name, or any model file chosen by `Sketchup.template=`, by path.
pub fn default_template() -> Template {
    let name = dir().and_then(|d| std::fs::read_to_string(d.join("modele_par_defaut.txt")).ok());
    let all = all_templates();
    let Some(n) = name.map(|n| n.trim().to_string()) else { return all[0].clone() };
    if let Some(t) = all.iter().find(|t| t.name == n) {
        return t.clone();
    }
    let p = PathBuf::from(&n);
    if p.is_absolute() && p.is_file() {
        let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        return Template { name, units: file_units(&p), path: Some(p) };
    }
    all[0].clone()
}

pub fn set_default_template(t: &Template) {
    if let Some(d) = dir() {
        // A template of the list by its name; a file from elsewhere by its path.
        let listed = all_templates().iter().any(|x| x == t);
        let text = match &t.path {
            Some(p) if !listed => p.to_string_lossy().into_owned(),
            _ => t.name.clone(),
        };
        let _ = std::fs::write(d.join("modele_par_defaut.txt"), text);
    }
}

/// `Sketchup.template_dir`: the folder of the templates that come with the application. SketchUp
/// ships them as .skp files; Esquisse writes its "Simple" ones there, as .esq, when asked.
pub fn builtin_dir() -> Option<PathBuf> {
    let d = dir()?.join("Modèles types intégrés");
    std::fs::create_dir_all(&d).ok()?;
    Some(d)
}

/// The file of a template (`Sketchup.template`): its own, or for a built-in one, its copy in
/// [`builtin_dir`] (written if missing).
pub fn template_file(t: &Template) -> Option<PathBuf> {
    if let Some(p) = &t.path {
        return Some(p.clone());
    }
    let p = builtin_dir()?.join(format!("{}.esq", t.name));
    if !p.is_file() {
        esq_core::io::save_esq(&t.model(), &p).ok()?;
    }
    Some(p)
}

/// `Sketchup.template = path`: an empty path brings back the default
/// template, a missing file is refused (false); any other model file becomes the template of
/// File › New (a built-in's or a listed template's file selects that template).
pub fn set_template_path(path: &str) -> bool {
    if path.is_empty() {
        set_default_template(&builtin_templates()[0]);
        return true;
    }
    let p = PathBuf::from(path);
    if !p.exists() {
        return false;
    }
    let same = |q: &Path| std::fs::canonicalize(q).ok().is_some_and(|q| Some(q) == std::fs::canonicalize(&p).ok());
    let builtin = builtin_templates()
        .into_iter()
        .find(|t| builtin_dir().is_some_and(|d| same(&d.join(format!("{}.esq", t.name)))));
    let t = builtin.or_else(|| user_templates().into_iter().find(|t| t.path.as_deref().is_some_and(same)));
    let t = t.unwrap_or_else(|| {
        let p = std::path::absolute(&p).unwrap_or(p);
        let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        Template { name, units: file_units(&p), path: Some(p) }
    });
    set_default_template(&t);
    true
}

/// A setting remembered as `<name>.txt` holding 1 or 0 (Position Texture's Fixed Pins…).
pub fn flag(name: &str, default: bool) -> bool {
    dir().and_then(|d| std::fs::read_to_string(d.join(format!("{name}.txt"))).ok()).map_or(default, |s| s.trim() == "1")
}

pub fn set_flag(name: &str, on: bool) {
    if let Some(d) = dir() {
        let _ = std::fs::write(d.join(format!("{name}.txt")), if on { "1" } else { "0" });
    }
}

/// File › Save As Template.
pub fn save_template(model: &Model, name: &str) -> anyhow::Result<PathBuf> {
    let clean: String = name.chars().map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c }).collect();
    let d = templates_dir().ok_or_else(|| anyhow::anyhow!("dossier des modèles types introuvable"))?;
    let p = d.join(format!("{}.esq", clean.trim()));
    esq_core::io::save_esq(model, Path::new(&p))?;
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_templates_that_come_with_esquisse_are_listed_with_sketchups_names_and_units() {
        let shipped = shipped_templates();
        // SketchUp's 21 templates, less the two "Simple" ones the built-ins already are.
        assert_eq!(shipped.len(), 19);
        let arch = shipped.iter().find(|t| t.name == "Architecture — Centimètres").expect("Architecture en cm");
        assert_eq!((arch.title(), arch.subtitle(), arch.units), ("Architecture", "Centimètres", Units::Centimeters));
        let m = arch.model();
        assert_eq!(m.units, Units::Centimeters);
        assert!(m.view_ext.contains_key("ro"));
        assert!(shipped.iter().any(|t| t.name == "Vue en plan — Mètres"));
        assert!(shipped.iter().any(|t| t.name == "Urbanisme — Pieds"));
        // The whole list: built-ins first, then these.
        let all = all_templates();
        assert_eq!(all[0], builtin_templates()[0]);
        assert!(all.len() >= 5 + 19);
    }
}
