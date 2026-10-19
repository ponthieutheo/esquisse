//! Edit › Make Component: SketchUp's « Créer un composant » dialog before the component is
//! made — General (Definition, Description), Alignment (Glue to, Cut opening, Always face
//! camera, Shadows face sun), Advanced Attributes (Price, Size, URL) and « Remplacer la
//! sélection par le composant » — then Create: one operation « Créer un composant ». « Définir
//! les axes du composant » (`IDC_PICK_ORIGIN`) hides the dialog while the Axes tool picks the
//! new definition's origin and axes, then shows it again. Unticked,
//! the selection stays as it was and only the definition is added to the model.

use eframe::egui;
use esq_core::glam::DMat4;
use esq_core::model::{AttrValue, Behavior, EntityRef};

use crate::app::EsquisseApp;
use crate::dialogs::name_widget;

#[derive(Clone)]
pub struct MakeComponent {
    pub name: String,
    pub description: String,
    pub behavior: Behavior,
    pub price: String,
    pub size: String,
    pub url: String,
    pub replace: bool,
    /// Set Component Axes: the axes picked (world), the origin and directions the new
    /// definition gets.
    pub axes: Option<DMat4>,
    /// The Axes tool is picking them: the dialog is hidden meanwhile.
    pub picking: bool,
    /// What was selected when the dialog opened.
    items: Vec<EntityRef>,
}

thread_local! {
    static OPEN: std::cell::RefCell<Option<MakeComponent>> = const { std::cell::RefCell::new(None) };
}

/// The Axes tool picked the component's axes (Set Component Axes): the dialog shows again.
pub fn axes_picked(axes: DMat4) {
    OPEN.with(|o| {
        if let Some(d) = o.borrow_mut().as_mut() {
            d.axes = Some(axes);
            d.picking = false;
        }
    });
}

/// Opens the dialog for the selection (nothing without one).
pub fn open(app: &mut EsquisseApp) {
    let items = app.doc.sel.clone();
    if items.is_empty() {
        return;
    }
    let d = MakeComponent {
        name: app.doc.model.unique_def_name("Composant"),
        description: String::new(),
        behavior: Behavior::default(),
        price: String::new(),
        size: String::new(),
        url: String::new(),
        replace: true,
        axes: None,
        picking: false,
        items,
    };
    OPEN.with(|o| *o.borrow_mut() = Some(d));
}

/// Tests: the dialog as it stands.
#[cfg(test)]
pub fn state() -> Option<MakeComponent> {
    OPEN.with(|o| o.borrow().clone())
}

/// Create: the component made from the selection with the dialog's settings.
pub fn create(app: &mut EsquisseApp, d: &MakeComponent) {
    let items = d.items.clone();
    let mut made = None;
    let s = d.clone();
    let ctx_x = app.doc.ctx_xform();
    app.doc.commit("Créer un composant", |m, ctx| {
        let before = m.entities(ctx).clone();
        let taken = m.defs.values().any(|x| x.name == s.name.trim());
        let name = if s.name.trim().is_empty() || taken {
            m.unique_def_name(if s.name.trim().is_empty() { "Composant" } else { s.name.trim() })
        } else {
            s.name.trim().to_string()
        };
        let Some(i) = m.make_group(ctx, &items, false, Some(name)) else { return };
        let def = m.entities(ctx).instances[i].def;
        let dd = &mut m.defs[def];
        dd.description = s.description.clone();
        dd.ext.behavior = s.behavior;
        let set = dd.attributes.entry("SU_DefinitionSet".to_string()).or_default();
        let price = s.price.trim().replace(',', ".");
        set.insert(
            "Price".into(),
            price.parse::<f64>().map_or(AttrValue::Text(s.price.trim().into()), AttrValue::Number),
        );
        set.insert("Size".into(), AttrValue::Text(s.size.trim().into()));
        set.insert("Url".into(), AttrValue::Text(s.url.trim().into()));
        // Set Component Axes: the definition's origin and axes are the ones picked, the
        // geometry staying where it is.
        if let Some(axes) = s.axes {
            let world = ctx_x * m.entities(ctx).instances[i].xform;
            m.change_definition_axes(def, world.inverse() * axes);
        }
        if s.replace {
            made = Some(i);
        } else {
            // Only the definition: the geometry stays where it was.
            *m.entities_mut(ctx) = before;
        }
    });
    match made {
        Some(i) => app.doc.select(vec![EntityRef::Instance(i)]),
        None => app.doc.select(items),
    }
}

/// Every frame.
pub fn window(app: &mut EsquisseApp, ctx: &egui::Context) {
    let Some(mut d) = OPEN.with(|o| o.borrow().clone()) else { return };
    if d.picking {
        // Hidden while the Axes tool picks; back when it is put down without axes.
        if app.tool.id() != crate::tools::ToolId::AxesTool {
            OPEN.with(|o| {
                if let Some(x) = o.borrow_mut().as_mut() {
                    x.picking = false;
                }
            });
        }
        return;
    }
    let (mut create_it, mut cancel, mut pick) = (false, false, false);
    crate::ui::centered("Créer un composant").show(ctx, |ui| {
        ui.set_max_width(380.0);
        crate::dialogs::group(ui, "Général");
        egui::Grid::new("make_component").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            for (label, value) in [("Définition:", &mut d.name), ("Description:", &mut d.description)] {
                ui.label(label);
                let r = ui.add(egui::TextEdit::singleline(value).desired_width(230.0));
                name_widget(ui, &r, label, Some(value.as_str()));
                ui.end_row();
            }
        });
        crate::dialogs::group(ui, "Alignement");
        crate::dialogs::mi_file::alignment_ui(ui, &mut d.behavior);
        pick = ui.button("Définir les axes du composant").clicked();
        crate::dialogs::group(ui, "Attributs avancés");
        egui::Grid::new("make_component_adv").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            for (label, value, hint) in [
                ("Prix :", &mut d.price, "Saisir le prix de la définition"),
                ("Taille:", &mut d.size, "Saisir la taille de la définition"),
                ("URL:", &mut d.url, "Saisir l'URL de la définition"),
            ] {
                ui.label(label);
                let r = ui.add(egui::TextEdit::singleline(value).hint_text(hint).desired_width(230.0));
                name_widget(ui, &r, label, Some(value.as_str()));
                ui.end_row();
            }
        });
        ui.add_space(4.0);
        ui.checkbox(&mut d.replace, "Remplacer la sélection par le composant");
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            create_it = ui.button("Créer").clicked();
            cancel = ui.button("Annuler").clicked();
        });
    });
    if create_it {
        create(app, &d);
    }
    if pick {
        // SketchUp hides the dialog and the Axes tool picks the origin, then the red and
        // green directions; Esc or another tool brings the dialog back unchanged.
        d.picking = true;
        app.set_tool(crate::tools::ToolId::Select);
        app.tool = Box::new(crate::tools::AxesTool::for_new_component());
    }
    OPEN.with(|o| *o.borrow_mut() = if create_it || cancel { None } else { Some(d) });
}
