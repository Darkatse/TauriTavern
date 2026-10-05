//! UIKit owns safe-area and keyboard geometry; WKWebView is an ordinary content view.
use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadOnly, Message, define_class, msg_send};
use objc2_foundation::{NSArray, NSData, NSObjectProtocol, NSPoint, NSRect, NSSize};
use objc2_ui_kit::{NSLayoutConstraint, UIColor, UIImage, UIImageView, UIView, UIViewAutoresizing};
use tauri::{AppHandle, WebviewWindow};
use tt_adapter_media::window_backdrop::BackdropStrip;
use tt_contracts::window_layout::{WindowBackdropRequest, WindowInsets, WindowSnapshot};

use super::window_layout::WallpaperRenderTarget;
use tt_domain::errors::DomainError;

struct WindowHostIvars {
    window: WebviewWindow,
    webview: Retained<UIView>,
    content_constraints: RefCell<Vec<Retained<NSLayoutConstraint>>>,
    snapshot: RefCell<Option<WindowSnapshot>>,
    wallpaper_token: Cell<u64>,
    strips: RefCell<Vec<Retained<UIImageView>>>,
}

define_class!(
    #[unsafe(super(UIView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = WindowHostIvars]
    struct WindowHost;

    unsafe impl NSObjectProtocol for WindowHost {}

    impl WindowHost {
        #[unsafe(method(didAddSubview:))]
        fn did_add_subview(&self, subview: &UIView) {
            unsafe { let _: () = msg_send![super(self), didAddSubview: subview]; }
            if std::ptr::eq(subview, &*self.ivars().webview) {
                // Element fullscreen reparents WKWebView, which deactivates its constraints.
                // Initial installation and fullscreen return both activate this same content policy.
                NSLayoutConstraint::activateConstraints(
                    &NSArray::from_retained_slice(&self.ivars().content_constraints.borrow()),
                    self.mtm(),
                );
                self.setNeedsLayout();
            }
        }

        #[unsafe(method(layoutSubviews))]
        fn layout_subviews(&self) {
            unsafe { let _: () = msg_send![super(self), layoutSubviews]; }
            self.publish_window();
        }
    }
);

thread_local! {
    static MAIN_HOST: RefCell<Option<Retained<WindowHost>>> = const { RefCell::new(None) };
}

impl WindowHost {
    fn clear_strips(&self) {
        for strip in self.ivars().strips.borrow_mut().drain(..) {
            strip.removeFromSuperview();
        }
    }

    fn publish_window(&self) {
        let Some(window) = self.window() else { return };
        let scale = window.screen().scale();
        let bounds = self.bounds();
        let safe = self.safeAreaInsets();
        let pixels = |points: f64| (points * scale).round() as u32;
        let mut next = WindowSnapshot {
            revision: 0,
            width: pixels(bounds.size.width),
            height: pixels(bounds.size.height),
            scale,
            insets: WindowInsets {
                top: pixels(safe.top),
                right: pixels(safe.right),
                bottom: pixels(safe.bottom),
                left: pixels(safe.left),
            },
        };
        if next.width == 0 || next.height == 0 {
            return;
        }
        let mut current = self.ivars().snapshot.borrow_mut();
        if let Some(current) = current.as_ref() {
            next.revision = current.revision;
            if current == &next {
                return;
            }
        }
        next.revision += 1;
        *current = Some(next.clone());
        drop(current);
        self.clear_strips();
        let json = serde_json::to_string(&next).expect("finite window geometry");
        if let Err(error) = self.ivars().window.eval(format!(
            "window.dispatchEvent(new CustomEvent('tt-window-changed',{{detail:{json}}}));"
        )) {
            tracing::warn!("Publish iOS window snapshot: {error}");
        }
    }
}

/// Called once after Wry has installed its WKWebView, on the UIKit main thread.
pub unsafe fn install(webview: &UIView, window: WebviewWindow) {
    let parent = webview.superview().expect("WKWebView must have a parent");
    let allocated = WindowHost::alloc(webview.mtm()).set_ivars(WindowHostIvars {
        window,
        webview: webview.retain(),
        content_constraints: RefCell::new(Vec::new()),
        snapshot: RefCell::new(None),
        wallpaper_token: Cell::new(0),
        strips: RefCell::new(Vec::new()),
    });
    let host: Retained<WindowHost> =
        unsafe { msg_send![super(allocated), initWithFrame: parent.bounds()] };
    host.setAutoresizingMask(
        UIViewAutoresizing::FlexibleWidth | UIViewAutoresizing::FlexibleHeight,
    );
    parent.addSubview(&host);
    webview.setAutoresizingMask(UIViewAutoresizing::empty());
    webview.setTranslatesAutoresizingMaskIntoConstraints(false);
    let safe = host.safeAreaLayoutGuide();
    // The guide rests at the bottom safe area when a docked keyboard is absent.
    // Floating/undocked keyboards do not reserve the entire viewport.
    let keyboard = host.keyboardLayoutGuide();
    *host.ivars().content_constraints.borrow_mut() = vec![
        webview
            .topAnchor()
            .constraintEqualToAnchor(&safe.topAnchor()),
        webview
            .leadingAnchor()
            .constraintEqualToAnchor(&safe.leadingAnchor()),
        webview
            .trailingAnchor()
            .constraintEqualToAnchor(&safe.trailingAnchor()),
        webview
            .bottomAnchor()
            .constraintEqualToAnchor(&keyboard.topAnchor()),
    ];
    host.addSubview(webview);
    MAIN_HOST.with(|slot| *slot.borrow_mut() = Some(host.clone()));
    host.setNeedsLayout();
}

async fn with_host<T: Send + 'static>(
    app: &AppHandle,
    action: impl FnOnce(&WindowHost) -> Result<T, DomainError> + Send + 'static,
) -> Result<T, DomainError> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let result = MAIN_HOST.with(|slot| {
            let slot = slot.borrow();
            let host = slot
                .as_ref()
                .ok_or_else(|| failure("host is not installed"))?;
            action(host)
        });
        let _ = sender.send(result);
    })
    .map_err(failure)?;
    receiver.await.map_err(failure)?
}

fn failure(error: impl std::fmt::Display) -> DomainError {
    DomainError::InternalError(format!("iOS window layout: {error}"))
}

pub async fn snapshot(app: &AppHandle) -> Result<WindowSnapshot, DomainError> {
    with_host(app, |host| {
        host.ivars()
            .snapshot
            .borrow()
            .clone()
            .ok_or_else(|| failure("window is not laid out"))
    })
    .await
}

pub(super) async fn begin_backdrop(
    app: &AppHandle,
    request: &WindowBackdropRequest,
) -> Result<Option<WallpaperRenderTarget>, DomainError> {
    let window_revision = request.window_revision;
    let color = request.color;
    let replace_wallpaper = request.wallpaper.is_some();
    with_host(app, move |host| {
        let window = host.ivars().snapshot.borrow().clone();
        let Some(window) = window.filter(|window| window.revision == window_revision) else {
            return Ok(None);
        };
        host.setBackgroundColor(Some(&UIColor::colorWithRed_green_blue_alpha(
            color[0] as f64 / 255.0,
            color[1] as f64 / 255.0,
            color[2] as f64 / 255.0,
            1.0,
        )));
        if replace_wallpaper {
            host.ivars()
                .wallpaper_token
                .set(host.ivars().wallpaper_token.get() + 1);
            host.clear_strips();
        }
        Ok(Some(WallpaperRenderTarget {
            window,
            token: host.ivars().wallpaper_token.get(),
        }))
    })
    .await
}

pub(super) async fn apply_backdrop(
    app: &AppHandle,
    target: WallpaperRenderTarget,
    strips: Vec<BackdropStrip>,
) -> Result<(), DomainError> {
    with_host(app, move |host| {
        let current = host.ivars().snapshot.borrow();
        let Some(snapshot) = current.as_ref() else {
            return Ok(());
        };
        if snapshot.revision != target.window.revision
            || host.ivars().wallpaper_token.get() != target.token
        {
            // Superseded window/wallpaper renders are expected and never reach the view tree.
            return Ok(());
        }
        let scale = snapshot.scale;
        drop(current);
        let mut views = Vec::new();
        for strip in strips {
            let image = UIImage::imageWithData_scale(&NSData::with_bytes(&strip.png), scale)
                .ok_or_else(|| failure("invalid backdrop PNG"))?;
            let view = UIImageView::initWithImage(UIImageView::alloc(host.mtm()), Some(&image));
            view.setFrame(NSRect::new(
                NSPoint::new(strip.x as f64 / scale, strip.y as f64 / scale),
                NSSize::new(strip.width as f64 / scale, strip.height as f64 / scale),
            ));
            views.push(view);
        }
        // Decode every strip before replacing the visible set; failures leave no partial views.
        host.clear_strips();
        for view in &views {
            host.insertSubview_atIndex(view, 0);
        }
        *host.ivars().strips.borrow_mut() = views;
        Ok(())
    })
    .await
}
