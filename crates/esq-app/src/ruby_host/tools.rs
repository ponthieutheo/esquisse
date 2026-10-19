//! `host.tools.*` : the active tool (Ruby tools pushed, popped, selected), the status bar and
//! the Measurements box, the cursor, the extents and context menu of tools and overlays
//! (`ruby_tools.rs`), and which observers Ruby holds. See `rb/46_observers_tools.rb`.
//!
//! While a script runs the document is a placeholder: nothing here touches `app.doc`.

use esq_core::glam::DVec3;
use esq_core::model::{EntityRef, Model};
use serde_json::{Value as J, json};

use crate::app::EsquisseApp;
use crate::ruby_tools::{State, native_info};
use crate::tools::ruby::RubyTool;
use crate::tools::{Tool, ToolId};

const INCH: f64 = 25.4;

/// What the operations change: the active tool, the tool state, the Measurements box being
/// typed.
pub struct ToolsCx<'a> {
    pub tool: &'a mut Box<dyn Tool>,
    pub st: &'a mut State,
    pub vcb: &'a mut String,
}

#[allow(clippy::ptr_arg)]
pub fn call(
    app: &mut EsquisseApp,
    _model: &mut Model,
    _sel: &mut Vec<EntityRef>,
    op: &str,
    a: &J,
) -> Result<J, String> {
    let mut cx = ToolsCx { tool: &mut app.tool, st: &mut app.ui_state.extra.ruby_tools, vcb: &mut app.vcb };
    cx.call(op, a)
}

fn v3(j: &J) -> Option<DVec3> {
    let a = j.as_array().filter(|a| a.len() >= 3)?;
    Some(DVec3::new(a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?))
}

fn ruby_tool(args: &J) -> Box<dyn Tool> {
    Box::new(RubyTool::new(args["token"].as_u64().unwrap_or(0), args["id"].as_u64().unwrap_or(0) as u32))
}

impl ToolsCx<'_> {
    /// The tool now active is the one Ruby knows about: no « the user changed tool » event.
    fn report(&mut self) {
        let token = self.tool.ruby().map_or(0, |s| s.borrow().token);
        self.st.reported = Some((self.tool.id(), token));
    }

    /// Replaces the active tool (`Model#select_tool`): the suspended ones are dropped.
    fn select(&mut self, t: Box<dyn Tool>) {
        self.st.suspended.clear();
        self.st.clear_preview = true;
        self.st.cursor = 0;
        *self.tool = t;
        self.vcb.clear();
        self.report();
    }

    pub fn call(&mut self, op: &str, a: &J) -> Result<J, String> {
        let name = op.strip_prefix("tools.").ok_or_else(|| format!("opération hôte inconnue: {op}"))?;
        Ok(match name {
            "observing" => {
                let kinds = a["kinds"].as_array().into_iter().flatten().filter_map(J::as_str).map(String::from);
                self.st.observing = kinds.collect();
                J::Null
            }
            "select" => {
                self.select(ruby_tool(a));
                J::Null
            }
            "select_native" => {
                self.select(ToolId::Select.make());
                J::Null
            }
            "push" => {
                let old = std::mem::replace(self.tool, ruby_tool(a));
                self.st.suspended.push(old);
                self.st.clear_preview = true;
                self.vcb.clear();
                self.report();
                J::Null
            }
            "pop" => {
                *self.tool = self.st.suspended.pop().unwrap_or_else(|| ToolId::Select.make());
                self.vcb.clear();
                self.st.cursor = 0;
                self.report();
                J::Null
            }
            "active" => {
                let (id, name) = match self.tool.ruby() {
                    Some(s) => (s.borrow().id, "RubyTool"),
                    None => native_info(self.tool.id()),
                };
                json!({ "id": id, "name": name })
            }
            "caps" => {
                let token = a["token"].as_u64().unwrap_or(0);
                let shared = std::iter::once(&*self.tool).chain(self.st.suspended.iter()).filter_map(|t| t.ruby());
                for sh in shared {
                    let mut s = sh.borrow_mut();
                    if s.token == token {
                        s.vcb = a["vcb"].as_bool().unwrap_or(false);
                        s.middle = a["middle"].as_bool().unwrap_or(false);
                    }
                }
                J::Null
            }
            "cursor" => {
                self.st.cursor = a["id"].as_i64().unwrap_or(0);
                J::Null
            }
            "invalidate" => {
                self.st.redraw = true;
                J::Null
            }
            "extents" => {
                let list = a["list"].as_array().into_iter().flatten();
                self.st.extents = list.filter_map(|e| Some((v3(&e[0])? * INCH, v3(&e[1])? * INCH))).collect();
                J::Null
            }
            "overlays" => {
                let list = a["list"].as_array().into_iter().flatten();
                let text = |o: &J, k: &str| o[k].as_str().unwrap_or("").to_string();
                self.st.overlay_info =
                    list.clone().map(|o| (text(o, "id"), (text(o, "description"), text(o, "source")))).collect();
                self.st.overlays = list
                    .map(|o| {
                        (
                            o["id"].as_str().unwrap_or("").to_string(),
                            o["name"].as_str().unwrap_or("").to_string(),
                            o["enabled"].as_bool().unwrap_or(false),
                        )
                    })
                    .collect();
                self.st.redraw = true;
                J::Null
            }
            _ => return Err(format!("opération hôte non implémentée: {op}")),
        })
    }
}
