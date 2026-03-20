#![cfg(target_os = "ios")]

use std::cell::RefCell;

use objc2::ffi::{OBJC_ASSOCIATION_RETAIN_NONATOMIC, objc_setAssociatedObject};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::{
    NSArray, NSError, NSFileManager, NSObject, NSObjectProtocol, NSString, NSURL, ns_string,
};
use objc2_ui_kit::{UIDocumentPickerDelegate, UIDocumentPickerViewController};
use objc2_uniform_type_identifiers::UTType;
use tauri::WebviewWindow;
use tokio::sync::oneshot;

use crate::domain::errors::DomainError;
use crate::infrastructure::ios_ui::resolve_presenting_view_controller;

pub struct PickedUrl {
    pub url: Retained<NSURL>,
    pub file_name: String,
}

pub enum PickZipResult {
    Cancelled,
    Picked(PickedUrl),
}

enum PickOutcome {
    Cancelled,
    Picked(PickedUrl),
    Failed(String),
}

struct DocumentPickerDelegateIvars {
    sender: RefCell<Option<oneshot::Sender<PickOutcome>>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = DocumentPickerDelegateIvars]
    struct DocumentPickerDelegate;

    impl DocumentPickerDelegate {
        #[unsafe(method_id(init))]
        fn init(this: Allocated<Self>) -> Retained<Self> {
            let this = this.set_ivars(DocumentPickerDelegateIvars {
                sender: RefCell::new(None),
            });
            unsafe { msg_send![super(this), init] }
        }
    }

    unsafe impl NSObjectProtocol for DocumentPickerDelegate {}

    #[allow(non_snake_case)]
    unsafe impl UIDocumentPickerDelegate for DocumentPickerDelegate {
        #[unsafe(method(documentPicker:didPickDocumentsAtURLs:))]
        fn documentPicker_didPickDocumentsAtURLs(
            &self,
            _controller: &UIDocumentPickerViewController,
            urls: &NSArray<NSURL>,
        ) {
            let Some(url) = urls.firstObject() else {
                self.send(PickOutcome::Failed(
                    "Document picker did not return any selected URLs".to_string(),
                ));
                return;
            };

            self.send(Self::picked_url_to_outcome(&url));
        }

        #[unsafe(method(documentPickerWasCancelled:))]
        fn documentPickerWasCancelled(&self, _controller: &UIDocumentPickerViewController) {
            self.send(PickOutcome::Cancelled);
        }

        #[unsafe(method(documentPicker:didPickDocumentAtURL:))]
        fn documentPicker_didPickDocumentAtURL(
            &self,
            _controller: &UIDocumentPickerViewController,
            url: &NSURL,
        ) {
            self.send(Self::picked_url_to_outcome(url));
        }
    }
);

impl DocumentPickerDelegate {
    fn new(mtm: MainThreadMarker, sender: oneshot::Sender<PickOutcome>) -> Retained<Self> {
        let this = Self::alloc(mtm);
        let this = this.set_ivars(DocumentPickerDelegateIvars {
            sender: RefCell::new(Some(sender)),
        });

        unsafe { msg_send![super(this), init] }
    }

    fn send(&self, outcome: PickOutcome) {
        let sender = self.ivars().sender.borrow_mut().take();
        if let Some(sender) = sender {
            let _ = sender.send(outcome);
        }
    }

    fn picked_url_to_outcome(url: &NSURL) -> PickOutcome {
        if !url.isFileURL() {
            return PickOutcome::Failed("Picked archive URL is not a file URL".to_string());
        }

        let file_name = url
            .lastPathComponent()
            .map(|value| value.to_string())
            .unwrap_or_default();

        PickOutcome::Picked(PickedUrl {
            url: Retained::from(url),
            file_name,
        })
    }
}

static DOCUMENT_PICKER_DELEGATE_KEY: u8 = 0;

unsafe fn retain_delegate(
    controller: &UIDocumentPickerViewController,
    delegate: &DocumentPickerDelegate,
) {
    unsafe {
        objc_setAssociatedObject(
            controller as *const _ as *mut AnyObject,
            (&DOCUMENT_PICKER_DELEGATE_KEY as *const u8).cast(),
            delegate as *const _ as *mut AnyObject,
            OBJC_ASSOCIATION_RETAIN_NONATOMIC,
        );
    }
}

pub async fn pick_zip_archive(window: &WebviewWindow) -> Result<PickZipResult, DomainError> {
    let (sender, receiver) = oneshot::channel::<PickOutcome>();

    window
        .run_on_main_thread(move || {
            let mut sender = Some(sender);

            let send_failure = |sender: &mut Option<oneshot::Sender<PickOutcome>>,
                                message: String| {
                if let Some(sender) = sender.take() {
                    let _ = sender.send(PickOutcome::Failed(message));
                }
            };

            let presenting = match resolve_presenting_view_controller() {
                Ok(presenting) => presenting,
                Err(error) => {
                    send_failure(&mut sender, error.to_string());
                    return;
                }
            };

            let Some(mtm) = MainThreadMarker::new() else {
                send_failure(
                    &mut sender,
                    "Document picker must be presented on the main thread".to_string(),
                );
                return;
            };

            let delegate_sender = sender
                .take()
                .expect("Document picker sender should be set before delegate creation");

            let delegate = DocumentPickerDelegate::new(mtm, delegate_sender);
            let delegate_protocol_object = ProtocolObject::from_ref(&*delegate);

            let zip_archive = match UTType::typeWithIdentifier(ns_string!("public.zip-archive")) {
                Some(value) => value,
                None => {
                    send_failure(
                        &mut sender,
                        "UTType public.zip-archive is unavailable".to_string(),
                    );
                    return;
                }
            };
            let pkware_zip = UTType::typeWithIdentifier(ns_string!("com.pkware.zip-archive"))
                .unwrap_or_else(|| zip_archive.clone());
            let content_types: Retained<NSArray<UTType>> =
                NSArray::from_retained_slice(&[zip_archive, pkware_zip]);

            let picker = UIDocumentPickerViewController::initForOpeningContentTypes_asCopy(
                UIDocumentPickerViewController::alloc(mtm),
                &content_types,
                true,
            );

            picker.setAllowsMultipleSelection(false);
            picker.setShouldShowFileExtensions(true);
            picker.setDelegate(Some(&delegate_protocol_object));

            unsafe { retain_delegate(&picker, &*delegate) };

            if let Some(popover) = picker.popoverPresentationController() {
                if let Some(source_view) = presenting.view() {
                    popover.setSourceView(Some(&source_view));
                    popover.setSourceRect(source_view.bounds());
                }
            }

            presenting.presentViewController_animated_completion(&picker, true, None);
        })
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    let outcome = receiver.await.map_err(|_| {
        DomainError::InternalError("Document picker was dismissed unexpectedly".to_string())
    })?;

    match outcome {
        PickOutcome::Cancelled => Ok(PickZipResult::Cancelled),
        PickOutcome::Picked(picked) => Ok(PickZipResult::Picked(picked)),
        PickOutcome::Failed(message) => Err(DomainError::InternalError(message)),
    }
}

struct SecurityScopedAccess<'a> {
    url: &'a NSURL,
    active: bool,
}

impl<'a> SecurityScopedAccess<'a> {
    fn start(url: &'a NSURL) -> Self {
        let active = unsafe { url.startAccessingSecurityScopedResource() };
        Self { url, active }
    }
}

impl Drop for SecurityScopedAccess<'_> {
    fn drop(&mut self) {
        if self.active {
            unsafe { self.url.stopAccessingSecurityScopedResource() };
        }
    }
}

pub fn copy_picked_url_to_path(
    source_url: &NSURL,
    target_path: &std::path::Path,
) -> Result<(), DomainError> {
    if !source_url.isFileURL() {
        return Err(DomainError::InvalidData(format!(
            "Picked URL is not a file URL: {}",
            source_url
                .absoluteString()
                .map(|value| value.to_string())
                .unwrap_or_default()
        )));
    }

    let _security_scope = SecurityScopedAccess::start(source_url);

    let target_path_string = target_path.to_string_lossy().to_string();
    let ns_target_path = NSString::from_str(&target_path_string);
    let target_url = NSURL::fileURLWithPath(&ns_target_path);

    let file_manager = NSFileManager::defaultManager();
    file_manager
        .copyItemAtURL_toURL_error(source_url, &target_url)
        .map_err(|error: Retained<NSError>| {
            DomainError::InternalError(format!(
                "Failed to copy selected archive to staging directory: {}",
                error.localizedDescription()
            ))
        })?;

    Ok(())
}
