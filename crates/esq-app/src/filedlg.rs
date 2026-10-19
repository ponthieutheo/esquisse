//! File dialogs that say which file type was chosen, like SketchUp's "Save as type" /
//! "Export type" lists (several types can share an extension: .skp versions), with the custom
//! controls of SketchUp's `CNativeFileDialog` subclasses (`CExportDialog`, `CImportDialog`…):
//!
//! - an "Options..." button (control 0xac5, made prominent: next to OK) that opens the options
//!   of the chosen type while the dialog stays open; shown or hidden as the type changes;
//! - the OK button renamed ("Exporter", "Importer");
//! - the "Utiliser image comme :" group (0xbdd) and its radio buttons (0xb56 image, 0xb57
//!   Texture, 0xb58 Nouvelle photo adaptée), shown for pictures only.
//!
//! On Windows the dialog is `IFileDialog` with `IFileDialogCustomize` and its events. A dialog
//! with an Options button runs on its own thread: the application keeps drawing and shows the
//! options over the dialog (`file_flow.rs`). Elsewhere `rfd` is used, the type is guessed from
//! the extension and the options are shown before or after the dialog.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::mpsc;

use eframe::egui;

/// Control identifiers of SketchUp's file dialogs.
pub const ID_OPTIONS: u32 = 0xac5;
pub const ID_IMAGE_GROUP: u32 = 0xbdd;
pub const ID_IMAGE_AS: u32 = 0xb56;
/// The radio items of "Utiliser image comme :", in the order of `import2d::ImageUse`.
pub const IMAGE_ITEMS: [u32; 3] = [0xb56, 0xb57, 0xb58];
/// Their labels, as SketchUp's Localizable.strings gives them ("Image" is "image").
pub const IMAGE_LABELS: [&str; 3] = ["image", "Texture", "Nouvelle photo adaptée"];
pub const IMAGE_GROUP_LABEL: &str = "Utiliser image comme :";
pub const OPTIONS_LABEL: &str = "Options...";

/// The choice of an "Utiliser image comme :" item (`CImportDialog::OnItemSelected`).
pub fn image_use_of_item(item: u32) -> Option<usize> {
    IMAGE_ITEMS.iter().position(|i| *i == item)
}

/// The item selected for a choice (`CImportDialog`: 1 texture, 2 photo, anything else image).
pub fn image_item(use_index: usize) -> u32 {
    IMAGE_ITEMS.get(use_index).copied().unwrap_or(IMAGE_ITEMS[0])
}

/// One entry of the type list: label shown, extensions without the dot (the first one is
/// added to a name typed without extension).
#[derive(Clone, Debug, PartialEq)]
pub struct FileType {
    pub label: String,
    pub exts: Vec<&'static str>,
}

impl FileType {
    pub fn new(label: impl Into<String>, exts: &[&'static str]) -> FileType {
        FileType { label: label.into(), exts: exts.to_vec() }
    }

    /// "Image PNG (*.png)".
    pub fn title(&self) -> String {
        if self.label.contains("(*.") {
            self.label.clone()
        } else {
            format!("{} ({})", self.label, self.patterns(", "))
        }
    }

    /// "*.jpg;*.jpeg".
    pub fn patterns(&self, sep: &str) -> String {
        let pats: Vec<String> = self.exts.iter().map(|e| format!("*.{e}")).collect();
        pats.join(sep)
    }
}

/// `path` with the extension of `t` when it has none of its extensions.
pub fn with_type_extension(path: &Path, t: &FileType) -> PathBuf {
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
    if t.exts.iter().any(|e| *e == ext) || t.exts.is_empty() {
        path.to_path_buf()
    } else {
        let mut s = path.as_os_str().to_owned();
        s.push(".");
        s.push(t.exts[0]);
        PathBuf::from(s)
    }
}

/// Index of the first type that has the extension of `path`.
pub fn type_of_path(path: &Path, types: &[FileType]) -> Option<usize> {
    let ext = path.extension().and_then(|e| e.to_str())?.to_lowercase();
    types.iter().position(|t| t.exts.contains(&ext.as_str()))
}

/// The file and type of a save dialog: a name typed with the extension of another type of
/// the list takes that type; otherwise the selected type's extension is added if missing.
pub fn chosen(path: PathBuf, selected: usize, types: &[FileType]) -> (PathBuf, usize) {
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
    if types[selected].exts.contains(&ext.as_str()) {
        return (path, selected);
    }
    match type_of_path(&path, types) {
        Some(i) => (path, i),
        None => (with_type_extension(&path, &types[selected]), selected),
    }
}

/// The custom controls a type shows (SketchUp's `OnTypeChange` / `OnSelectionChange`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Controls {
    /// "Options...".
    pub options: bool,
    /// "Utiliser image comme :".
    pub image: bool,
}

/// What a type shows, from its index in the list and the name of the file selected or typed
/// ("All supported types" takes the importer of the file's extension).
pub type ControlsFn = Box<dyn Fn(usize, &str) -> Controls + Send>;

/// A file dialog to show.
pub struct Request {
    pub save: bool,
    pub title: String,
    /// Label of the OK button (`None`: "Enregistrer" / "Ouvrir").
    pub ok_label: Option<String>,
    pub file_name: String,
    pub types: Vec<FileType>,
    /// The type selected when the dialog opens.
    pub initial: usize,
    /// The folder the dialog starts in.
    pub dir: Option<PathBuf>,
    /// An "Options..." button.
    pub options: bool,
    /// The "Utiliser image comme :" group, with the item selected when the dialog opens.
    pub image_use: Option<usize>,
    pub controls: ControlsFn,
}

impl Request {
    fn new(save: bool, title: &str, file_name: &str, types: Vec<FileType>, initial: usize, kind: &str) -> Request {
        let initial = initial.min(types.len().saturating_sub(1));
        Request {
            save,
            title: title.into(),
            ok_label: None,
            file_name: file_name.into(),
            types,
            initial,
            // Preferences › Files gives the folder of each kind ("models", "export"…).
            dir: crate::settings::with(|s| s.location(kind)),
            options: false,
            image_use: None,
            controls: Box::new(|_, _| Controls::default()),
        }
    }

    /// A save dialog (`CNativeFileSaveDialog`).
    pub fn save(title: &str, file_name: &str, types: Vec<FileType>, initial: usize, kind: &str) -> Request {
        Request::new(true, title, file_name, types, initial, kind)
    }

    /// An open dialog (`CNativeFileOpenDialog`).
    pub fn open(title: &str, types: Vec<FileType>, initial: usize, kind: &str) -> Request {
        Request::new(false, title, "", types, initial, kind)
    }

    pub fn ok_label(mut self, label: &str) -> Request {
        self.ok_label = Some(label.into());
        self
    }

    /// Adds "Options..."; `controls` says, for each type, whether it and the image group show.
    pub fn options(mut self, controls: impl Fn(usize, &str) -> Controls + Send + 'static) -> Request {
        self.options = true;
        self.controls = Box::new(controls);
        self
    }

    /// Adds "Utiliser image comme :" with `use_index` selected.
    pub fn image_group(mut self, use_index: usize) -> Request {
        self.image_use = Some(use_index);
        self
    }

    /// The controls of type `i` for the file `name`.
    pub fn controls_of(&self, i: usize, name: &str) -> Controls {
        let c = (self.controls)(i, name);
        Controls { options: c.options && self.options, image: c.image && self.image_use.is_some() }
    }
}

/// What the dialog gave.
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub path: PathBuf,
    pub type_index: usize,
    /// "Utiliser image comme :" when it was shown at OK.
    pub image_use: Option<usize>,
}

/// A message of a dialog running on its own thread.
#[derive(Debug, PartialEq)]
pub enum Event {
    /// "Options..." was clicked with type `type_index` selected and `name` selected or typed.
    Options { type_index: usize, name: String },
    /// The dialog is closed (`None`: cancelled).
    Done(Option<Choice>),
}

/// A dialog open on its own thread.
pub struct Pending {
    rx: mpsc::Receiver<Event>,
    /// The dialog's window while it waits for an options window, else 0.
    dialog: Arc<AtomicIsize>,
}

impl Pending {
    pub fn try_recv(&self) -> Option<Event> {
        self.rx.try_recv().ok()
    }

    /// The window of the dialog (0 when unknown).
    pub fn window(&self) -> isize {
        self.dialog.load(Ordering::Acquire)
    }

    /// The options window is closed: the dialog takes the keyboard back.
    pub fn options_closed(&self) {
        #[cfg(windows)]
        win::reactivate(self.window());
    }

    /// A dialog whose events the test sends.
    #[cfg(test)]
    pub fn fake() -> (mpsc::Sender<Event>, Pending) {
        let (tx, rx) = mpsc::channel();
        (tx, Pending { rx, dialog: Arc::new(AtomicIsize::new(0)) })
    }
}

/// Opens the dialog on its own thread, so that the application keeps drawing and can show the
/// options of a type over it. `None` where there is no such dialog (not Windows).
pub fn spawn(req: Request, ctx: &egui::Context) -> Option<Pending> {
    #[cfg(windows)]
    {
        let owner = win::active_window();
        let (tx, rx) = mpsc::channel();
        let dialog = Arc::new(AtomicIsize::new(0));
        let slot = dialog.clone();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new().name("file-dialog".into()).spawn(move || {
            let send = move |e: Event| {
                let _ = tx.send(e);
                ctx.request_repaint();
            };
            let send: std::rc::Rc<dyn Fn(Event)> = std::rc::Rc::new(send);
            let events = send.clone();
            let choice = win::run(req, owner, slot.clone(), Box::new(move |e| events(e)));
            slot.store(0, Ordering::Release);
            send(Event::Done(choice));
        });
        spawned.ok().map(|_| Pending { rx, dialog })
    }
    #[cfg(not(windows))]
    {
        let _ = (req, ctx);
        None
    }
}

/// Shows a dialog and waits for it (modal to the active window). Without an options window
/// to show, it runs on the calling thread.
pub fn run(req: Request) -> Option<Choice> {
    #[cfg(windows)]
    {
        win::run(req, win::active_window(), Arc::new(AtomicIsize::new(0)), Box::new(|_| {}))
    }
    #[cfg(not(windows))]
    {
        let mut d = rfd::FileDialog::new().set_title(&req.title);
        if let Some(dir) = &req.dir {
            d = d.set_directory(dir);
        }
        for t in &req.types {
            d = d.add_filter(t.title(), &t.exts);
        }
        if req.save {
            let p = d.set_file_name(&req.file_name).save_file()?;
            let (path, type_index) = chosen(p, req.initial, &req.types);
            Some(Choice { path, type_index, image_use: None })
        } else {
            let path = d.pick_file()?;
            let type_index = type_of_path(&path, &req.types).unwrap_or(req.initial);
            Some(Choice { path, type_index, image_use: None })
        }
    }
}

/// Save dialog: the path and the index of the chosen type. `initial` is the type selected
/// when the dialog opens; the dialog starts in the folder Preferences › Files gives for
/// `kind` ("models", "export"…).
pub fn save(title: &str, file_name: &str, types: &[FileType], initial: usize, kind: &str) -> Option<(PathBuf, usize)> {
    if types.is_empty() {
        return None;
    }
    let c = run(Request::save(title, file_name, types.to_vec(), initial, kind))?;
    Some((c.path, c.type_index))
}

/// Gives the options window titled `title` to the dialog `dialog` (it stays in front of it)
/// and centres it there. False while that window does not exist yet.
pub fn adopt(title: &str, dialog: isize) -> bool {
    #[cfg(windows)]
    {
        win::adopt(title, dialog)
    }
    #[cfg(not(windows))]
    {
        let _ = (title, dialog);
        true
    }
}

#[cfg(windows)]
mod win {
    use std::cell::Cell;
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicIsize, Ordering};

    use windows::Win32::Foundation::{E_NOTIMPL, HWND, LPARAM, RECT};
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance, CoInitializeEx,
        CoTaskMemFree, CoUninitialize,
    };
    use windows::Win32::System::Ole::IOleWindow;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, GetActiveWindow};
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::{
        CDCONTROLSTATEF, CDCS_ENABLEDVISIBLE, CDCS_INACTIVE, FDE_OVERWRITE_RESPONSE, FDE_SHAREVIOLATION_RESPONSE,
        FILEOPENDIALOGOPTIONS, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR, FOS_NOREADONLYRETURN,
        FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST, FileOpenDialog, FileSaveDialog, IFileDialog, IFileDialogControlEvents,
        IFileDialogControlEvents_Impl, IFileDialogCustomize, IFileDialogEvents, IFileDialogEvents_Impl, IShellItem,
        SHCreateItemFromParsingName, SIGDN_FILESYSPATH,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumThreadWindows, GWLP_HWNDPARENT, GetWindowRect, GetWindowTextW, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
        SetForegroundWindow, SetWindowLongPtrW, SetWindowPos,
    };
    use windows_core::{BOOL, HSTRING, Interface, PCWSTR, Ref, implement};

    use super::{
        Choice, Controls, Event, ID_IMAGE_AS, ID_IMAGE_GROUP, ID_OPTIONS, IMAGE_GROUP_LABEL, IMAGE_ITEMS, IMAGE_LABELS,
        OPTIONS_LABEL, Request, chosen, image_item, image_use_of_item,
    };

    pub fn active_window() -> isize {
        // SAFETY: plain query on the calling thread.
        unsafe { GetActiveWindow().0 as isize }
    }

    fn hwnd(h: isize) -> HWND {
        HWND(h as *mut core::ffi::c_void)
    }

    /// The dialog's window enabled again and in front (after an options window).
    pub fn reactivate(dialog: isize) {
        if dialog != 0 {
            // SAFETY: the window belongs to the dialog's thread, which is waiting in its modal
            // loop; both calls are allowed across threads.
            unsafe {
                let _ = EnableWindow(hwnd(dialog), true);
                let _ = SetForegroundWindow(hwnd(dialog));
            }
        }
    }

    /// A path or text returned by the shell, freed.
    fn take_string(p: windows_core::PWSTR) -> String {
        // SAFETY: `p` is a string the shell allocated with CoTaskMemAlloc for the caller.
        unsafe {
            let s = p.to_string().unwrap_or_default();
            CoTaskMemFree(Some(p.0 as *const _));
            s
        }
    }

    /// The file selected or typed in the dialog.
    fn current_name(pfd: &IFileDialog) -> String {
        // SAFETY: COM calls on the dialog, on its own thread.
        unsafe {
            if let Ok(item) = pfd.GetCurrentSelection()
                && let Ok(p) = item.GetDisplayName(SIGDN_FILESYSPATH)
            {
                return take_string(p);
            }
            pfd.GetFileName().map(take_string).unwrap_or_default()
        }
    }

    fn type_index(pfd: &IFileDialog, count: usize) -> usize {
        // SAFETY: COM call on the dialog's thread; the index is 1-based.
        let i = unsafe { pfd.GetFileTypeIndex() }.unwrap_or(1).max(1) as usize - 1;
        i.min(count.saturating_sub(1))
    }

    fn state(on: bool) -> CDCONTROLSTATEF {
        // SketchUp hides what does not apply (`CDCS_INACTIVE`), it does not grey it.
        if on { CDCS_ENABLEDVISIBLE } else { CDCS_INACTIVE }
    }

    /// The dialog's state, shared by its event handlers.
    struct Shared {
        req: Request,
        types: usize,
        shown: Cell<Controls>,
        image_use: Cell<usize>,
        slot: Arc<AtomicIsize>,
        send: Box<dyn Fn(Event)>,
    }

    impl Shared {
        /// Shows or hides the custom controls for the type and file now chosen.
        fn update(&self, pfd: &IFileDialog) {
            let c = self.req.controls_of(type_index(pfd, self.types), &current_name(pfd));
            let Ok(cust) = pfd.cast::<IFileDialogCustomize>() else { return };
            // SAFETY: COM calls on the dialog's thread, on controls added before `Show`.
            unsafe {
                if self.req.options {
                    let _ = cust.SetControlState(ID_OPTIONS, state(c.options));
                }
                if self.req.image_use.is_some() {
                    let _ = cust.SetControlState(ID_IMAGE_GROUP, state(c.image));
                    let _ = cust.SetControlState(ID_IMAGE_AS, state(c.image));
                }
            }
            self.shown.set(c);
        }
    }

    #[implement(IFileDialogEvents, IFileDialogControlEvents)]
    struct Events {
        shared: Rc<Shared>,
    }

    impl Events {
        fn shared(&self) -> &Shared {
            &self.shared
        }
    }

    impl IFileDialogEvents_Impl for Events_Impl {
        fn OnFileOk(&self, _pfd: Ref<IFileDialog>) -> windows_core::Result<()> {
            Ok(())
        }

        fn OnFolderChanging(&self, _pfd: Ref<IFileDialog>, _folder: Ref<IShellItem>) -> windows_core::Result<()> {
            Ok(())
        }

        fn OnFolderChange(&self, _pfd: Ref<IFileDialog>) -> windows_core::Result<()> {
            Ok(())
        }

        fn OnSelectionChange(&self, pfd: Ref<IFileDialog>) -> windows_core::Result<()> {
            self.shared().update(pfd.ok()?);
            Ok(())
        }

        fn OnShareViolation(
            &self,
            _pfd: Ref<IFileDialog>,
            _psi: Ref<IShellItem>,
        ) -> windows_core::Result<FDE_SHAREVIOLATION_RESPONSE> {
            Err(E_NOTIMPL.into())
        }

        fn OnTypeChange(&self, pfd: Ref<IFileDialog>) -> windows_core::Result<()> {
            self.shared().update(pfd.ok()?);
            Ok(())
        }

        fn OnOverwrite(
            &self,
            _pfd: Ref<IFileDialog>,
            _psi: Ref<IShellItem>,
        ) -> windows_core::Result<FDE_OVERWRITE_RESPONSE> {
            Err(E_NOTIMPL.into())
        }
    }

    impl IFileDialogControlEvents_Impl for Events_Impl {
        fn OnItemSelected(&self, _pfdc: Ref<IFileDialogCustomize>, ctl: u32, item: u32) -> windows_core::Result<()> {
            if ctl == ID_IMAGE_AS
                && let Some(u) = image_use_of_item(item)
            {
                self.shared().image_use.set(u);
            }
            Ok(())
        }

        fn OnButtonClicked(&self, pfdc: Ref<IFileDialogCustomize>, ctl: u32) -> windows_core::Result<()> {
            if ctl != ID_OPTIONS {
                return Ok(());
            }
            let s = self.shared();
            let pfd: IFileDialog = pfdc.ok()?.cast()?;
            let window: IOleWindow = pfd.cast()?;
            // SAFETY: COM and window calls on the dialog's thread. The dialog is disabled
            // while the options window is open, as under SketchUp's modal options dialog;
            // `Pending::options_closed` enables it again.
            let h = unsafe {
                let h = window.GetWindow()?;
                let _ = EnableWindow(h, false);
                h
            };
            s.slot.store(h.0 as isize, Ordering::Release);
            (s.send)(Event::Options { type_index: type_index(&pfd, s.types), name: current_name(&pfd) });
            Ok(())
        }

        fn OnCheckButtonToggled(
            &self,
            _pfdc: Ref<IFileDialogCustomize>,
            _ctl: u32,
            _checked: BOOL,
        ) -> windows_core::Result<()> {
            Ok(())
        }

        fn OnControlActivating(&self, _pfdc: Ref<IFileDialogCustomize>, _ctl: u32) -> windows_core::Result<()> {
            Ok(())
        }
    }

    /// Shows the dialog of `req`, owned by `owner`, and waits for it. `slot` receives the
    /// dialog's window when Options is clicked; `send` gets the Options events.
    pub fn run(req: Request, owner: isize, slot: Arc<AtomicIsize>, send: Box<dyn Fn(Event)>) -> Option<Choice> {
        if req.types.is_empty() {
            return None;
        }
        // SAFETY: COM is initialised for this thread (already done on the window's thread,
        // where this only counts one more use) and released at the end.
        let init = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        let r = show(req, owner, slot, send);
        if init.is_ok() {
            // SAFETY: balances the successful CoInitializeEx above.
            unsafe { CoUninitialize() };
        }
        r.ok().flatten()
    }

    fn show(
        req: Request,
        owner: isize,
        slot: Arc<AtomicIsize>,
        send: Box<dyn Fn(Event)>,
    ) -> windows_core::Result<Option<Choice>> {
        // SAFETY: COM calls on this thread; every string passed lives until `Show` returns.
        unsafe {
            let clsid = if req.save { &FileSaveDialog } else { &FileOpenDialog };
            let dlg: IFileDialog = CoCreateInstance(clsid, None, CLSCTX_INPROC_SERVER)?;
            let mut flags = dlg.GetOptions()?.0 | FOS_FORCEFILESYSTEM.0 | FOS_NOCHANGEDIR.0 | FOS_NOREADONLYRETURN.0;
            flags |= if req.save { FOS_OVERWRITEPROMPT.0 } else { FOS_FILEMUSTEXIST.0 | FOS_PATHMUSTEXIST.0 };
            dlg.SetOptions(FILEOPENDIALOGOPTIONS(flags))?;

            let names: Vec<HSTRING> = req.types.iter().map(|t| HSTRING::from(t.title())).collect();
            let specs: Vec<HSTRING> = req.types.iter().map(|t| HSTRING::from(t.patterns(";"))).collect();
            let filters: Vec<COMDLG_FILTERSPEC> = names
                .iter()
                .zip(&specs)
                .map(|(n, s)| COMDLG_FILTERSPEC { pszName: PCWSTR(n.as_ptr()), pszSpec: PCWSTR(s.as_ptr()) })
                .collect();
            dlg.SetFileTypes(&filters)?;
            dlg.SetFileTypeIndex(req.initial as u32 + 1)?;
            // The extension added to a name typed without one (then the selected type's).
            if let Some(e) = req.types[req.initial].exts.first() {
                dlg.SetDefaultExtension(&HSTRING::from(*e))?;
            }
            dlg.SetTitle(&HSTRING::from(req.title.as_str()))?;
            if let Some(ok) = &req.ok_label {
                dlg.SetOkButtonLabel(&HSTRING::from(ok.as_str()))?;
            }
            if !req.file_name.is_empty() {
                dlg.SetFileName(&HSTRING::from(req.file_name.as_str()))?;
            }
            if let Some(dir) = &req.dir
                && let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(dir.as_os_str()), None)
            {
                dlg.SetFolder(&item)?;
            }

            let cust: IFileDialogCustomize = dlg.cast()?;
            if req.options {
                cust.AddPushButton(ID_OPTIONS, &HSTRING::from(OPTIONS_LABEL))?;
                cust.MakeProminent(ID_OPTIONS)?;
            }
            if let Some(u) = req.image_use {
                cust.StartVisualGroup(ID_IMAGE_GROUP, &HSTRING::from(IMAGE_GROUP_LABEL))?;
                cust.AddRadioButtonList(ID_IMAGE_AS)?;
                for (item, label) in IMAGE_ITEMS.iter().zip(IMAGE_LABELS) {
                    cust.AddControlItem(ID_IMAGE_AS, *item, &HSTRING::from(label))?;
                }
                cust.SetSelectedControlItem(ID_IMAGE_AS, image_item(u))?;
                cust.EndVisualGroup()?;
            }

            let shared = Rc::new(Shared {
                types: req.types.len(),
                shown: Cell::new(Controls::default()),
                image_use: Cell::new(req.image_use.unwrap_or(0).min(2)),
                slot,
                send,
                req,
            });
            // The controls of the initial type, before the dialog shows (`CImportDialog`).
            shared.update(&dlg);
            let events: IFileDialogEvents = Events { shared: shared.clone() }.into();
            let cookie = dlg.Advise(&events)?;
            let owner = (owner != 0).then(|| hwnd(owner));
            let shown = dlg.Show(owner);
            let _ = dlg.Unadvise(cookie);
            drop(events);
            if shown.is_err() {
                return Ok(None);
            }
            let path = PathBuf::from(take_string(dlg.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?));
            let req = &shared.req;
            let index = type_index(&dlg, req.types.len());
            let image_use = shared.shown.get().image.then(|| shared.image_use.get());
            let (path, type_index) = if req.save { chosen(path, index, &req.types) } else { (path, index) };
            Ok(Some(Choice { path, type_index, image_use }))
        }
    }

    struct Search {
        title: Vec<u16>,
        found: isize,
    }

    unsafe extern "system" fn by_title(h: HWND, data: LPARAM) -> BOOL {
        // SAFETY: `data` is the `Search` given to EnumThreadWindows below.
        let s = unsafe { &mut *(data.0 as *mut Search) };
        let mut buf = [0u16; 256];
        // SAFETY: plain query on a window of this thread.
        let n = unsafe { GetWindowTextW(h, &mut buf) }.max(0) as usize;
        if buf[..n] == s.title[..] {
            s.found = h.0 as isize;
            return false.into();
        }
        true.into()
    }

    pub fn adopt(title: &str, dialog: isize) -> bool {
        let mut s = Search { title: title.encode_utf16().collect(), found: 0 };
        // SAFETY: the callback only reads window titles of this thread and writes `s`.
        unsafe {
            let _ = EnumThreadWindows(GetCurrentThreadId(), Some(by_title), LPARAM(&mut s as *mut Search as isize));
        }
        if s.found == 0 {
            return false;
        }
        if dialog != 0 {
            let (w, d) = (hwnd(s.found), hwnd(dialog));
            // SAFETY: the window is ours; the dialog becomes its owner (it stays in front of
            // it) and it is centred over it.
            unsafe {
                SetWindowLongPtrW(w, GWLP_HWNDPARENT, dialog);
                let (mut a, mut b) = (RECT::default(), RECT::default());
                if GetWindowRect(d, &mut a).is_ok() && GetWindowRect(w, &mut b).is_ok() {
                    let x = (a.left + a.right - (b.right - b.left)) / 2;
                    let y = (a.top + a.bottom - (b.bottom - b.top)) / 2;
                    let _ = SetWindowPos(w, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn types() -> Vec<FileType> {
        vec![FileType::new("Image JPEG", &["jpg", "jpeg"]), FileType::new("Image PNG (*.png)", &["png"])]
    }

    #[test]
    fn types_show_their_patterns() {
        let t = types();
        assert_eq!(t[0].title(), "Image JPEG (*.jpg, *.jpeg)");
        assert_eq!(t[1].title(), "Image PNG (*.png)");
        assert_eq!(t[0].patterns(";"), "*.jpg;*.jpeg");
    }

    #[test]
    fn the_chosen_type_gives_the_extension() {
        let t = types();
        assert_eq!(with_type_extension(Path::new("C:/a/vue"), &t[1]), PathBuf::from("C:/a/vue.png"));
        assert_eq!(with_type_extension(Path::new("C:/a/vue.JPEG"), &t[0]), PathBuf::from("C:/a/vue.JPEG"));
        assert_eq!(with_type_extension(Path::new("C:/a/v.1"), &t[0]), PathBuf::from("C:/a/v.1.jpg"));
        assert_eq!(type_of_path(Path::new("x.Png"), &t), Some(1));
        assert_eq!(type_of_path(Path::new("x"), &t), None);
        // A name typed with another type's extension takes that type.
        assert_eq!(chosen(PathBuf::from("v.jpg"), 1, &t), (PathBuf::from("v.jpg"), 0));
        assert_eq!(chosen(PathBuf::from("v"), 1, &t), (PathBuf::from("v.png"), 1));
        assert_eq!(chosen(PathBuf::from("v.png"), 1, &t), (PathBuf::from("v.png"), 1));
        assert_eq!(chosen(PathBuf::from("v.b"), 0, &t), (PathBuf::from("v.b.jpg"), 0));
    }

    /// `CImportDialog::OnItemSelected` and `CImportDialog::SetImageAs`: the three radio items
    /// and the choices 0, 1, 2 (anything else selects "image").
    #[test]
    fn image_items_map_to_choices() {
        assert_eq!(IMAGE_ITEMS, [0xb56, 0xb57, 0xb58]);
        for (i, item) in IMAGE_ITEMS.iter().enumerate() {
            assert_eq!(image_use_of_item(*item), Some(i));
            assert_eq!(image_item(i), *item);
        }
        assert_eq!(image_use_of_item(ID_OPTIONS), None);
        assert_eq!(image_item(7), 0xb56);
        assert_eq!(IMAGE_LABELS, ["image", "Texture", "Nouvelle photo adaptée"]);
    }

    /// The controls of a type are only those the dialog has.
    #[test]
    fn a_request_shows_only_its_own_controls() {
        let all = |_: usize, _: &str| Controls { options: true, image: true };
        let r = Request::save("t", "a", types(), 9, "export").options(all);
        assert_eq!(r.initial, 1, "the initial type is in the list");
        assert_eq!(r.controls_of(0, ""), Controls { options: true, image: false });
        let r = Request::open("t", types(), 0, "models").image_group(2);
        assert_eq!(r.controls_of(0, ""), Controls::default(), "no Options button, nothing shown");
        let r = Request::open("t", types(), 0, "models").options(all).image_group(2).ok_label("Importer");
        assert_eq!(r.controls_of(1, "a.png"), Controls { options: true, image: true });
        assert_eq!((r.ok_label.as_deref(), r.image_use), (Some("Importer"), Some(2)));
    }
}
