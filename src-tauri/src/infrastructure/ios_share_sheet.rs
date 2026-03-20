#![cfg(target_os = "ios")]

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_foundation::{NSArray, NSError, NSString, NSURL};
use objc2_ui_kit::{UIActivityType, UIActivityViewController};
use tauri::WebviewWindow;
use tokio::sync::oneshot;

use crate::domain::errors::DomainError;
use crate::infrastructure::ios_ui::resolve_presenting_view_controller;

#[derive(Debug, Clone)]
pub struct ShareResult {
    pub completed: bool,
    pub activity: Option<String>,
}

enum ShareOutcome {
    Finished(ShareResult),
    Failed(String),
}

pub async fn share_file(
    window: &WebviewWindow,
    file_path: &std::path::Path,
) -> Result<ShareResult, DomainError> {
    if !file_path.is_file() {
        return Err(DomainError::NotFound(format!(
            "Export archive file not found: {}",
            file_path.display()
        )));
    }

    let file_path_string = file_path.to_string_lossy().to_string();
    let (sender, receiver) = oneshot::channel::<ShareOutcome>();

    window
        .run_on_main_thread(move || {
            let mut sender = Some(sender);

            let send_failure = |sender: &mut Option<oneshot::Sender<ShareOutcome>>,
                                message: String| {
                if let Some(sender) = sender.take() {
                    let _ = sender.send(ShareOutcome::Failed(message));
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
                    "Share sheet must be presented on the main thread".to_string(),
                );
                return;
            };

            let ns_path = NSString::from_str(&file_path_string);
            let url = NSURL::fileURLWithPath(&ns_path);
            let activity_items = NSArray::from_retained_slice(&[Retained::from(url)]);

            let controller = unsafe {
                UIActivityViewController::initWithActivityItems_applicationActivities(
                    UIActivityViewController::alloc(mtm),
                    &activity_items,
                    None,
                )
            };

            let completion_sender = std::cell::RefCell::new(sender.take());
            let completion_block: RcBlock<
                dyn Fn(*mut UIActivityType, Bool, *mut NSArray, *mut NSError),
            > = RcBlock::new(
                move |activity_type: *mut UIActivityType,
                      completed: Bool,
                      _items: *mut NSArray,
                      error: *mut NSError| {
                    let sender = completion_sender.borrow_mut().take();
                    let Some(sender) = sender else {
                        return;
                    };

                    if let Some(error) = unsafe { error.as_ref() } {
                        let _ = sender.send(ShareOutcome::Failed(
                            error.localizedDescription().to_string(),
                        ));
                        return;
                    }

                    let activity = unsafe { activity_type.as_ref() }.map(|value| value.to_string());
                    let _ = sender.send(ShareOutcome::Finished(ShareResult {
                        completed: completed.as_bool(),
                        activity,
                    }));
                },
            );

            unsafe { controller.setCompletionWithItemsHandler(RcBlock::as_ptr(&completion_block)) };

            if let Some(popover) = controller.popoverPresentationController() {
                if let Some(source_view) = presenting.view() {
                    popover.setSourceView(Some(&source_view));
                    popover.setSourceRect(source_view.bounds());
                }
            }

            presenting.presentViewController_animated_completion(&controller, true, None);
        })
        .map_err(|error| DomainError::InternalError(error.to_string()))?;

    let outcome = receiver.await.map_err(|_| {
        DomainError::InternalError("Share sheet was dismissed unexpectedly".to_string())
    })?;

    match outcome {
        ShareOutcome::Finished(result) => Ok(result),
        ShareOutcome::Failed(message) => Err(DomainError::InternalError(message)),
    }
}
