#![cfg(target_os = "ios")]

use std::cell::RefCell;

use objc2::ffi::{OBJC_ASSOCIATION_RETAIN_NONATOMIC, objc_setAssociatedObject};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::{
    NSArray, NSError, NSFileManager, NSObject, NSObjectProtocol, NSString, NSURL,
};
use objc2_ui_kit::{UIDocumentPickerDelegate, UIDocumentPickerViewController};
use objc2_uniform_type_identifiers::UTType;
use tauri::WebviewWindow;
use tokio::sync::oneshot;

use crate::domain::errors::DomainError;
use crate::infrastructure::ios_ui::resolve_presenting_view_controller;

const DATA_ARCHIVE_CONTENT_TYPES: &[&str] = &[
    "public.zip-archive",
    "com.pkware.zip-archive",
    "public.tar-archive",
    "org.gnu.gnu-zip-archive",
    "com.tauritavern.client.tar-archive",
    "com.tauritavern.client.gzip-archive",
];
const SKILL_IMPORT_CONTENT_TYPES: &[&str] = &[
    "public.zip-archive",
    "com.pkware.zip-archive",
    "public.data",
];

pub struct PickedUrl {
    pub url: Retained<NSURL>,
    pub file_name: String,
}

pub enum PickDocumentResult {
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

async fn pick_archive_with_content_types(
    window: &WebviewWindow,
    identifiers: &'static [&'static str],
) -> Result<PickDocumentResult, DomainError> {
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

            let content_types = match resolve_content_types(identifiers) {
                Ok(content_types) => content_types,
                Err(message) => {
                    send_failure(&mut sender, message);
                    return;
                }
            };

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
        PickOutcome::Cancelled => Ok(PickDocumentResult::Cancelled),
        PickOutcome::Picked(picked) => Ok(PickDocumentResult::Picked(picked)),
        PickOutcome::Failed(message) => Err(DomainError::InternalError(message)),
    }
}

pub async fn pick_data_archive(window: &WebviewWindow) -> Result<PickDocumentResult, DomainError> {
    pick_archive_with_content_types(window, DATA_ARCHIVE_CONTENT_TYPES).await
}

pub async fn pick_skill_import_archive(
    window: &WebviewWindow,
) -> Result<PickDocumentResult, DomainError> {
    pick_archive_with_content_types(window, SKILL_IMPORT_CONTENT_TYPES).await
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
