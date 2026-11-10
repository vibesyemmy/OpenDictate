//! The two things the window needs that egui has no builder for: a blurred
//! backdrop, and a Dock tile that does not depend on Launch Services having
//! noticed the bundle's icon.

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, Sel};
use objc2::{sel, AnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSAutoresizingMaskOptions, NSBox, NSBoxType, NSColor, NSImage, NSTitlePosition,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
    NSWindowOrderingMode,
};
use objc2_foundation::NSData;
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};

thread_local! {
    /// The wash of paper over the blur. Kept so a theme change can retint it
    /// without rebuilding the backdrop.
    static TINT: RefCell<Option<Retained<NSBox>>> = const { RefCell::new(None) };
    /// The blur itself, kept so its presence can be checked rather than
    /// assumed. Attaching it to a reopened window reported success and then
    /// came to nothing: AppKit rebuilds the frame view's subviews while the
    /// window is being set up, and anything added too early goes with them.
    static FX: RefCell<Option<Retained<NSVisualEffectView>>> = const { RefCell::new(None) };
}

/// Keeps the backdrop the size of the window it is in.
///
/// It is built from the frame view's bounds, and on a window that has just
/// been reopened those are not settled yet - a zero-sized view attaches
/// perfectly well and draws nothing, which is exactly what a reopened
/// window looked like: the attach logged success and the desktop showed
/// straight through the page. An autoresizing mask cannot grow a view from
/// nothing either, so the size is set here instead of trusted.
pub fn fit_backdrop() {
    if MainThreadMarker::new().is_none() {
        return;
    }
    FX.with(|f| {
        if let Some(fx) = f.borrow().as_ref() {
            unsafe {
                if let Some(parent) = fx.superview() {
                    let want = parent.bounds();
                    let have = fx.frame();
                    if (have.size.width - want.size.width).abs() > 0.5
                        || (have.size.height - want.size.height).abs() > 0.5
                    {
                        fx.setFrame(want);
                        TINT.with(|t| {
                            if let Some(wash) = t.borrow().as_ref() {
                                wash.setFrame(fx.bounds());
                            }
                        });
                    }
                }
            }
        }
    });
}

/// Whether the backdrop is still in the window it is supposed to be in.
///
/// Checks the view, not a flag and not a window number: the only thing that
/// matters is whether it is in a hierarchy, and whether that hierarchy is
/// the window on screen now.
pub fn backdrop_ok(min_width: f64) -> bool {
    let Some(current) = main_window_number(min_width) else { return false };
    FX.with(|f| {
        f.borrow().as_ref().is_some_and(|fx| unsafe {
            fx.superview().is_some() && fx.window().is_some_and(|w| w.windowNumber() as isize == current)
        })
    })
}

/// Sets the Dock tile from the icon we ship.
///
/// The bundle already names it in CFBundleIconFile, but Launch Services keys
/// its icon cache to the bundle path, so replacing an app in place can leave
/// the Dock showing whatever was there first - the generic tile, for an app
/// that once had no icon. Setting it on the running NSApplication skips that
/// cache entirely.
pub fn set_dock_icon(png: &[u8]) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let data = NSData::with_bytes(png);
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else { return };
    unsafe { NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image)) };
}

fn ns_color(rgba: [f64; 4]) -> Retained<NSColor> {
    unsafe { NSColor::colorWithSRGBRed_green_blue_alpha(rgba[0], rgba[1], rgba[2], rgba[3]) }
}

/// Puts a blurred backdrop behind the whole window, titlebar included.
///
/// Transparency alone shows what is behind the window sharply, which is
/// distracting under text. This is the system's own blur, the same one Finder
/// and Notes sit on.
///
/// It goes into the window's frame view rather than its content view, for two
/// reasons. The content view is the Metal view egui draws into - not a
/// container holding one - so anything added to it covers the drawing instead
/// of sitting under it. And the content view stops below the titlebar, which
/// is why the bar had nothing behind it at all once the window went
/// transparent.
///
/// Whether the app is the one in front.
///
/// macOS expects clicking an app's Dock icon to bring its window back, which
/// is normally the delegate's applicationShouldHandleReopen. eframe owns the
/// delegate, so this is read instead: the app going from background to front
/// with no window open means the same thing.
pub fn app_is_active() -> bool {
    MainThreadMarker::new().is_some_and(|mtm| NSApplication::sharedApplication(mtm).isActive())
}

/// Set by AppKit when the Dock icon is clicked.
static REOPEN: AtomicBool = AtomicBool::new(false);

extern "C-unwind" fn handle_reopen(_this: &AnyObject, _cmd: Sel, _sender: *mut AnyObject, _visible: Bool) -> Bool {
    REOPEN.store(true, Ordering::Relaxed);
    Bool::YES
}

/// Teaches eframe's application delegate to answer the Dock.
///
/// Clicking a running app's icon makes macOS send
/// applicationShouldHandleReopen: to the delegate, and that is the only
/// honest signal that the user wants the window back. eframe owns the
/// delegate and does not answer it, so the method is added to its class at
/// runtime. Everything read from the outside instead - whether the app was
/// active, whether it had just been deactivated - missed the ordinary case
/// where the app is already in front with nothing on screen.
///
/// Returns false if the delegate already answers it, in which case nothing
/// is touched and the caller is no worse off than before.
pub fn install_reopen_hook() -> bool {
    let Some(mtm) = MainThreadMarker::new() else { return false };
    let app = NSApplication::sharedApplication(mtm);
    let Some(delegate) = app.delegate() else { return false };
    let object: &AnyObject = unsafe { &*Retained::as_ptr(&delegate).cast::<AnyObject>() };
    let class = object.class();
    let selector = sel!(applicationShouldHandleReopen:hasVisibleWindows:);
    if class.instance_method(selector).is_some() {
        return false;
    }
    // "B@:@B": returns BOOL, takes self, _cmd, the application, and whether
    // it has visible windows.
    unsafe {
        objc2::ffi::class_addMethod(
            (class as *const AnyClass).cast_mut(),
            selector,
            std::mem::transmute::<
                extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject, Bool) -> Bool,
                unsafe extern "C-unwind" fn(),
            >(handle_reopen),
            c"B@:@B".as_ptr(),
        )
        .as_bool()
    }
}

/// Whether the Dock has been clicked since this was last asked.
pub fn take_reopen() -> bool {
    REOPEN.swap(false, Ordering::Relaxed)
}

/// Brings the app to the front.
///
/// Showing a hidden window does not activate the app, and a status-menu
/// click does not either, so "Show Open Dictate" could put the window back
/// behind whatever the user was looking at - indistinguishable, from the
/// outside, from nothing happening.
pub fn activate() {
    if let Some(mtm) = MainThreadMarker::new() {
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    }
}

/// Steps the app out of the foreground.
///
/// Called when the window hides, so that the next Dock click is an
/// activation. Without it the app usually stays frontmost with nothing on
/// screen, and clicking its Dock icon changes nothing to notice - macOS
/// would normally handle that through the delegate's
/// applicationShouldHandleReopen, which eframe owns.
pub fn resign_active() {
    if let Some(mtm) = MainThreadMarker::new() {
        NSApplication::sharedApplication(mtm).deactivate();
    }
}

/// Logs what the backdrop is doing, beside the settings. Attaching happens
/// on a window the user can close and reopen at will, and a fault there is
/// invisible from the outside.
pub fn log_backdrop(line: &str) {
    use std::io::Write;
    let path = crate::settings::data_dir().join("backdrop.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}

/// The number of the window a meeting-sized window currently has, or None
/// when there isn't one. Closing and reopening gives a different number,
/// which is how the caller knows its backdrop went with the old window.
pub fn main_window_number(min_width: f64) -> Option<isize> {
    let mtm = MainThreadMarker::new()?;
    let app = NSApplication::sharedApplication(mtm);
    app.windows()
        .iter()
        .find(|w| w.frame().size.width >= min_width)
        .map(|w| w.windowNumber() as isize)
}

/// Returns the window it attached to, or None while there isn't one.
pub fn blur_window(min_width: f64, tint: [f64; 4]) -> Option<isize> {
    let mtm = MainThreadMarker::new()?;
    let app = NSApplication::sharedApplication(mtm);
    for window in app.windows().iter() {
        // The root viewport is a 1x1 anchor with the same title, so the real
        // window is found by being the only one of any size.
        if window.frame().size.width < min_width {
            continue;
        }
        let Some(content) = window.contentView() else { continue };
        let Some(frame_view) = (unsafe { content.superview() }) else { continue };
        window.setOpaque(false);
        window.setBackgroundColor(Some(&NSColor::clearColor()));
        // Otherwise the titlebar paints its own material over the blur and
        // the top of the window is a different surface from the rest.
        window.setTitlebarAppearsTransparent(true);

        let bounds = frame_view.bounds();
        let fill =
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable;

        let fx = NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), bounds);
        // UnderWindowBackground is the material meant for exactly this: a
        // whole window sitting on the desktop, not a sidebar or a popover.
        // Measured against the same pixels with and without the window:
        // HUDWindow passes about 40% more of the backdrop than
        // UnderWindowBackground, which is the most of any material here.
        fx.setMaterial(NSVisualEffectMaterial::HUDWindow);
        fx.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
        // Without Active the blur stops when the window loses focus.
        fx.setState(NSVisualEffectState::Active);
        fx.setAutoresizingMask(fill);

        // The paper wash, over the blur and under everything egui draws, so
        // the titlebar and the page are one surface. egui leaves its own
        // background clear once this exists.
        let wash = NSBox::initWithFrame(NSBox::alloc(mtm), bounds);
        wash.setBoxType(NSBoxType::Custom);
        wash.setBorderWidth(0.0);
        wash.setTitlePosition(NSTitlePosition::NoTitle);
        wash.setFillColor(&ns_color(tint));
        wash.setAutoresizingMask(fill);
        fx.addSubview(&wash);

        frame_view.addSubview_positioned_relativeTo(&fx, NSWindowOrderingMode::Below, None);
        TINT.with(|t| *t.borrow_mut() = Some(wash));
        FX.with(|f| *f.borrow_mut() = Some(fx));
        return Some(window.windowNumber() as isize);
    }
    None
}

/// Repaints the wash after a theme change.
pub fn set_backdrop_tint(tint: [f64; 4]) {
    if MainThreadMarker::new().is_none() {
        return;
    }
    TINT.with(|t| {
        if let Some(wash) = t.borrow().as_ref() {
            wash.setFillColor(&ns_color(tint));
        }
    });
}
