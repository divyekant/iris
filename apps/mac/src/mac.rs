//! The macOS part of the app: the window, the menu bar icon and the notices
//! (`docs/specs/2026-11-17-mac-app.md`, shape A).

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use mac_notification_sys::{Notification, NotificationResponse};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::webview::PageLoadEvent;
use tauri::{
    ActivationPolicy, AppHandle, LogicalPosition, Manager, RunEvent, TitleBarStyle, Url,
    WebviewUrl, WebviewWindowBuilder, WindowEvent,
};

use crate::engine;

const MAIN: &str = "main";
const TRAY: &str = "iris";
/// The bundle ID of the app (`tauri.conf.json`).
const ID: &str = "io.github.divyekant.iris.app";
/// The address of the "Start Iris" button of the local page. The app stops
/// the navigation and starts the engine: the page needs no Tauri command.
const START: &str = "start.iris.invalid";
/// The most notices that wait for a click at the same time. Each one holds
/// a small thread. More notices show with no click action (issue #590).
const WAIT_MAX: usize = 20;

const TRAY_ICON: &[u8] = include_bytes!("../icons/tray.png");
const TRAY_DOT: &[u8] = include_bytes!("../icons/tray-dot.png");

/// Runs before each page: the client adds room for the window buttons in
/// its top bar (`client/src/App.svelte`).
const INIT: &str = r#"document.documentElement.dataset.shell = "mac";"#;

struct Shell {
    home: PathBuf,
    /// The engine socket.
    socket: PathBuf,
    /// The route to open after the next page load.
    route: Mutex<Option<String>>,
    /// The origin of the engine of the last sign-in address, for example
    /// `http://127.0.0.1:4800`. Only this origin loads in the window.
    origin: Mutex<Option<String>>,
    /// The menu item with the count of notices.
    count: Mutex<Option<MenuItem<tauri::Wry>>>,
    /// The last count that the menu bar shows. `u64::MAX` before the first.
    open: AtomicU64,
}

pub fn run() {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let socket = engine::socket(&home);
    let app = tauri::Builder::default()
        .manage(Shell {
            home,
            socket,
            route: Mutex::new(None),
            origin: Mutex::new(None),
            count: Mutex::new(None),
            open: AtomicU64::new(u64::MAX),
        })
        .setup(|app| {
            let open = MenuItem::with_id(app, "open", "Open Iris", true, None::<&str>)?;
            let count = MenuItem::with_id(app, "notices", "No notices", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit Iris", true, None::<&str>)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let menu = Menu::with_items(app, &[&open, &count, &sep, &quit])?;
            *app.state::<Shell>().count.lock().unwrap() = Some(count);
            TrayIconBuilder::with_id(TRAY)
                .icon(Image::from_bytes(TRAY_ICON)?)
                .icon_as_template(true)
                .tooltip("Iris")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, e| match e.id().as_ref() {
                    "open" => show(app, None),
                    "notices" => show(app, Some("#/brief".into())),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, e| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = e
                    {
                        show(tray.app_handle(), None);
                    }
                })
                .build(app)?;
            show(app.handle(), None);
            let handle = app.handle().clone();
            std::thread::Builder::new()
                .name("notices".into())
                .spawn(move || notices(handle))?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("the Iris app could not start");
    app.run(|app, event| match event {
        // The window closed: stay in the menu bar. "Quit Iris" gives a code.
        RunEvent::ExitRequested {
            api, code: None, ..
        } => api.prevent_exit(),
        // The web view ended with the window: leave the Dock too.
        RunEvent::WindowEvent {
            label,
            event: WindowEvent::Destroyed,
            ..
        } if label == MAIN => {
            let _ = app.set_activation_policy(ActivationPolicy::Accessory);
        }
        RunEvent::Reopen { .. } => show(app, None),
        _ => {}
    });
}

/// Open the window, or bring it to the front. With a route, show that view.
fn show(app: &AppHandle, route: Option<String>) {
    if let Some(w) = app.get_webview_window(MAIN) {
        if let Some(r) = route {
            let _ = w.eval(format!("location.hash = {}", serde_json::Value::from(r)));
        }
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let shell = app.state::<Shell>();
    *shell.route.lock().unwrap() = route;
    let _ = app.set_activation_policy(ActivationPolicy::Regular);
    let handle = app.clone();
    let built = WebviewWindowBuilder::new(app, MAIN, address(&shell))
        .title("Iris")
        .inner_size(1100.0, 760.0)
        .min_inner_size(420.0, 480.0)
        .title_bar_style(TitleBarStyle::Overlay)
        .hidden_title(true)
        // y 34: the centre of the buttons meets the centre of the view text.
        .traffic_light_position(LogicalPosition::new(20.0, 34.0))
        .initialization_script(INIT)
        .on_navigation(move |url| navigate(&handle, url))
        .on_page_load(|w, p| {
            let engine = is_engine(&w.state::<Shell>(), p.url());
            if p.event() == PageLoadEvent::Finished && engine {
                let route = w.state::<Shell>().route.lock().unwrap().take();
                if let Some(r) = route {
                    let _ = w.eval(format!("location.hash = {}", serde_json::Value::from(r)));
                }
            }
        })
        .build();
    if let Ok(w) = built {
        let _ = w.set_focus();
    }
}

/// The page of the window: a new sign-in address of the engine, or the local
/// page when no engine answers.
fn address(shell: &Shell) -> WebviewUrl {
    let sign_in = engine::request(&shell.socket, "sign-in", serde_json::json!({}));
    let url = sign_in
        .ok()
        .and_then(|r| {
            r["url"]
                .as_str()
                .and_then(engine::checked_url)
                .map(str::to_owned)
        })
        .and_then(|u| Url::parse(&u).ok());
    // No origin while the window shows a local page.
    *shell.origin.lock().unwrap() = url.as_ref().map(|u| u.origin().ascii_serialization());
    match url {
        Some(u) => WebviewUrl::External(u),
        None if engine::program(&shell.home, |p| p.exists()).is_some() => {
            WebviewUrl::App("index.html".into())
        }
        None => WebviewUrl::App("missing.html".into()),
    }
}

/// True when `url` has the exact origin of the engine of the last sign-in.
fn is_engine(shell: &Shell, url: &Url) -> bool {
    shell.origin.lock().unwrap().as_deref() == Some(url.origin().ascii_serialization().as_str())
}

/// Allow the engine and the local pages in the window. "Start Iris" starts
/// the engine. Each other address, also another loopback port, opens in the
/// default browser.
fn navigate(app: &AppHandle, url: &Url) -> bool {
    if url.host_str() == Some(START) {
        // Only the button of the local page, never a link of the engine page.
        let from = app.get_webview_window(MAIN).and_then(|w| w.url().ok());
        if engine::start_allowed(from.as_ref().map_or("", |u| u.scheme()), url.as_str()) {
            start(app.clone());
        }
        return false;
    }
    if url.scheme() == "tauri" || is_engine(&app.state::<Shell>(), url) {
        return true;
    }
    if matches!(url.scheme(), "http" | "https" | "mailto") {
        let _ = std::process::Command::new("/usr/bin/open")
            .arg(url.as_str())
            .status();
    }
    false
}

/// "Start Iris": `iris service install` (safe to repeat), then load the
/// client again.
fn start(app: AppHandle) {
    std::thread::spawn(move || {
        let shell = app.state::<Shell>();
        if let Some(iris) = engine::program(&shell.home, |p| p.exists()) {
            let _ = std::process::Command::new(iris)
                .args(["service", "install"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
        // The engine needs a moment to bind its socket.
        for _ in 0..20 {
            // Only the notice thread may call `app.notices`: a call with no
            // `after` starts the app, and its `last` must reach the cursor.
            if engine::listening(&shell.socket) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        let h = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(w) = h.get_webview_window(MAIN) {
                if let WebviewUrl::External(u) = address(&h.state::<Shell>()) {
                    let _ = w.navigate(u);
                }
            }
        });
    });
}

/// The notice thread: ask the engine each [`engine::POLL`], show each new
/// notice, and keep the menu bar dot current.
fn notices(app: AppHandle) {
    // A failed `set_application` leaves the hook too: ask LaunchServices
    // first, and use `osascript` when it does not know the app.
    let native = known_app(ID) && mac_notification_sys::set_application(ID).is_ok();
    let socket = app.state::<Shell>().socket.clone();
    let waiting = Arc::new(AtomicUsize::new(0));
    let mut poll = engine::Poll::default();
    loop {
        let started = std::time::Instant::now();
        let page = engine::request(&socket, "app.notices", poll.args())
            .ok()
            .and_then(|r| poll.read(&r));
        if let Some(page) = page {
            // A new start of the engine, or an engine that answers while the
            // window shows "Iris is not running": sign the window in.
            let signed_in = app.state::<Shell>().origin.lock().unwrap().is_some();
            if engine::sign_in_needed(page.restarted, signed_in) {
                sign_in_again(&app);
            }
            menu_bar(&app, page.open);
            for n in page.notices {
                notify(&app, n, native, &waiting);
            }
            if page.more {
                continue;
            }
        }
        std::thread::sleep(engine::next_poll(started.elapsed()));
    }
}

/// The engine started again with a new web secret: load a new sign-in
/// address in the open window (issue #547). The window keeps its view: the
/// page load handler applies the route of the old page, also one that a
/// notice click set just before (issue #603).
fn sign_in_again(app: &AppHandle) {
    let h = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(w) = h.get_webview_window(MAIN) {
            let shell = h.state::<Shell>();
            let view = w.url().ok().and_then(|u| engine::view_route(u.as_str()));
            if let WebviewUrl::External(u) = address(&shell) {
                if let Some(r) = view {
                    shell.route.lock().unwrap().get_or_insert(r);
                }
                let _ = w.navigate(u);
            }
        }
    });
}

/// The dot on the menu bar icon and the count in the menu.
fn menu_bar(app: &AppHandle, open: u64) {
    let shell = app.state::<Shell>();
    if shell.open.swap(open, Ordering::Relaxed) == open {
        return;
    }
    if let Some(tray) = app.tray_by_id(TRAY) {
        let icon = if open == 0 { TRAY_ICON } else { TRAY_DOT };
        let _ = Image::from_bytes(icon).map(|i| tray.set_icon(Some(i)));
        let _ = tray.set_icon_as_template(true);
    }
    let text = match open {
        0 => "No notices".to_string(),
        1 => "1 notice".to_string(),
        n => format!("{n} notices"),
    };
    // A named guard: a temporary in the tail would outlive `shell`.
    let count = shell.count.lock().unwrap();
    if let Some(item) = count.as_ref() {
        let _ = item.set_text(text);
    }
}

/// True when LaunchServices finds an app with the bundle ID `id`. Only then
/// may `mac-notification-sys` run: on a miss, its `set_application` leaves a
/// hook that gives the whole process the ID of Terminal. A miss occurs for an
/// app in a temporary folder, for example an app that macOS translocated.
fn known_app(id: &str) -> bool {
    use std::ffi::c_void;
    type CfRef = *const c_void;
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringCreateWithBytes(
            alloc: CfRef,
            bytes: *const u8,
            len: isize,
            encoding: u32,
            external: u8,
        ) -> CfRef;
        fn CFRelease(cf: CfRef);
    }
    #[link(name = "CoreServices", kind = "framework")]
    extern "C" {
        fn LSCopyApplicationURLsForBundleIdentifier(id: CfRef, error: *mut CfRef) -> CfRef;
    }
    const UTF8: u32 = 0x0800_0100;
    // SAFETY: each object that a Create or Copy call gives is released once.
    unsafe {
        let s = CFStringCreateWithBytes(std::ptr::null(), id.as_ptr(), id.len() as isize, UTF8, 0);
        if s.is_null() {
            return false;
        }
        let urls = LSCopyApplicationURLsForBundleIdentifier(s, std::ptr::null_mut());
        CFRelease(s);
        if urls.is_null() {
            return false;
        }
        CFRelease(urls);
        true
    }
}

/// Show one notice. With `native`, through the notice API, and a click
/// opens its route in the window. Else, or if the notice API fails, the
/// fixed `osascript` of the engine shows it, with no click action.
fn notify(app: &AppHandle, n: engine::Notice, native: bool, waiting: &Arc<AtomicUsize>) {
    if !native {
        let _ = engine::run(engine::osascript(&n.title, &n.body), engine::SHOW_WAIT);
        return;
    }
    let wait = waiting.fetch_add(1, Ordering::Relaxed) < WAIT_MAX;
    let waiting = waiting.clone();
    let app = app.clone();
    std::thread::spawn(move || {
        let sent = Notification::new()
            .title(&n.title)
            .message(&n.body)
            .wait_for_click(wait)
            .send();
        match sent {
            Ok(NotificationResponse::Click) => {
                let h = app.clone();
                let _ = app.run_on_main_thread(move || show(&h, Some(n.route)));
            }
            Ok(_) => {}
            Err(_) => {
                let _ = engine::run(engine::osascript(&n.title, &n.body), engine::SHOW_WAIT);
            }
        }
        waiting.fetch_sub(1, Ordering::Relaxed);
    });
}
