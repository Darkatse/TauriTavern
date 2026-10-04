#![cfg(target_os = "ios")]

use std::cell::RefCell;
use std::path::PathBuf;

use objc2::ffi::{OBJC_ASSOCIATION_RETAIN_NONATOMIC, objc_setAssociatedObject};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSString, NSURL};
use objc2_ui_kit::{UIDocumentPickerDelegate, UIDocumentPickerViewController};
use objc2_uniform_type_identifiers::UTType;
use tauri::WebviewWindow;
use tokio::sync::oneshot;

use crate::platform::file_transfer::{PickedFile, PickedSource};
use crate::platform::ios_ui::resolve_presenting_view_controller;
use tt_domain::errors::DomainError;

enum PickOutcome {
    Cancelled,
    Picked(Vec<PickedFile>),
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
            self.send(Self::picked_urls_to_outcome(urls));
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
            self.send(match Self::picked_url(url) {
                Ok(picked) => PickOutcome::Picked(vec![picked]),
                Err(message) => PickOutcome::Failed(message),
            });
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

    fn picked_urls_to_outcome(urls: &NSArray<NSURL>) -> PickOutcome {
        let urls = urls.to_vec();
        if urls.is_empty() {
            return PickOutcome::Failed(
                "Document picker did not return any selected URLs".to_string(),
            );
        }

        let mut picked = Vec::with_capacity(urls.len());
        for url in urls {
            match Self::picked_url(&url) {
                Ok(url) => picked.push(url),
                Err(message) => return PickOutcome::Failed(message),
            }
        }
        PickOutcome::Picked(picked)
    }

    fn picked_url(url: &NSURL) -> Result<PickedFile, String> {
        if !url.isFileURL() {
            return Err("Picked document URL is not a file URL".to_string());
        }

        let file_name = url
            .lastPathComponent()
            .map(|value| value.to_string())
            .unwrap_or_default();

        let path = url
            .path()
            .ok_or_else(|| "Picked document URL has no path".to_string())?;
        Ok(PickedFile {
            name: file_name,
            source: PickedSource::AppCopy(PathBuf::from(path.to_string())),
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

fn resolve_content_types(identifiers: &[&str]) -> Result<Retained<NSArray<UTType>>, String> {
    let mut content_types = Vec::with_capacity(identifiers.len());
    for identifier in identifiers {
        let ns_identifier = NSString::from_str(identifier);
        if let Some(content_type) = UTType::typeWithIdentifier(&ns_identifier) {
            content_types.push(content_type);
        }
    }

    if content_types.is_empty() {
        return Err(format!(
            "No supported iOS document picker content types are available: {}",
            identifiers.join(", ")
        ));
    }

    Ok(NSArray::from_retained_slice(&content_types))
}

pub async fn pick_documents(
    window: &WebviewWindow,
    identifiers: &'static [&'static str],
    allows_multiple_selection: bool,
) -> Result<Option<Vec<PickedFile>>, DomainError> {
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

            let content_types = match resolve_content_types(identifiers) {
                Ok(content_types) => content_types,
                Err(message) => {
                    send_failure(&mut sender, message);
                    return;
                }
            };

            let delegate_sender = sender
                .take()
                .expect("Document picker sender should be set before delegate creation");

            let delegate = DocumentPickerDelegate::new(mtm, delegate_sender);
            let delegate_protocol_object = ProtocolObject::from_ref(&*delegate);

            let picker = UIDocumentPickerViewController::initForOpeningContentTypes_asCopy(
                UIDocumentPickerViewController::alloc(mtm),
                &content_types,
                true,
            );

            picker.setAllowsMultipleSelection(allows_multiple_selection);
            picker.setShouldShowFileExtensions(true);
            picker.setDelegate(Some(delegate_protocol_object));

            unsafe { retain_delegate(&picker, &delegate) };

            if let Some(popover) = picker.popoverPresentationController()
                && let Some(source_view) = presenting.view()
            {
                popover.setSourceView(Some(&source_view));
                popover.setSourceRect(source_view.bounds());
            }

            presenting.presentViewController_animated_completion(&picker, true, None);
        })
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    let outcome = receiver.await.map_err(|_| {
        DomainError::InternalError("Document picker was dismissed unexpectedly".to_string())
    })?;

    match outcome {
        PickOutcome::Cancelled => Ok(None),
        PickOutcome::Picked(picked) => Ok(Some(picked)),
        PickOutcome::Failed(message) => Err(DomainError::InternalError(message)),
    }
}
