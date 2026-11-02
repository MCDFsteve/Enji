//! Text that floats over everything else on the screen, such as desktop
//! lyrics or a line on the taskbar: a few lines, each with its size, weight
//! and colour, drawn with a soft shadow so they read on any background, on
//! a transparent window above the others.
//!
//! [`show`] puts up or changes the overlay of an id, [`hide`] takes it down.
//! Locked, clicks go through it to what lies under it; unlocked, a faint
//! plate shows behind the text and it can be dragged. Where it was left is
//! kept for the next time. [`Place::Taskbar`] stands it on the Windows
//! taskbar, beside the notification area; elsewhere it is a free overlay.
//!
//! macOS draws it on a borderless panel, Windows on a layered window,
//! Android on a system overlay once the user allows one ([`allowed`],
//! [`ask`]), Linux through the GTK host, which installs itself with
//! [`install`].

use std::sync::{Arc, RwLock};

/// One line of an overlay.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub text: String,
    /// In points.
    pub size: f64,
    pub bold: bool,
    /// Red, green, blue and alpha.
    pub color: [u8; 4],
}

/// Where an overlay stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Place {
    /// Where the user last left it; at first low in the middle of the main
    /// screen.
    #[default]
    Free,
    /// On the Windows taskbar, beside the notification area.
    Taskbar,
}

/// What an overlay shows and how.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Overlay {
    /// From the top.
    pub lines: Vec<Line>,
    pub place: Place,
    /// Clicks go through it; unlocked, it shows a plate and can be dragged.
    pub locked: bool,
}

/// A host that draws overlays itself (the GTK host on Linux).
pub trait Host: Send + Sync {
    fn show(&self, id: &str, overlay: &Overlay);
    fn hide(&self, id: &str);
}

static HOST: RwLock<Option<Arc<dyn Host>>> = RwLock::new(None);

/// Draw overlays through `host` from now on.
pub fn install(host: impl Host + 'static) {
    *HOST.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(host));
}

fn host() -> Option<Arc<dyn Host>> {
    HOST.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Whether this system shows overlays at all.
pub fn supported() -> bool {
    cfg!(any(target_os = "macos", target_os = "windows", target_os = "android")) || host().is_some()
}

/// Whether the user lets the application draw over other apps: always,
/// except on Android before the user allowed it.
pub fn allowed() -> bool {
    #[cfg(target_os = "android")]
    return crate::platform::request("overlay_allowed", "", "").is_ok_and(|answer| answer == "yes");
    #[cfg(not(target_os = "android"))]
    supported()
}

/// Open the system's page where the user allows drawing over other apps
/// (Android); nothing elsewhere.
pub fn ask() {
    #[cfg(target_os = "android")]
    let _ = crate::platform::request("overlay_ask", "", "");
}

/// Show `overlay` as the overlay `id`, putting it up when it is not.
pub fn show(id: &str, overlay: &Overlay) {
    match host() {
        Some(host) => host.show(id, overlay),
        None => show_here(id, overlay),
    }
}

/// Take the overlay `id` down.
pub fn hide(id: &str) {
    match host() {
        Some(host) => host.hide(id),
        None => hide_here(id),
    }
}

/// Show an overlay with this system's own windows.
fn show_here(id: &str, overlay: &Overlay) {
    #[cfg(target_os = "macos")]
    mac::show(id.to_string(), overlay.clone());
    #[cfg(target_os = "windows")]
    win::show(id.to_string(), overlay.clone());
    #[cfg(target_os = "android")]
    let _ = crate::platform::request("overlay_show", id, &as_json(overlay));
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "android")))]
    let _ = (id, overlay);
}

fn hide_here(id: &str) {
    #[cfg(target_os = "macos")]
    mac::hide(id.to_string());
    #[cfg(target_os = "windows")]
    win::hide(id.to_string());
    #[cfg(target_os = "android")]
    let _ = crate::platform::request("overlay_hide", id, "");
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "android")))]
    let _ = id;
}

/// Where the user left the overlay `id`: the middle of its bottom edge, in
/// points from the bottom left of the main screen.
pub fn saved_place(id: &str) -> Option<(f64, f64)> {
    let bytes = crate::storage::Store::host().read(&format!("overlay.{id}")).ok()??;
    let text = String::from_utf8(bytes).ok()?;
    let (x, y) = text.split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

/// Keep where the user left the overlay `id`.
pub fn save_place(id: &str, x: f64, y: f64) {
    let _ = crate::storage::Store::host().write(&format!("overlay.{id}"), format!("{x},{y}").as_bytes());
}

/// An overlay as JSON for a host: its lines, place and lock.
#[cfg(any(target_os = "android", test))]
fn as_json(overlay: &Overlay) -> String {
    fn text(value: &str) -> String {
        let mut out = String::from("\"");
        for c in value.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                c => out.push(c),
            }
        }
        out.push('"');
        out
    }
    let lines: Vec<String> = overlay
        .lines
        .iter()
        .map(|line| {
            let [r, g, b, a] = line.color;
            format!(
                "{{\"text\":{},\"size\":{},\"bold\":{},\"color\":\"#{a:02x}{r:02x}{g:02x}{b:02x}\"}}",
                text(&line.text),
                line.size,
                line.bold
            )
        })
        .collect();
    format!(
        "{{\"lines\":[{}],\"place\":\"{}\",\"locked\":{}}}",
        lines.join(","),
        match overlay.place {
            Place::Free => "free",
            Place::Taskbar => "taskbar",
        },
        overlay.locked
    )
}

/// macOS: a borderless, non-activating panel above the other windows, on
/// every space and over full-screen apps, with a label per line.
#[cfg(target_os = "macos")]
mod mac {
    use super::{Line, Overlay};
    use dispatch2::DispatchQueue;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, ProtocolObject};
    use objc2::{AnyThread, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSBackingStoreType, NSColor, NSFont, NSFontAttributeName, NSForegroundColorAttributeName, NSPanel, NSScreen,
        NSShadow, NSShadowAttributeName, NSTextField, NSView, NSWindowCollectionBehavior, NSWindowStyleMask,
    };
    use objc2_foundation::{NSAttributedString, NSMutableDictionary, NSPoint, NSRect, NSSize, NSString};
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// The margin around the text, and the gap between lines.
    const MARGIN: f64 = 12.0;
    const GAP: f64 = 2.0;

    thread_local! {
        static PANELS: RefCell<HashMap<String, Retained<NSPanel>>> = RefCell::new(HashMap::new());
    }

    pub fn show(id: String, overlay: Overlay) {
        DispatchQueue::main().exec_async(move || {
            if let Some(mtm) = MainThreadMarker::new() {
                put(mtm, &id, &overlay);
            }
        });
    }

    pub fn hide(id: String) {
        DispatchQueue::main().exec_async(move || {
            PANELS.with(|panels| {
                if let Some(panel) = panels.borrow().get(&id) {
                    keep_place(&id, panel);
                    panel.orderOut(None);
                }
            });
        });
    }

    /// Remember where the panel is: the middle of its bottom edge.
    fn keep_place(id: &str, panel: &NSPanel) {
        let frame = panel.frame();
        super::save_place(id, frame.origin.x + frame.size.width / 2.0, frame.origin.y);
    }

    fn label(line: &Line, mtm: MainThreadMarker) -> Retained<NSTextField> {
        let attributes = NSMutableDictionary::<NSString, AnyObject>::new();
        let font = if line.bold { NSFont::boldSystemFontOfSize(line.size) } else { NSFont::systemFontOfSize(line.size) };
        let [r, g, b, a] = line.color.map(|c| c as f64 / 255.0);
        let color = NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, a);
        let shadow = NSShadow::new();
        shadow.setShadowOffset(NSSize::new(0.0, -1.0));
        shadow.setShadowBlurRadius(3.0);
        shadow.setShadowColor(Some(&NSColor::colorWithWhite_alpha(0.0, 0.85)));
        // SAFETY: the keys are AppKit's attribute names, with values of their
        // kinds.
        unsafe {
            attributes.setObject_forKey(&font, ProtocolObject::from_ref(NSFontAttributeName));
            attributes.setObject_forKey(&color, ProtocolObject::from_ref(NSForegroundColorAttributeName));
            attributes.setObject_forKey(&shadow, ProtocolObject::from_ref(NSShadowAttributeName));
        }
        // SAFETY: string attributes as AppKit takes them.
        let text = unsafe {
            NSAttributedString::initWithString_attributes(NSAttributedString::alloc(), &NSString::from_str(&line.text), Some(&attributes))
        };
        let label = NSTextField::labelWithAttributedString(&text, mtm);
        label.sizeToFit();
        label
    }

    fn put(mtm: MainThreadMarker, id: &str, overlay: &Overlay) {
        let panel = PANELS.with(|panels| {
            panels
                .borrow_mut()
                .entry(id.to_string())
                .or_insert_with(|| {
                    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(200.0, 60.0));
                    let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
                        NSPanel::alloc(mtm),
                        frame,
                        NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
                        NSBackingStoreType::Buffered,
                        false,
                    );
                    panel.setOpaque(false);
                    panel.setHasShadow(false);
                    // A floating panel takes the floating level; the status
                    // level after it keeps it above other apps' panels too.
                    panel.setFloatingPanel(true);
                    panel.setLevel(objc2_app_kit::NSStatusWindowLevel);
                    panel.setCollectionBehavior(
                        NSWindowCollectionBehavior::CanJoinAllSpaces
                            | NSWindowCollectionBehavior::Stationary
                            | NSWindowCollectionBehavior::FullScreenAuxiliary
                            | NSWindowCollectionBehavior::IgnoresCycle,
                    );
                    panel.setMovableByWindowBackground(true);
                    panel.setHidesOnDeactivate(false);
                    panel.setBecomesKeyOnlyIfNeeded(true);
                    // SAFETY: the panel stays in `PANELS` and is never closed.
                    unsafe { panel.setReleasedWhenClosed(false) };
                    panel
                })
                .clone()
        });
        let shown = panel.isVisible();
        // Where it was: the middle of its bottom edge, kept as the text
        // changes width.
        let place = if shown {
            let frame = panel.frame();
            Some((frame.origin.x + frame.size.width / 2.0, frame.origin.y))
        } else {
            super::saved_place(id)
        };
        let labels: Vec<Retained<NSTextField>> = overlay.lines.iter().filter(|line| !line.text.is_empty()).map(|line| label(line, mtm)).collect();
        let width = labels.iter().map(|label| label.frame().size.width).fold(0.0, f64::max) + 2.0 * MARGIN;
        let height = labels.iter().map(|label| label.frame().size.height).sum::<f64>()
            + GAP * labels.len().saturating_sub(1) as f64
            + 2.0 * MARGIN;
        let content = NSView::initWithFrame(NSView::alloc(mtm), NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, height)));
        let mut top = height - MARGIN;
        for label in &labels {
            let size = label.frame().size;
            top -= size.height;
            label.setFrameOrigin(NSPoint::new((width - size.width) / 2.0, top));
            top -= GAP;
            content.addSubview(label);
        }
        panel.setContentView(Some(&content));
        let (x, y) = place.unwrap_or_else(|| {
            let screen = NSScreen::mainScreen(mtm).map(|screen| screen.visibleFrame()).unwrap_or(NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(1440.0, 900.0),
            ));
            (screen.origin.x + screen.size.width / 2.0, screen.origin.y + screen.size.height * 0.1)
        });
        panel.setFrame_display(NSRect::new(NSPoint::new(x - width / 2.0, y), NSSize::new(width, height)), true);
        let plate = if overlay.locked { NSColor::clearColor() } else { NSColor::colorWithWhite_alpha(0.0, 0.3) };
        panel.setBackgroundColor(Some(&plate));
        panel.setIgnoresMouseEvents(overlay.locked);
        if overlay.lines.iter().all(|line| line.text.is_empty()) {
            panel.orderOut(None);
        } else {
            panel.orderFrontRegardless();
        }
        keep_place(id, &panel);
    }
}

/// Windows: a layered, top-most tool window per overlay, drawn with
/// Direct2D and DirectWrite into a bitmap whose alpha the window takes, on
/// a thread of its own with its message loop. Unlocked, the whole window
/// drags; locked, it lets clicks through.
#[cfg(target_os = "windows")]
mod win {
    use super::{Overlay, Place};
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::sync::{Mutex, mpsc};
    use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
    use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT};
    use windows_numerics::Vector2;
    use windows::Win32::Graphics::Direct2D::{
        D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES,
        D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1CreateFactory,
        ID2D1Factory,
    };
    use windows::Win32::Graphics::DirectWrite::{
        DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_BOLD,
        DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_METRICS, DWriteCreateFactory, IDWriteFactory, IDWriteTextLayout,
    };
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
    use windows::Win32::Graphics::Gdi::{
        AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CreateCompatibleDC, CreateDIBSection,
        DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, HGDIOBJ, ReleaseDC, SelectObject,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, SetThreadDpiAwarenessContext};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowExW, FindWindowW, GWL_EXSTYLE, GetMessageW,
        GetWindowLongPtrW, GetWindowRect, HTCAPTION, HWND_TOPMOST, IDC_ARROW, LoadCursorW, MA_NOACTIVATE, MSG, PostThreadMessageW,
        RegisterClassExW, SPI_GETWORKAREA, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetTimer,
        SetWindowLongPtrW, SetWindowPos, ShowWindow, SystemParametersInfoW, TranslateMessage, ULW_ALPHA, UpdateLayeredWindow,
        WM_APP, WM_EXITSIZEMOVE, WM_MOUSEACTIVATE, WM_NCHITTEST, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
    };
    use windows::core::{HSTRING, w};

    enum Message {
        Show(String, Overlay),
        Hide(String),
    }

    static CHANNEL: Mutex<Option<(mpsc::Sender<Message>, u32)>> = Mutex::new(None);

    /// Around the text and between lines, in points.
    const MARGIN: f32 = 12.0;
    const GAP: f32 = 2.0;
    /// How often an overlay on the taskbar is put back on top of it.
    const TOPMOST_TIMER: usize = 1;

    pub fn show(id: String, overlay: Overlay) {
        send(Message::Show(id, overlay));
    }

    pub fn hide(id: String) {
        send(Message::Hide(id));
    }

    fn send(message: Message) {
        let mut channel = CHANNEL.lock().unwrap_or_else(|e| e.into_inner());
        if channel.is_none() {
            let (send, receive) = mpsc::channel();
            let (ready, started) = mpsc::channel();
            let spawned = std::thread::Builder::new().name("bern-overlay".into()).spawn(move || run(receive, ready));
            let Ok(_) = spawned else { return };
            let Ok(thread) = started.recv() else { return };
            *channel = Some((send, thread));
        }
        if let Some((sender, thread)) = channel.as_ref()
            && sender.send(message).is_ok()
        {
            // SAFETY: wakes the overlay thread's message loop.
            let _ = unsafe { PostThreadMessageW(*thread, WM_APP, WPARAM(0), LPARAM(0)) };
        }
    }

    /// An overlay's window and where it stands: the middle of its bottom
    /// edge, in pixels.
    struct Window {
        hwnd: HWND,
        overlay: Overlay,
        place: Option<(f64, f64)>,
    }

    thread_local! {
        static WINDOWS: RefCell<HashMap<String, Window>> = RefCell::new(HashMap::new());
    }

    extern "system" fn window_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match message {
            // Unlocked, the whole window is the part that drags it; locked,
            // the window lets clicks through and never asks.
            WM_NCHITTEST => LRESULT(HTCAPTION as isize),
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            WM_EXITSIZEMOVE => {
                let mut rect = RECT::default();
                // SAFETY: the window this procedure serves.
                if unsafe { GetWindowRect(hwnd, &mut rect) }.is_ok() {
                    let place = ((rect.left + rect.right) as f64 / 2.0, rect.bottom as f64);
                    WINDOWS.with(|windows| {
                        for (id, window) in windows.borrow_mut().iter_mut() {
                            if window.hwnd == hwnd {
                                window.place = Some(place);
                                super::save_place(id, place.0, place.1);
                            }
                        }
                    });
                }
                LRESULT(0)
            }
            WM_TIMER => {
                // SAFETY: keeps a window of this thread on top of the taskbar.
                let _ = unsafe { SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
                LRESULT(0)
            }
            // SAFETY: the default handling of a window of this thread.
            _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
        }
    }

    fn run(messages: mpsc::Receiver<Message>, ready: mpsc::Sender<u32>) {
        // SAFETY: this thread's own settings, window class and factories.
        unsafe {
            let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let instance = GetModuleHandleW(None).unwrap_or_default();
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(window_proc),
                hInstance: instance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                lpszClassName: w!("BernOverlay"),
                ..Default::default()
            };
            RegisterClassExW(&class);
            let Ok(d2d) = D2D1CreateFactory::<ID2D1Factory>(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) else { return };
            let Ok(dwrite) = DWriteCreateFactory::<IDWriteFactory>(DWRITE_FACTORY_TYPE_SHARED) else { return };
            let _ = ready.send(GetCurrentThreadId());
            let mut message = MSG::default();
            while GetMessageW(&mut message, None, 0, 0).as_bool() {
                if message.hwnd.is_invalid() && message.message == WM_APP {
                    while let Ok(next) = messages.try_recv() {
                        apply(next, &d2d, &dwrite, instance.into());
                    }
                    continue;
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    unsafe fn apply(
        message: Message,
        d2d: &ID2D1Factory,
        dwrite: &IDWriteFactory,
        instance: windows::Win32::Foundation::HINSTANCE,
    ) {
        match message {
            Message::Hide(id) => WINDOWS.with(|windows| {
                if let Some(window) = windows.borrow().get(&id) {
                    // SAFETY: a window of this thread.
                    let _ = unsafe { ShowWindow(window.hwnd, SW_HIDE) };
                }
            }),
            Message::Show(id, overlay) => WINDOWS.with(|windows| {
                let mut windows = windows.borrow_mut();
                let window = windows.entry(id.clone()).or_insert_with(|| {
                    // SAFETY: a window of the class this thread registered.
                    let hwnd = unsafe {
                        CreateWindowExW(
                            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                            w!("BernOverlay"),
                            w!(""),
                            WS_POPUP,
                            0,
                            0,
                            1,
                            1,
                            None,
                            None,
                            Some(instance),
                            None,
                        )
                    }
                    .unwrap_or_default();
                    Window { hwnd, overlay: Overlay::default(), place: super::saved_place(&id) }
                });
                window.overlay = overlay;
                // SAFETY: draws a window of this thread.
                let _ = unsafe { draw(window, d2d, dwrite) };
            }),
        }
    }

    /// The text of a line laid out, and its size in pixels.
    unsafe fn layout(dwrite: &IDWriteFactory, line: &super::Line, scale: f32) -> Option<(IDWriteTextLayout, f32, f32)> {
        let weight = if line.bold { DWRITE_FONT_WEIGHT_BOLD } else { DWRITE_FONT_WEIGHT_NORMAL };
        // SAFETY: DirectWrite objects made and used on this thread.
        unsafe {
            let format = dwrite
                .CreateTextFormat(
                    w!("Microsoft YaHei UI"),
                    None,
                    weight,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    line.size as f32 * scale,
                    w!(""),
                )
                .ok()?;
            let text: Vec<u16> = line.text.encode_utf16().collect();
            let layout = dwrite.CreateTextLayout(&text, &format, 8192.0, 8192.0).ok()?;
            let mut metrics = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut metrics).ok()?;
            Some((layout, metrics.widthIncludingTrailingWhitespace, metrics.height))
        }
    }

    /// Where an overlay on the taskbar goes: its right edge beside the
    /// notification area, its middle on the taskbar's, in pixels.
    unsafe fn taskbar_spot(width: i32, height: i32) -> Option<(i32, i32)> {
        // SAFETY: reads the shell's windows.
        unsafe {
            let tray = FindWindowW(w!("Shell_TrayWnd"), None).ok()?;
            let mut bar = RECT::default();
            GetWindowRect(tray, &mut bar).ok()?;
            let mut right = bar.right - 8;
            if let Ok(notify) = FindWindowExW(Some(tray), None, w!("TrayNotifyWnd"), None) {
                let mut area = RECT::default();
                if GetWindowRect(notify, &mut area).is_ok() {
                    right = area.left - 8;
                }
            }
            Some((right - width, (bar.top + bar.bottom) / 2 - height / 2))
        }
    }

    unsafe fn draw(window: &mut Window, d2d: &ID2D1Factory, dwrite: &IDWriteFactory) -> windows::core::Result<()> {
        let hwnd = window.hwnd;
        // SAFETY: Win32, Direct2D and DirectWrite calls on this thread's
        // window and its own device contexts and bitmap.
        unsafe {
            let dpi = GetDpiForWindow(hwnd).max(96);
            let scale = dpi as f32 / 96.0;
            let lines: Vec<(IDWriteTextLayout, f32, f32, [u8; 4])> = window
                .overlay
                .lines
                .iter()
                .filter(|line| !line.text.is_empty())
                .filter_map(|line| layout(dwrite, line, scale).map(|(layout, w, h)| (layout, w, h, line.color)))
                .collect();
            if lines.is_empty() {
                let _ = ShowWindow(hwnd, SW_HIDE);
                return Ok(());
            }
            // On the taskbar the lines fill its height, with no room around.
            let (margin, gap) = match window.overlay.place {
                Place::Taskbar => (2.0 * scale, 0.0),
                Place::Free => (MARGIN * scale, GAP * scale),
            };
            let width = (lines.iter().map(|line| line.1).fold(0.0, f32::max) + 2.0 * margin).ceil() as i32;
            let height = (lines.iter().map(|line| line.2).sum::<f32>() + gap * (lines.len() - 1) as f32 + 2.0 * margin).ceil() as i32;

            let screen = GetDC(None);
            let memory = CreateCompatibleDC(Some(screen));
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bitmap = CreateDIBSection(Some(memory), &info, DIB_RGB_COLORS, &mut bits, None, 0)?;
            let previous = SelectObject(memory, HGDIOBJ(bitmap.0));
            let target = d2d.CreateDCRenderTarget(&D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0,
                dpiY: 96.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            })?;
            target.BindDC(memory, &RECT { left: 0, top: 0, right: width, bottom: height })?;
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            target.BeginDraw();
            let plate = if window.overlay.locked { 0.0 } else { 0.3 };
            target.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: plate }));
            let shadow = target.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.75 }, None)?;
            let mut top = margin;
            for (layout, line_width, line_height, [r, g, b, a]) in &lines {
                let left = (width as f32 - line_width) / 2.0;
                let brush = target.CreateSolidColorBrush(
                    &D2D1_COLOR_F { r: *r as f32 / 255.0, g: *g as f32 / 255.0, b: *b as f32 / 255.0, a: *a as f32 / 255.0 },
                    None,
                )?;
                // A soft shadow: the text a little down and around, dark,
                // under the text itself.
                for (dx, dy) in [(1.0, 1.5), (0.0, 1.0), (1.0, 0.0), (-0.5, 1.0)] {
                    target.DrawTextLayout(
                        Vector2 { X: left + dx * scale, Y: top + dy * scale },
                        layout,
                        &shadow,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                    );
                }
                target.DrawTextLayout(Vector2 { X: left, Y: top }, layout, &brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
                top += line_height + gap;
            }
            target.EndDraw(None, None)?;

            let (left, top) = match window.overlay.place {
                Place::Taskbar => taskbar_spot(width, height).unwrap_or((0, 0)),
                Place::Free => {
                    let (x, y) = window.place.unwrap_or_else(|| {
                        let mut area = RECT::default();
                        let _ = SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut area as *mut RECT as *mut _), Default::default());
                        ((area.left + area.right) as f64 / 2.0, area.bottom as f64 - (area.bottom - area.top) as f64 * 0.1)
                    });
                    ((x - width as f64 / 2.0) as i32, (y - height as f64) as i32)
                }
            };
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            let style = if window.overlay.locked { style | WS_EX_TRANSPARENT.0 } else { style & !WS_EX_TRANSPARENT.0 };
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style as isize);
            let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
            let result = UpdateLayeredWindow(
                hwnd,
                Some(screen),
                Some(&POINT { x: left, y: top }),
                Some(&SIZE { cx: width, cy: height }),
                Some(memory),
                Some(&POINT { x: 0, y: 0 }),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            SelectObject(memory, previous);
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(memory);
            ReleaseDC(None, screen);
            result?;
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            if window.overlay.place == Place::Taskbar {
                SetTimer(Some(hwnd), TOPMOST_TIMER, 1000, None);
            }
            let _ = HSTRING::new();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_read_overlays_as_json() {
        let overlay = Overlay {
            lines: vec![Line { text: "你好 \"world\"".into(), size: 28.0, bold: true, color: [255, 128, 0, 255] }],
            place: Place::Taskbar,
            locked: true,
        };
        assert_eq!(
            as_json(&overlay),
            r##"{"lines":[{"text":"你好 \"world\"","size":28,"bold":true,"color":"#ffff8000"}],"place":"taskbar","locked":true}"##
        );
    }
}
