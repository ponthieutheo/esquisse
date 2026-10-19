//! Keyboard shortcuts: SketchUp's defaults (`Cmd::default_shortcut`), changed in
//! Window › Preferences › Shortcuts and remembered in the preferences. Menus, the command
//! search, the keyboard and the Ruby API (`Sketchup.get_shortcuts`) all read them here.

use eframe::egui::{Key, Modifiers};

use crate::commands::Cmd;
use crate::settings;

pub type Shortcut = (Modifiers, Key);

/// Keys the application keeps for itself (measurements box, arrow locks, cancelling).
pub const RESERVED: [Key; 9] = [
    Key::Escape,
    Key::Enter,
    Key::Tab,
    Key::Backspace,
    Key::ArrowUp,
    Key::ArrowDown,
    Key::ArrowLeft,
    Key::ArrowRight,
    Key::F1,
];

/// The key under which a command's shortcuts are saved ("Tool(Line)", "Save"…).
pub fn key(cmd: Cmd) -> String {
    format!("{cmd:?}")
}

/// "Ctrl+Maj+K", as menus show it.
pub fn text((m, k): Shortcut) -> String {
    let mut s = String::new();
    if m.command || m.ctrl {
        s.push_str("Ctrl+");
    }
    if m.shift {
        s.push_str("Maj+");
    }
    if m.alt {
        s.push_str("Alt+");
    }
    s.push_str(match k {
        Key::Space => "Espace",
        Key::Delete => "Suppr",
        _ => k.name(),
    });
    s
}

/// Reads "Ctrl+Maj+K" back.
pub fn parse(t: &str) -> Option<Shortcut> {
    let mut m = Modifiers::NONE;
    let mut key = None;
    for part in t.split('+').map(str::trim) {
        match part {
            "Ctrl" | "Ctl" => m |= Modifiers::COMMAND,
            "Maj" | "Shift" => m |= Modifiers::SHIFT,
            "Alt" => m |= Modifiers::ALT,
            "Espace" => key = Some(Key::Space),
            "Suppr" => key = Some(Key::Delete),
            k => key = Some(Key::from_name(k)?),
        }
    }
    key.map(|k| (m, k))
}

/// The same shortcut, whichever way Ctrl was recorded.
pub fn same(a: Shortcut, b: Shortcut) -> bool {
    let norm = |m: Modifiers| (m.ctrl || m.command, m.shift, m.alt);
    a.1 == b.1 && norm(a.0) == norm(b.0)
}

/// The shortcuts of a command: the user's if they changed them, else SketchUp's.
pub fn bindings(cmd: Cmd) -> Vec<Shortcut> {
    let custom = settings::with(|s| s.shortcuts.get(&key(cmd)).cloned());
    match custom {
        Some(list) => list.iter().filter_map(|t| parse(t)).collect(),
        None => cmd.default_shortcut().into_iter().collect(),
    }
}

/// The command a key press runs.
pub fn cmd_for(k: Key, m: Modifiers) -> Option<Cmd> {
    crate::menus::all_commands().into_iter().find(|c| bindings(*c).iter().any(|b| same(*b, (m, k))))
}

/// The command (other than `except`) that already has this shortcut.
pub fn owner(s: Shortcut, except: Option<Cmd>) -> Option<Cmd> {
    crate::menus::all_commands()
        .into_iter()
        .filter(|c| Some(*c) != except)
        .find(|c| bindings(*c).iter().any(|b| same(*b, s)))
}

/// Gives a command one more shortcut, taking it from the command that had it.
pub fn assign(cmd: Cmd, s: Shortcut) {
    let previous = owner(s, Some(cmd));
    let mut mine = bindings(cmd);
    if !mine.iter().any(|b| same(*b, s)) {
        mine.push(s);
    }
    settings::update(|st| {
        if let Some(o) = previous {
            let left: Vec<String> = bindings(o).into_iter().filter(|b| !same(*b, s)).map(text).collect();
            st.shortcuts.insert(key(o), left);
        }
        st.shortcuts.insert(key(cmd), mine.into_iter().map(text).collect());
    });
}

/// Takes a shortcut away from a command.
pub fn remove(cmd: Cmd, s: Shortcut) {
    let left: Vec<String> = bindings(cmd).into_iter().filter(|b| !same(*b, s)).map(text).collect();
    settings::update(|st| {
        st.shortcuts.insert(key(cmd), left);
    });
}

/// Reset All / Reset Shortcuts: back to SketchUp's shortcuts.
pub fn reset_all() {
    settings::update(|st| st.shortcuts.clear());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolId;

    #[test]
    fn shortcuts_read_back_and_move_between_commands() {
        reset_all();
        assert_eq!(parse("Ctrl+Maj+I"), Some((Modifiers::COMMAND | Modifiers::SHIFT, Key::I)));
        assert_eq!(parse("Espace"), Some((Modifiers::NONE, Key::Space)));
        assert_eq!(text((Modifiers::COMMAND, Key::S)), "Ctrl+S");
        // L draws lines; giving L to the rectangle takes it from the line.
        let l = (Modifiers::NONE, Key::L);
        assert_eq!(cmd_for(Key::L, Modifiers::NONE), Some(Cmd::Tool(ToolId::Line)));
        assert_eq!(owner(l, Some(Cmd::Tool(ToolId::Rect))), Some(Cmd::Tool(ToolId::Line)));
        assign(Cmd::Tool(ToolId::Rect), l);
        assert_eq!(cmd_for(Key::L, Modifiers::NONE), Some(Cmd::Tool(ToolId::Rect)));
        assert!(bindings(Cmd::Tool(ToolId::Line)).is_empty());
        assert_eq!(bindings(Cmd::Tool(ToolId::Rect)).len(), 2, "R et L");
        remove(Cmd::Tool(ToolId::Rect), (Modifiers::NONE, Key::R));
        assert_eq!(cmd_for(Key::R, Modifiers::NONE), None);
        reset_all();
        assert_eq!(cmd_for(Key::R, Modifiers::NONE), Some(Cmd::Tool(ToolId::Rect)));
    }
}
