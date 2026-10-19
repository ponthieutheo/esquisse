//! State and entry point of the dialogs added by File › Export (section slice, animation),
//! File › Print, View › Toolbars and Tools › Component options/attributes. Each dialog
//! lives in its own module; this file only gathers their state and shows them.

use eframe::egui;

use crate::app::EsquisseApp;

#[derive(Default)]
pub struct Extra {
    pub slice: Option<crate::export_section::SliceDialog>,
    pub anim: Option<crate::export_anim::AnimDialog>,
    pub print: Option<crate::print::PrintDialog>,
    pub toolbars: crate::toolbars::ToolbarConfig,
    pub attrs: Option<crate::attrs::AttrDialog>,
    pub ruby: crate::ruby_ui::RubyState,
    pub jobs: crate::offscreen::Jobs,
    pub view: crate::ruby_host::view::ViewState,
    /// File › Export › 2D Graphic: the options of the chosen file.
    pub export2d: Option<crate::export2d::Export2dDialog>,
    /// Same, for a PDF, EPS or AutoCAD drawing.
    pub export_vec: Option<crate::export_vec::VecDialog>,
    /// File › Import of a picture: "Use Image As".
    pub import_image: Option<crate::import2d::ImportImageDialog>,
    pub ruby_tools: crate::ruby_tools::State,
    /// Window › Model Info and Window › Preferences.
    pub dialogs: crate::dialogs::DialogState,
    /// File › Import, File › Export › 3D Model and their options.
    pub formats: crate::formats3d::State,
    pub ops: crate::edit_ops::OpsState,
    /// The right-click menu of the view while it is open.
    pub ctx_menu: crate::context_menu::State,
    /// SketchUp's file dialogs of File › Import / Export and their options windows.
    pub files: crate::file_flow::State,
    /// Tools › Comment, View › Comments and the Comments panel.
    pub comments: crate::comments::CommentsState,
    /// Window › Components › In Model: the definitions' thumbnails (`def_thumbs.rs`).
    pub def_thumbs: crate::def_thumbs::Cache,
}

/// Called once per frame from `ui::dialogs`.
pub fn show(app: &mut EsquisseApp, ctx: &egui::Context) {
    crate::export_section::show(app, ctx);
    crate::export2d::show(app, ctx);
    crate::export_vec::show(app, ctx);
    crate::import2d::show(app, ctx);
    crate::export_anim::show(app, ctx);
    crate::print::show(app, ctx);
    crate::toolbars::show(app, ctx);
    crate::attrs::show(app, ctx);
    crate::ruby_ui::show(app, ctx);
    crate::ruby_tools::show(app, ctx);
    crate::dialogs::show(app, ctx);
    crate::formats3d::show(app, ctx);
    crate::edit_ops::show(app, ctx);
    crate::file_flow::show(app, ctx);
    crate::comments::show(app, ctx);
}
