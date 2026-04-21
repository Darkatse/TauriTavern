#![cfg(any(target_os = "ios", target_os = "macos"))]

use block2::RcBlock;
use objc2::ffi::{
    OBJC_ASSOCIATION_RETAIN_NONATOMIC, objc_getAssociatedObject, objc_setAssociatedObject,
};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject, AnyProtocol, NSObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_foundation::NSString;

struct Ivars {
    original_delegate: Option<Retained<AnyObject>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    struct TauriTavernWkUiDialogDelegate;

    impl TauriTavernWkUiDialogDelegate {
        #[unsafe(method_id(initWithOriginalDelegate:))]
        fn init_with_original_delegate(
            this: Allocated<Self>,
            original_delegate: Option<&AnyObject>,
        ) -> Retained<Self> {
            let this = this.set_ivars(Ivars {
                original_delegate: original_delegate.map(|value| value.retain()),
            });
            unsafe { msg_send![super(this), init] }
        }

        #[unsafe(method(respondsToSelector:))]
        fn responds_to_selector(&self, selector: Sel) -> objc2::runtime::Bool {
            let responds_super: objc2::runtime::Bool =
                unsafe { msg_send![super(self), respondsToSelector: selector] };
            if responds_super.as_bool() {
                return responds_super;
            }

            let Some(delegate) = self.ivars().original_delegate.as_ref() else {
                return objc2::runtime::Bool::NO;
            };

            let delegate: &AnyObject = delegate;
            let responds: bool = unsafe { msg_send![delegate, respondsToSelector: selector] };
            objc2::runtime::Bool::new(responds)
        }

        #[unsafe(method(forwardingTargetForSelector:))]
        fn forwarding_target_for_selector(&self, selector: Sel) -> Option<&AnyObject> {
            let Some(delegate) = self.ivars().original_delegate.as_ref() else {
                return None;
            };

            let delegate: &AnyObject = delegate;
            let responds: bool = unsafe { msg_send![delegate, respondsToSelector: selector] };
            if responds { Some(delegate) } else { None }
        }

        #[unsafe(method(conformsToProtocol:))]
        fn conforms_to_protocol(&self, protocol: &AnyProtocol) -> objc2::runtime::Bool {
            let conforms_super: objc2::runtime::Bool =
                unsafe { msg_send![super(self), conformsToProtocol: protocol] };
            if conforms_super.as_bool() {
                return conforms_super;
            }

            let Some(delegate) = self.ivars().original_delegate.as_ref() else {
                return objc2::runtime::Bool::NO;
            };

            let delegate: &AnyObject = delegate;
            let conforms: bool = unsafe { msg_send![delegate, conformsToProtocol: protocol] };
            objc2::runtime::Bool::new(conforms)
        }

        #[unsafe(method(webView:runJavaScriptAlertPanelWithMessage:initiatedByFrame:completionHandler:))]
        fn run_alert_panel(
            &self,
            _web_view: &AnyObject,
            message: &NSString,
            _frame: &AnyObject,
            completion_handler: &block2::DynBlock<dyn Fn()>,
        ) {
            let completion_handler = completion_handler.copy();
            platform::present_alert(message, completion_handler);
        }

        #[unsafe(method(webView:runJavaScriptConfirmPanelWithMessage:initiatedByFrame:completionHandler:))]
        fn run_confirm_panel(
            &self,
            _web_view: &AnyObject,
            message: &NSString,
            _frame: &AnyObject,
            completion_handler: &block2::DynBlock<dyn Fn(objc2::runtime::Bool)>,
        ) {
            let completion_handler = completion_handler.copy();
            platform::present_confirm(message, completion_handler);
        }

        #[unsafe(method(webView:runJavaScriptTextInputPanelWithPrompt:defaultText:initiatedByFrame:completionHandler:))]
        fn run_text_input_panel(
            &self,
            _web_view: &AnyObject,
            prompt: &NSString,
            default_text: Option<&NSString>,
            _frame: &AnyObject,
            completion_handler: &block2::DynBlock<dyn Fn(*mut NSString)>,
        ) {
            let completion_handler = completion_handler.copy();
            platform::present_prompt(prompt, default_text, completion_handler);
        }
    }
);

impl TauriTavernWkUiDialogDelegate {
    fn new(original_delegate: Option<&AnyObject>) -> Retained<Self> {
        let mtm =
            MainThreadMarker::new().expect("WKUIDelegate must be installed on the main thread");
        unsafe {
            msg_send![
                Self::alloc(mtm),
                initWithOriginalDelegate: original_delegate
            ]
        }
    }
}

static UIDIALOG_DELEGATE_KEY: u8 = 0;

pub unsafe fn install_js_dialog_ui_delegate(wkwebview: &AnyObject) {
    let wkwebview = wkwebview as *const AnyObject as *mut AnyObject;
    let key = std::ptr::from_ref(&UIDIALOG_DELEGATE_KEY).cast();
    let existing = unsafe { objc_getAssociatedObject(wkwebview, key) };
    if !existing.is_null() {
        return;
    }

    let original_delegate: Option<&AnyObject> = unsafe { msg_send![&*wkwebview, UIDelegate] };
    let delegate = TauriTavernWkUiDialogDelegate::new(original_delegate);

    let _: () = unsafe { msg_send![&*wkwebview, setUIDelegate: &*delegate] };
    unsafe {
        objc_setAssociatedObject(
            wkwebview,
            key,
            Retained::as_ptr(&delegate).cast_mut().cast(),
            OBJC_ASSOCIATION_RETAIN_NONATOMIC,
        );
    }
}

#[cfg(target_os = "ios")]
mod platform {
    use super::*;
    use objc2::MainThreadMarker;
    use objc2_ui_kit::{
        UIAlertAction, UIAlertActionStyle, UIAlertController, UIAlertControllerStyle, UITextField,
    };

    use crate::infrastructure::ios_ui::resolve_presenting_view_controller;

    pub(super) fn present_alert(message: &NSString, completion_handler: RcBlock<dyn Fn()>) {
        let presenting = match resolve_presenting_view_controller() {
            Ok(presenting) => presenting,
            Err(error) => {
                tracing::error!(
                    "[WKUIDelegate] Failed to resolve presenting view controller: {error}"
                );
                completion_handler.call(());
                return;
            }
        };

        let Some(mtm) = MainThreadMarker::new() else {
            tracing::error!("[WKUIDelegate] JS dialog must be presented on the main thread");
            completion_handler.call(());
            return;
        };

        let controller = UIAlertController::alertControllerWithTitle_message_preferredStyle(
            None,
            Some(message),
            UIAlertControllerStyle::Alert,
            mtm,
        );

        let ok_title = NSString::from_str("OK");
        let completion_cell = std::cell::RefCell::new(Some(completion_handler));
        let ok_block: RcBlock<dyn Fn(std::ptr::NonNull<UIAlertAction>)> = RcBlock::new(move |_| {
            let handler = completion_cell.borrow_mut().take();
            if let Some(handler) = handler {
                handler.call(());
            }
        });
        let ok_action = UIAlertAction::actionWithTitle_style_handler(
            Some(&*ok_title),
            UIAlertActionStyle::Default,
            Some(&ok_block),
            mtm,
        );
        controller.addAction(&ok_action);

        presenting.presentViewController_animated_completion(&controller, true, None);
    }

    pub(super) fn present_confirm(
        message: &NSString,
        completion_handler: RcBlock<dyn Fn(objc2::runtime::Bool)>,
    ) {
        let presenting = match resolve_presenting_view_controller() {
            Ok(presenting) => presenting,
            Err(error) => {
                tracing::error!(
                    "[WKUIDelegate] Failed to resolve presenting view controller: {error}"
                );
                completion_handler.call((objc2::runtime::Bool::NO,));
                return;
            }
        };

        let Some(mtm) = MainThreadMarker::new() else {
            tracing::error!("[WKUIDelegate] JS dialog must be presented on the main thread");
            completion_handler.call((objc2::runtime::Bool::NO,));
            return;
        };

        let controller = UIAlertController::alertControllerWithTitle_message_preferredStyle(
            None,
            Some(message),
            UIAlertControllerStyle::Alert,
            mtm,
        );

        let ok_title = NSString::from_str("OK");
        let cancel_title = NSString::from_str("Cancel");
        let completion_cell = std::rc::Rc::new(std::cell::RefCell::new(Some(completion_handler)));

        let ok_completion = completion_cell.clone();
        let ok_block: RcBlock<dyn Fn(std::ptr::NonNull<UIAlertAction>)> = RcBlock::new(move |_| {
            let handler = ok_completion.borrow_mut().take();
            if let Some(handler) = handler {
                handler.call((objc2::runtime::Bool::YES,));
            }
        });

        let cancel_completion = completion_cell.clone();
        let cancel_block: RcBlock<dyn Fn(std::ptr::NonNull<UIAlertAction>)> =
            RcBlock::new(move |_| {
                let handler = cancel_completion.borrow_mut().take();
                if let Some(handler) = handler {
                    handler.call((objc2::runtime::Bool::NO,));
                }
            });

        let ok_action = UIAlertAction::actionWithTitle_style_handler(
            Some(&*ok_title),
            UIAlertActionStyle::Default,
            Some(&ok_block),
            mtm,
        );
        let cancel_action = UIAlertAction::actionWithTitle_style_handler(
            Some(&*cancel_title),
            UIAlertActionStyle::Cancel,
            Some(&cancel_block),
            mtm,
        );

        controller.addAction(&cancel_action);
        controller.addAction(&ok_action);
        controller.setPreferredAction(Some(&*ok_action));

        presenting.presentViewController_animated_completion(&controller, true, None);
    }

    pub(super) fn present_prompt(
        prompt: &NSString,
        default_text: Option<&NSString>,
        completion_handler: RcBlock<dyn Fn(*mut NSString)>,
    ) {
        let presenting = match resolve_presenting_view_controller() {
            Ok(presenting) => presenting,
            Err(error) => {
                tracing::error!(
                    "[WKUIDelegate] Failed to resolve presenting view controller: {error}"
                );
                completion_handler.call((std::ptr::null_mut(),));
                return;
            }
        };

        let Some(mtm) = MainThreadMarker::new() else {
            tracing::error!("[WKUIDelegate] JS dialog must be presented on the main thread");
            completion_handler.call((std::ptr::null_mut(),));
            return;
        };

        let controller = UIAlertController::alertControllerWithTitle_message_preferredStyle(
            None,
            Some(prompt),
            UIAlertControllerStyle::Alert,
            mtm,
        );

        let default_text = default_text.map(|value| value.retain());
        let text_field_cell: std::rc::Rc<std::cell::RefCell<Option<Retained<UITextField>>>> =
            std::rc::Rc::new(std::cell::RefCell::new(None));
        let config_cell = text_field_cell.clone();
        let config_block: RcBlock<dyn Fn(std::ptr::NonNull<UITextField>)> =
            RcBlock::new(move |field: std::ptr::NonNull<UITextField>| {
                let field = unsafe { field.as_ref() };
                field.setText(default_text.as_deref());
                *config_cell.borrow_mut() = Some(field.retain());
            });
        controller.addTextFieldWithConfigurationHandler(Some(&config_block));

        let ok_title = NSString::from_str("OK");
        let cancel_title = NSString::from_str("Cancel");
        let completion_cell = std::rc::Rc::new(std::cell::RefCell::new(Some(completion_handler)));

        let ok_completion = completion_cell.clone();
        let ok_text_field_cell = text_field_cell.clone();
        let ok_block: RcBlock<dyn Fn(std::ptr::NonNull<UIAlertAction>)> = RcBlock::new(move |_| {
            let handler = ok_completion.borrow_mut().take();
            let Some(handler) = handler else {
                return;
            };

            let text = ok_text_field_cell
                .borrow()
                .as_ref()
                .and_then(|field| field.text());
            let ptr = text
                .as_ref()
                .map(|value| Retained::as_ptr(value).cast_mut())
                .unwrap_or(std::ptr::null_mut());
            handler.call((ptr,));
        });

        let cancel_completion = completion_cell.clone();
        let cancel_block: RcBlock<dyn Fn(std::ptr::NonNull<UIAlertAction>)> =
            RcBlock::new(move |_| {
                let handler = cancel_completion.borrow_mut().take();
                if let Some(handler) = handler {
                    handler.call((std::ptr::null_mut(),));
                }
            });

        let ok_action = UIAlertAction::actionWithTitle_style_handler(
            Some(&*ok_title),
            UIAlertActionStyle::Default,
            Some(&ok_block),
            mtm,
        );
        let cancel_action = UIAlertAction::actionWithTitle_style_handler(
            Some(&*cancel_title),
            UIAlertActionStyle::Cancel,
            Some(&cancel_block),
            mtm,
        );

        controller.addAction(&cancel_action);
        controller.addAction(&ok_action);
        controller.setPreferredAction(Some(&*ok_action));

        presenting.presentViewController_animated_completion(&controller, true, None);
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use objc2::runtime::AnyClass;
    use objc2_foundation::NSSize;

    pub(super) fn present_alert(message: &NSString, completion_handler: RcBlock<dyn Fn()>) {
        if let Err(message) = run_alert(message) {
            tracing::error!("[WKUIDelegate] Failed to present JS alert: {message}");
        }
        completion_handler.call(());
    }

    pub(super) fn present_confirm(
        message: &NSString,
        completion_handler: RcBlock<dyn Fn(objc2::runtime::Bool)>,
    ) {
        let ok = run_confirm(message).unwrap_or_else(|message| {
            tracing::error!("[WKUIDelegate] Failed to present JS confirm: {message}");
            false
        });
        completion_handler.call((objc2::runtime::Bool::new(ok),));
    }

    pub(super) fn present_prompt(
        prompt: &NSString,
        default_text: Option<&NSString>,
        completion_handler: RcBlock<dyn Fn(*mut NSString)>,
    ) {
        let result = run_prompt(prompt, default_text).unwrap_or_else(|message| {
            tracing::error!("[WKUIDelegate] Failed to present JS prompt: {message}");
            None
        });
        let ptr = result
            .as_ref()
            .map(|value| Retained::as_ptr(value).cast_mut())
            .unwrap_or(std::ptr::null_mut());
        completion_handler.call((ptr,));
    }

    fn nsalert_class() -> Result<&'static AnyClass, String> {
        AnyClass::get(unsafe { std::ffi::CStr::from_bytes_with_nul_unchecked(b"NSAlert\0") })
            .ok_or_else(|| "NSAlert class is unavailable".to_string())
    }

    fn nstextfield_class() -> Result<&'static AnyClass, String> {
        AnyClass::get(unsafe { std::ffi::CStr::from_bytes_with_nul_unchecked(b"NSTextField\0") })
            .ok_or_else(|| "NSTextField class is unavailable".to_string())
    }

    fn make_alert(informative_text: &NSString) -> Result<Retained<AnyObject>, String> {
        let Some(_mtm) = MainThreadMarker::new() else {
            return Err("JS dialog must be presented on the main thread".to_string());
        };

        let alert_class = nsalert_class()?;
        let alert: Retained<AnyObject> = unsafe { msg_send![alert_class, new] };

        let title = NSString::from_str("TauriTavern");
        let _: () = unsafe { msg_send![&*alert, setMessageText: &*title] };
        let _: () = unsafe { msg_send![&*alert, setInformativeText: informative_text] };
        Ok(alert)
    }

    fn run_alert(message: &NSString) -> Result<(), String> {
        let alert = make_alert(message)?;
        let ok = NSString::from_str("OK");
        let _: Retained<AnyObject> = unsafe { msg_send![&*alert, addButtonWithTitle: &*ok] };
        let _: isize = unsafe { msg_send![&*alert, runModal] };
        Ok(())
    }

    fn run_confirm(message: &NSString) -> Result<bool, String> {
        let alert = make_alert(message)?;
        let ok = NSString::from_str("OK");
        let cancel = NSString::from_str("Cancel");
        let _: Retained<AnyObject> = unsafe { msg_send![&*alert, addButtonWithTitle: &*ok] };
        let _: Retained<AnyObject> = unsafe { msg_send![&*alert, addButtonWithTitle: &*cancel] };

        let response: isize = unsafe { msg_send![&*alert, runModal] };
        Ok(response == 1000)
    }

    fn run_prompt(
        prompt: &NSString,
        default_text: Option<&NSString>,
    ) -> Result<Option<Retained<NSString>>, String> {
        let alert = make_alert(prompt)?;

        let field_class = nstextfield_class()?;
        let field: Retained<AnyObject> = unsafe { msg_send![field_class, new] };
        let size = NSSize::new(320.0, 24.0);
        let _: () = unsafe { msg_send![&*field, setFrameSize: size] };
        let _: () = unsafe { msg_send![&*field, setBezeled: true] };
        let _: () = unsafe { msg_send![&*field, setBordered: true] };
        let _: () = unsafe { msg_send![&*field, setEditable: true] };
        let _: () = unsafe { msg_send![&*field, setSelectable: true] };
        let _: () = unsafe { msg_send![&*field, setDrawsBackground: true] };

        let empty = NSString::from_str("");
        let initial = default_text.unwrap_or(&*empty);
        let _: () = unsafe { msg_send![&*field, setStringValue: initial] };
        let _: () = unsafe { msg_send![&*alert, setAccessoryView: &*field] };

        let ok = NSString::from_str("OK");
        let cancel = NSString::from_str("Cancel");
        let _: Retained<AnyObject> = unsafe { msg_send![&*alert, addButtonWithTitle: &*ok] };
        let _: Retained<AnyObject> = unsafe { msg_send![&*alert, addButtonWithTitle: &*cancel] };

        let response: isize = unsafe { msg_send![&*alert, runModal] };
        if response != 1000 {
            return Ok(None);
        }

        let value: Retained<NSString> = unsafe { msg_send![&*field, stringValue] };
        Ok(Some(value))
    }
}
