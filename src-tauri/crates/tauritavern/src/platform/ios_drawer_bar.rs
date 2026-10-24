//! The page's top-level drawers as native buttons in a system bar region (iOS 27.1+).
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{ClassType, MainThreadMarker, MainThreadOnly, class, msg_send, sel};
use objc2_foundation::{NSArray, NSInteger, NSString};
use objc2_ui_kit::{
    NSDirectionalRectEdge, NSLayoutConstraint, NSObjectUIAccessibility, UIAction, UIButton,
    UIButtonConfiguration, UIButtonConfigurationCornerStyle, UIColor, UICornerConfiguration,
    UIGlassEffect, UIGlassEffectStyle, UIImage, UIImageSymbolConfiguration, UIImageSymbolWeight,
    UIImageView, UILargeContentViewerInteraction, UILayoutConstraintAxis,
    UILayoutPriorityDefaultHigh, UIStackView, UIStackViewDistribution, UIStatusBarStyle,
    UITraitCollection, UITraitEnvironment, UITraitEnvironmentLayoutDirection,
    UIUserInterfaceSizeClass, UIUserInterfaceStyle, UIView, UIViewLayoutRegion, UIVisualEffectView,
};
use tauri::WebviewWindow;
use tt_contracts::window_layout::{DrawerBarEdge, DrawerBarItem, DrawerBarMark};
use tt_domain::errors::DomainError;

const BAR_EXTENT: f64 = 56.0;
const BUTTON_LENGTH: f64 = 44.0;
const CAPSULE_PADDING: f64 = 6.0;
const SYMBOL_POINT_SIZE: f64 = 20.0;
const DOT_POINT_SIZE: f64 = 7.0;
/// Puts the dot on the glyph's top trailing corner, where the page draws its badge.
const DOT_OFFSET: f64 = 11.0;

/// A bar region the system lays out beside its status bar.
#[derive(Clone, Copy)]
pub(super) struct SystemBar {
    pub directional_edge: NSDirectionalRectEdge,
    pub edge: DrawerBarEdge,
}

impl SystemBar {
    /// The system's vertical bar, which also holds the status bar.
    pub fn vertical(traits: &UITraitCollection) -> Option<Self> {
        if !traits.respondsToSelector(sel!(verticalBarEdge)) {
            return None;
        }
        let raw: NSInteger = unsafe { msg_send![traits, verticalBarEdge] };
        let directional_edge = match raw {
            0 => return None,
            1 => NSDirectionalRectEdge::Leading,
            2 => NSDirectionalRectEdge::Trailing,
            other => {
                tracing::error!("Unknown UIVerticalBarEdge {other}; the page keeps its top bar");
                return None;
            }
        };
        let right_to_left =
            unsafe { traits.layoutDirection() } == UITraitEnvironmentLayoutDirection::RightToLeft;
        let edge = if (directional_edge == NSDirectionalRectEdge::Trailing) != right_to_left {
            DrawerBarEdge::Right
        } else {
            DrawerBarEdge::Left
        };
        Some(Self {
            directional_edge,
            edge,
        })
    }

    /// Where the host draws the drawers; compact height keeps the page's own top bar.
    pub fn hosting_drawers(host: &UIView) -> Option<Self> {
        let traits = host.traitCollection();
        // Bar regions arrived together with the vertical bar.
        if !traits.respondsToSelector(sel!(verticalBarEdge))
            || unsafe { traits.verticalSizeClass() } != UIUserInterfaceSizeClass::Regular
        {
            return None;
        }
        Self::vertical(&traits).or_else(|| {
            let top = Self {
                directional_edge: NSDirectionalRectEdge::Top,
                edge: DrawerBarEdge::Top,
            };
            // Some layouts set the top bar beside the status bar, inside the top safe-area inset.
            let region = host.edgeInsetsForLayoutRegion(&top.region());
            let region_bottom = host.bounds().size.height - region.bottom;
            (region_bottom <= host.safeAreaInsets().top).then_some(top)
        })
    }

    fn region(&self) -> Retained<UIViewLayoutRegion> {
        unsafe {
            msg_send![UIViewLayoutRegion::class(), layoutRegionForBarOnDirectionalEdge: self.directional_edge, extent: BAR_EXTENT]
        }
    }

    fn axis(&self) -> UILayoutConstraintAxis {
        match self.edge {
            DrawerBarEdge::Top => UILayoutConstraintAxis::Horizontal,
            DrawerBarEdge::Left | DrawerBarEdge::Right => UILayoutConstraintAxis::Vertical,
        }
    }
}

pub(super) struct BarItem {
    item: DrawerBarItem,
    image: Retained<UIImage>,
    /// The page's badge dot, shown for working and unread marks.
    dot: Option<Retained<UIImage>>,
}

pub(super) fn resolve_items(items: Vec<DrawerBarItem>) -> Result<Vec<BarItem>, DomainError> {
    items
        .into_iter()
        .map(|item| {
            let image = system_image(symbol_name(&item.id))?;
            let dot = match item.mark {
                Some(DrawerBarMark::Working | DrawerBarMark::Unread) => {
                    Some(system_image("circle.fill")?)
                }
                Some(DrawerBarMark::Alert) | None => None,
            };
            Ok(BarItem { item, image, dot })
        })
        .collect()
}

fn system_image(name: &str) -> Result<Retained<UIImage>, DomainError> {
    UIImage::systemImageNamed(&NSString::from_str(name))
        .ok_or_else(|| DomainError::InternalError(format!("SF Symbol {name} is unavailable")))
}

fn symbol_name(drawer_id: &str) -> &'static str {
    match drawer_id {
        "ai-config-button" => "slider.horizontal.3",
        "sys-settings-button" => "powerplug",
        "advanced-formatting-button" => "textformat",
        "WI-SP-button" => "book.closed",
        "user-settings-button" => "person.badge.gearshape",
        "backgrounds-button" => "photo",
        "extensions-settings-button" => "puzzlepiece.extension",
        "persona-management-button" => "face.smiling",
        "rightNavHolder" => "person.text.rectangle",
        // Drawers added by extensions have no native symbol.
        _ => "square.grid.2x2",
    }
}

pub(super) struct NativeDrawerBar {
    bar: SystemBar,
    capsule: Retained<UIVisualEffectView>,
    stack: Retained<UIStackView>,
    buttons: Vec<DrawerButton>,
    shown: bool,
    window: WebviewWindow,
}

impl NativeDrawerBar {
    pub fn install(host: &UIView, bar: SystemBar, window: WebviewWindow) -> Self {
        let mtm = host.mtm();
        let guide = host.layoutGuideForLayoutRegion(&bar.region());
        let glass = UIGlassEffect::effectWithStyle(UIGlassEffectStyle::Regular, mtm);
        let capsule =
            UIVisualEffectView::initWithEffect(UIVisualEffectView::alloc(mtm), Some(&glass));
        capsule.setCornerConfiguration(&UICornerConfiguration::capsuleConfiguration());
        capsule.setTranslatesAutoresizingMaskIntoConstraints(false);
        let stack = UIStackView::initWithArrangedSubviews(UIStackView::alloc(mtm), &NSArray::new());
        stack.setAxis(bar.axis());
        stack.setDistribution(UIStackViewDistribution::FillEqually);
        stack.setTranslatesAutoresizingMaskIntoConstraints(false);
        let content = capsule.contentView();
        content.addSubview(&stack);
        let large_content_viewer = UILargeContentViewerInteraction::new(mtm);
        capsule.addInteraction(ProtocolObject::from_ref(&*large_content_viewer));
        host.addSubview(&capsule);
        // The region clears the corners, status bar and camera, and centers on the bar's axis.
        let placement = if bar.axis() == UILayoutConstraintAxis::Horizontal {
            [
                capsule
                    .leadingAnchor()
                    .constraintEqualToAnchor(&guide.leadingAnchor()),
                capsule
                    .centerYAnchor()
                    .constraintEqualToAnchor(&guide.centerYAnchor()),
                capsule
                    .heightAnchor()
                    .constraintEqualToAnchor(&guide.heightAnchor()),
                capsule
                    .trailingAnchor()
                    .constraintLessThanOrEqualToAnchor(&guide.trailingAnchor()),
            ]
        } else {
            [
                capsule
                    .topAnchor()
                    .constraintEqualToAnchor(&guide.topAnchor()),
                capsule
                    .centerXAnchor()
                    .constraintEqualToAnchor(&guide.centerXAnchor()),
                capsule
                    .widthAnchor()
                    .constraintEqualToAnchor(&guide.widthAnchor()),
                capsule
                    .bottomAnchor()
                    .constraintLessThanOrEqualToAnchor(&guide.bottomAnchor()),
            ]
        };
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&placement), mtm);
        NSLayoutConstraint::activateConstraints(
            &NSArray::from_retained_slice(&[
                stack
                    .topAnchor()
                    .constraintEqualToAnchor_constant(&content.topAnchor(), CAPSULE_PADDING),
                stack
                    .bottomAnchor()
                    .constraintEqualToAnchor_constant(&content.bottomAnchor(), -CAPSULE_PADDING),
                stack
                    .leadingAnchor()
                    .constraintEqualToAnchor_constant(&content.leadingAnchor(), CAPSULE_PADDING),
                stack
                    .trailingAnchor()
                    .constraintEqualToAnchor_constant(&content.trailingAnchor(), -CAPSULE_PADDING),
            ]),
            mtm,
        );
        let drawer_bar = Self {
            bar,
            capsule,
            stack,
            buttons: Vec::new(),
            shown: true,
            window,
        };
        drawer_bar.update_visibility();
        drawer_bar
    }

    pub fn directional_edge(&self) -> NSDirectionalRectEdge {
        self.bar.directional_edge
    }

    pub fn set_shown(&mut self, shown: bool) {
        self.shown = shown;
        self.update_visibility();
    }

    pub fn set_items(&mut self, items: &[BarItem]) {
        let mtm = self.capsule.mtm();
        let same_drawers = self
            .buttons
            .iter()
            .map(|drawer| drawer.id.as_str())
            .eq(items.iter().map(|bar_item| bar_item.item.id.as_str()));
        if !same_drawers {
            for drawer in self.buttons.drain(..) {
                drawer.button.removeFromSuperview();
            }
            for bar_item in items {
                let drawer =
                    DrawerButton::new(&bar_item.item.id, &self.window, self.bar.axis(), mtm);
                self.stack.addArrangedSubview(&drawer.button);
                self.buttons.push(drawer);
            }
        }
        for (drawer, bar_item) in self.buttons.iter_mut().zip(items) {
            drawer.present(bar_item, mtm);
        }
        self.update_visibility();
    }

    /// Follows the status bar so the glyphs stay legible over the same backdrop.
    pub fn set_status_bar_style(&self, style: UIStatusBarStyle) {
        let interface_style = match style {
            UIStatusBarStyle::DarkContent => UIUserInterfaceStyle::Light,
            UIStatusBarStyle::LightContent => UIUserInterfaceStyle::Dark,
            _ => UIUserInterfaceStyle::Unspecified,
        };
        self.capsule.setOverrideUserInterfaceStyle(interface_style);
    }

    pub fn remove(self) {
        self.capsule.removeFromSuperview();
    }

    fn update_visibility(&self) {
        self.capsule
            .setHidden(!self.shown || self.buttons.is_empty());
    }
}

struct DrawerButton {
    id: String,
    button: Retained<UIButton>,
    mark: Option<DrawerBarMark>,
    dot: Option<Retained<UIImageView>>,
}

impl DrawerButton {
    fn new(
        id: &str,
        window: &WebviewWindow,
        axis: UILayoutConstraintAxis,
        mtm: MainThreadMarker,
    ) -> Self {
        let handler: RcBlock<dyn Fn(NonNull<UIAction>)> = {
            let window = window.clone();
            let id = id.to_owned();
            RcBlock::new(move |_action: NonNull<UIAction>| activate(&window, &id))
        };
        let action = unsafe { UIAction::actionWithHandler(RcBlock::as_ptr(&handler), mtm) };
        let button = UIButton::buttonWithConfiguration_primaryAction(
            &UIButtonConfiguration::plainButtonConfiguration(mtm),
            Some(&action),
        );
        button.setShowsLargeContentViewer(true);
        // Equal buttons shrink together when the region is shorter than the drawers need.
        let length = if axis == UILayoutConstraintAxis::Horizontal {
            button.widthAnchor()
        } else {
            button.heightAnchor()
        }
        .constraintEqualToConstant(BUTTON_LENGTH);
        length.setPriority(UILayoutPriorityDefaultHigh);
        length.setActive(true);
        Self {
            id: id.to_owned(),
            button,
            mark: None,
            dot: None,
        }
    }

    fn present(&mut self, bar_item: &BarItem, mtm: MainThreadMarker) {
        let title = NSString::from_str(&bar_item.item.title);
        self.button
            .setConfiguration(Some(&configuration(bar_item, mtm)));
        self.button.setSelected(bar_item.item.open);
        self.button.setAccessibilityLabel(Some(&title), mtm);
        self.button.setLargeContentTitle(Some(&title));
        self.button.setLargeContentImage(Some(&bar_item.image));
        if self.mark == bar_item.item.mark {
            return;
        }
        self.mark = bar_item.item.mark;
        if let Some(dot) = self.dot.take() {
            dot.removeFromSuperview();
        }
        self.dot = bar_item.dot.as_ref().map(|image| {
            let dot = add_dot(&self.button, image, mtm);
            if self.mark == Some(DrawerBarMark::Working) {
                // The page's badge breathes while work runs.
                let effect: Retained<AnyObject> =
                    unsafe { msg_send![class!(NSSymbolBreatheEffect), effect] };
                let _: () = unsafe { msg_send![&*dot, addSymbolEffect: &*effect] };
            }
            dot
        });
    }
}

fn add_dot(button: &UIButton, image: &UIImage, mtm: MainThreadMarker) -> Retained<UIImageView> {
    let dot = UIImageView::initWithImage(UIImageView::alloc(mtm), Some(image));
    dot.setPreferredSymbolConfiguration(Some(
        &UIImageSymbolConfiguration::configurationWithPointSize(DOT_POINT_SIZE),
    ));
    dot.setUserInteractionEnabled(false);
    dot.setTranslatesAutoresizingMaskIntoConstraints(false);
    button.addSubview(&dot);
    NSLayoutConstraint::activateConstraints(
        &NSArray::from_retained_slice(&[
            dot.centerXAnchor()
                .constraintEqualToAnchor_constant(&button.centerXAnchor(), DOT_OFFSET),
            dot.centerYAnchor()
                .constraintEqualToAnchor_constant(&button.centerYAnchor(), -DOT_OFFSET),
        ]),
        mtm,
    );
    dot
}

fn configuration(bar_item: &BarItem, mtm: MainThreadMarker) -> Retained<UIButtonConfiguration> {
    let configuration = if bar_item.item.open {
        UIButtonConfiguration::tintedButtonConfiguration(mtm)
    } else {
        UIButtonConfiguration::plainButtonConfiguration(mtm)
    };
    configuration.setImage(Some(&bar_item.image));
    configuration.setPreferredSymbolConfigurationForImage(Some(
        &UIImageSymbolConfiguration::configurationWithPointSize_weight(
            SYMBOL_POINT_SIZE,
            UIImageSymbolWeight::Medium,
        ),
    ));
    configuration.setCornerStyle(UIButtonConfigurationCornerStyle::Capsule);
    if bar_item.item.mark == Some(DrawerBarMark::Alert) {
        configuration.setBaseForegroundColor(Some(&UIColor::systemRedColor()));
    } else if !bar_item.item.open {
        configuration.setBaseForegroundColor(Some(&UIColor::labelColor()));
    }
    configuration
}

fn activate(window: &WebviewWindow, id: &str) {
    let detail = serde_json::json!({ "id": id });
    if let Err(error) = window.eval(format!(
        "window.dispatchEvent(new CustomEvent('tt-drawer-bar-activate',{{detail:{detail}}}));"
    )) {
        tracing::warn!("Activate drawer bar button {id}: {error}");
    }
}
