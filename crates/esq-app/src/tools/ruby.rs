//! A tool written in Ruby (`Model#select_tool`, `Tools#push_tool`). This side only forwards:
//! each mouse, keyboard and Measurements event is queued in the state shared with the app,
//! which hands the queue to Ruby once per frame (`ruby_tools.rs`) where the tool's
//! `onMouseMove`, `onLButtonDown`, `onUserText`… run. What the tool draws goes through the
//! view's drawing list (`ruby_host/view.rs`).

use std::cell::RefCell;
use std::rc::Rc;

use eframe::egui;
use esq_core::glam::DVec2;
use esq_core::model::Units;
use serde_json::{Value as J, json};

use super::{ButtonEvent, Tool, ToolCx, ToolId};

/// SketchUp's mouse flags (`MK_*`, `CONSTRAIN_MODIFIER_MASK`…).
pub const MK_LBUTTON: u32 = 1;
pub const MK_RBUTTON: u32 = 2;
pub const MK_SHIFT: u32 = 4;
pub const MK_CONTROL: u32 = 8;
pub const MK_MBUTTON: u32 = 16;
pub const MK_ALT: u32 = 32;

/// What the app and Ruby share about the active Ruby tool.
#[derive(Debug, Default)]
pub struct RubyShared {
    /// The tool object's key on the Ruby side (`Esq::Tools.registry`).
    pub token: u64,
    /// `Tools#active_tool_id` (50000 and up).
    pub id: u32,
    /// Events waiting for Ruby: `["onMouseMove", flags, x, y]`… (pixels).
    pub queue: Vec<J>,
    /// `enableVCB?` (or the tool answers `onUserText`).
    pub vcb: bool,
    /// The tool answers `onMButtonDown` and `onMButtonUp`: the middle button is its own.
    pub middle: bool,
    pub status: String,
    pub vcb_label: String,
    pub vcb_value: String,
    /// Pixels per egui point (Ruby coordinates are pixels).
    pub ppp: f32,
    pub buttons: u32,
}

impl RubyShared {
    pub fn push(&mut self, ev: J) {
        // Only the last position matters between two frames.
        if ev[0] == "onMouseMove"
            && let Some(last) = self.queue.last_mut()
            && last[0] == "onMouseMove"
        {
            *last = ev;
            return;
        }
        self.queue.push(ev);
    }

    fn px(&self, m: DVec2) -> (f64, f64) {
        let k = if self.ppp > 0.0 { self.ppp as f64 } else { 1.0 };
        (m.x * k, m.y * k)
    }
}

/// Mouse flags for the buttons held and the modifiers.
pub fn flags(buttons: u32, mods: egui::Modifiers) -> u32 {
    let mut f = buttons;
    if mods.shift {
        f |= MK_SHIFT;
    }
    if mods.ctrl || mods.command {
        f |= MK_CONTROL;
    }
    if mods.alt {
        f |= MK_ALT;
    }
    f
}

pub struct RubyTool {
    pub sh: Rc<RefCell<RubyShared>>,
}

impl RubyTool {
    pub fn new(token: u64, id: u32) -> Self {
        RubyTool { sh: Rc::new(RefCell::new(RubyShared { token, id, ..Default::default() })) }
    }

    fn mouse(&mut self, cx: &ToolCx, name: &str, m: DVec2) {
        let mut s = self.sh.borrow_mut();
        let (x, y) = s.px(m);
        let f = flags(s.buttons, cx.mods);
        s.push(json!([name, f, x, y]));
    }
}

impl Tool for RubyTool {
    fn id(&self) -> ToolId {
        ToolId::Ruby
    }

    fn status(&self) -> String {
        self.sh.borrow().status.clone()
    }

    fn vcb_label_text(&self) -> String {
        self.sh.borrow().vcb_label.clone()
    }

    fn vcb_value(&self, _units: Units) -> String {
        self.sh.borrow().vcb_value.clone()
    }

    fn mouse_move(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse(cx, "onMouseMove", m);
    }

    fn press(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.sh.borrow_mut().buttons |= MK_LBUTTON;
        self.mouse(cx, "onLButtonDown", m);
    }

    fn release(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.sh.borrow_mut().buttons &= !MK_LBUTTON;
        self.mouse(cx, "onLButtonUp", m);
    }

    fn double_click(&mut self, cx: &mut ToolCx, m: DVec2) {
        self.mouse(cx, "onLButtonDoubleClick", m);
    }

    fn button(&mut self, cx: &mut ToolCx, m: DVec2, button: u8, what: ButtonEvent) {
        let (bit, prefix) = match button {
            2 => (MK_RBUTTON, "onRButton"),
            3 if self.sh.borrow().middle => (MK_MBUTTON, "onMButton"),
            _ => return,
        };
        let suffix = match what {
            ButtonEvent::Down => {
                self.sh.borrow_mut().buttons |= bit;
                "Down"
            }
            ButtonEvent::Up => {
                self.sh.borrow_mut().buttons &= !bit;
                "Up"
            }
            ButtonEvent::DoubleClick => "DoubleClick",
        };
        self.mouse(cx, &format!("{prefix}{suffix}"), m);
    }

    fn wants_middle(&self) -> bool {
        self.sh.borrow().middle
    }

    fn key(&mut self, cx: &mut ToolCx, vk: u32, down: bool, repeat: u32) {
        let mut s = self.sh.borrow_mut();
        let f = flags(s.buttons, cx.mods);
        s.push(json!([if down { "onKeyDown" } else { "onKeyUp" }, vk, repeat, f]));
    }

    fn vcb(&mut self, _cx: &mut ToolCx, text: &str) -> Result<(), String> {
        let mut s = self.sh.borrow_mut();
        if !s.vcb {
            return Err("Cet outil n'utilise pas la boîte de mesures".into());
        }
        s.push(json!(["onUserText", text]));
        Ok(())
    }

    fn enter(&mut self, _cx: &mut ToolCx) {
        self.sh.borrow_mut().push(json!(["onReturn"]));
    }

    /// Escape: `onCancel(0, view)`; the tool stays active, as in SketchUp.
    fn cancel(&mut self, _cx: &mut ToolCx) {
        self.sh.borrow_mut().push(json!(["onCancel", 0]));
    }

    /// Always: Escape goes to `cancel` (onCancel) instead of switching to Select.
    fn busy(&self) -> bool {
        true
    }

    fn ruby(&self) -> Option<Rc<RefCell<RubyShared>>> {
        Some(self.sh.clone())
    }
}

/// Windows virtual-key code of a key (what `onKeyDown` receives).
pub fn vk_of(k: egui::Key) -> Option<u32> {
    use egui::Key::*;
    let name = k.name();
    if name.len() == 1 {
        let c = name.chars().next()?;
        if c.is_ascii_uppercase() || c.is_ascii_digit() {
            return Some(c as u32);
        }
    }
    Some(match k {
        Backspace => 8,
        Tab => 9,
        Enter => 13,
        Escape => 27,
        Space => 32,
        PageUp => 33,
        PageDown => 34,
        End => 35,
        Home => 36,
        ArrowLeft => 37,
        ArrowUp => 38,
        ArrowRight => 39,
        ArrowDown => 40,
        Insert => 45,
        Delete => 46,
        F1 => 112,
        F2 => 113,
        F3 => 114,
        F4 => 115,
        F5 => 116,
        F6 => 117,
        F7 => 118,
        F8 => 119,
        F9 => 120,
        F10 => 121,
        F11 => 122,
        F12 => 123,
        Semicolon => 186,
        Equals | Plus => 187,
        Comma => 188,
        Minus => 189,
        Period => 190,
        Slash => 191,
        Backtick => 192,
        OpenBracket => 219,
        Backslash => 220,
        CloseBracket => 221,
        Quote => 222,
        _ => return None,
    })
}
