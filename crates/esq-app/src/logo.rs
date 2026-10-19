//! The logo of Esquisse in the application: the icon of the window and of its taskbar button,
//! and the brand at the top of the welcome window. The files come from `assets/logo` through
//! `build.rs` (the icon of the exe too); `logo/draw.rs` draws them and writes them:
//! `ESQ_WRITE_LOGO=<logo> cargo test -p esq-app logo` (see there). Without these files the
//! window keeps eframe's icon and the welcome window has no brand.

use eframe::egui;

#[cfg(test)]
pub(crate) mod draw;

pub(crate) mod files {
    include!(concat!(env!("OUT_DIR"), "/logo_files.rs"));
}

/// Height of the logo with its word in the welcome window, in points (the picture is twice that
/// in pixels: sharp up to a 200 % display scale).
const BRAND_HEIGHT: f32 = 32.0;
/// The icon group of the exe's resources (`winres::GROUP_ID`, written by `build.rs`).
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) const EXE_ICON_GROUP: u16 = 1;

fn decode(png: &[u8]) -> Option<image::RgbaImage> {
    image::load_from_memory_with_format(png, image::ImageFormat::Png).ok().map(|i| i.to_rgba8())
}

fn icon_from(png: &[u8]) -> Option<egui::IconData> {
    let img = decode(png)?;
    Some(egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() })
}

/// The icon given to the window (`ViewportBuilder::with_icon`), if the build has a logo.
pub fn icon() -> Option<egui::IconData> {
    files::ICON_PNG.and_then(icon_from)
}

/// Windows: the window takes the exe's own icon (`build.rs`) at the very sizes of the title bar
/// and of the taskbar, picked by Windows among the images of the resource, instead of the one
/// picture winit hands it and Windows shrinks. Nothing when the exe has no icon.
#[cfg(windows)]
pub fn use_exe_icon(cc: &eframe::CreationContext<'_>) {
    use eframe::egui_wgpu::wgpu::rwh::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        GWLP_HINSTANCE, GetSystemMetrics, GetWindowLongPtrW, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_DEFAULTCOLOR,
        LoadImageW, SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON, SendMessageW, WM_SETICON,
    };
    let Ok(handle) = cc.window_handle() else { return };
    let RawWindowHandle::Win32(h) = handle.as_raw() else { return };
    let hwnd = HWND(h.hwnd.get() as *mut core::ffi::c_void);
    // SAFETY: a live window of this process; the icon group is resource 1 of the exe.
    unsafe {
        let module = HINSTANCE(GetWindowLongPtrW(hwnd, GWLP_HINSTANCE) as *mut core::ffi::c_void);
        let group = windows::core::PCWSTR(EXE_ICON_GROUP as usize as *const u16);
        for (kind, cx, cy) in [(ICON_SMALL, SM_CXSMICON, SM_CYSMICON), (ICON_BIG, SM_CXICON, SM_CYICON)] {
            let size = (GetSystemMetrics(cx), GetSystemMetrics(cy));
            if let Ok(icon) = LoadImageW(Some(module), group, IMAGE_ICON, size.0, size.1, LR_DEFAULTCOLOR) {
                SendMessageW(hwnd, WM_SETICON, Some(WPARAM(kind as usize)), Some(LPARAM(icon.0 as isize)));
            }
        }
    }
}

/// The logo and its word, in the variant of the current theme: the brand of the welcome window.
/// Nothing when the build has no logo.
pub fn brand(ui: &mut egui::Ui) -> Option<egui::Response> {
    brand_with(ui, files::WORD_LIGHT_PNG?, files::WORD_DARK_PNG?)
}

fn brand_with(ui: &mut egui::Ui, light: &[u8], dark_png: &[u8]) -> Option<egui::Response> {
    let dark = ui.visuals().dark_mode;
    let id = egui::Id::unique(("esquisse-logo-mot", dark));
    let tex = match ui.ctx().data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        Some(t) => t,
        None => {
            let img = decode(if dark { dark_png } else { light })?;
            let size = [img.width() as usize, img.height() as usize];
            let t = crate::gpu_img::load_texture(ui.ctx(), "esquisse-logo-mot", size, img.as_raw(), Default::default());
            ui.ctx().data_mut(|d| d.insert_temp(id, t.clone()));
            t
        }
    };
    let [w, h] = tex.size();
    let size = egui::vec2(BRAND_HEIGHT * w as f32 / h.max(1) as f32, BRAND_HEIGHT);
    Some(ui.add(egui::Image::new((tex.id(), size)).alt_text("Esquisse")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use draw::Logo;

    #[test]
    fn the_window_icon_is_the_logo_at_256_pixels() {
        let png = draw::png(256, 256, &draw::render_icon(Logo::Compas, 256));
        let data = icon_from(&png).unwrap();
        assert_eq!((data.width, data.height, data.rgba.len()), (256, 256, 256 * 256 * 4));
        let px = |x: usize, y: usize| &data.rgba[(y * 256 + x) * 4..(y * 256 + x) * 4 + 4];
        // Rounded corner: clear; the tile under the arc: red chalk; the head of the compass: paper.
        assert_eq!(px(1, 1)[3], 0);
        assert_eq!(px(128, 240), [0xB4, 0x44, 0x2B, 255]);
        assert_eq!(px(128, 60), [0xF2, 0xED, 0xE3, 255]);
        assert!(icon_from(b"pas un PNG").is_none());
        // The build's icon, when it has one, is a 256-pixel square too.
        if let Some(i) = icon() {
            assert_eq!((i.width, i.height), (256, 256));
        }
    }

    #[test]
    fn the_welcome_brand_shows_the_logo_and_its_word_in_the_theme() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        for logo in Logo::ALL {
            let (light, dark) = draw::word_pngs(logo).expect("police du logo");
            for theme in [egui::Theme::Light, egui::Theme::Dark] {
                ctx.set_theme(theme);
                ctx.data_mut(|d| d.clear());
                let mut rect = None;
                let mut out = ctx.run_ui(Default::default(), |ui| rect = brand_with(ui, &light, &dark).map(|r| r.rect));
                out.textures_delta.clear();
                let rect = rect.unwrap();
                assert_eq!(rect.height(), BRAND_HEIGHT);
                // The word goes with the logo: the picture is three to four times as wide as high.
                assert!((2.8..4.2).contains(&(rect.width() / rect.height())), "{rect:?}");
                let update = out.platform_output.accesskit_update.expect("accessibilité");
                assert!(update.nodes.iter().any(|(_, n)| n.label() == Some("Esquisse")));
                let id = egui::Id::unique(("esquisse-logo-mot", theme == egui::Theme::Dark));
                assert!(ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)).is_some());
            }
        }
        // The build's brand: there when the build has a logo, nothing otherwise.
        let mut out = ctx.run_ui(Default::default(), |ui| {
            assert_eq!(brand(ui).is_some(), files::WORD_LIGHT_PNG.is_some() && files::WORD_DARK_PNG.is_some());
        });
        out.textures_delta.clear();
    }
}
