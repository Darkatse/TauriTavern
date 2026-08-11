use std::sync::Arc;

use tauri::AppHandle;
use tt_ports::generation_background::GenerationBackgroundRuntime;

pub(crate) fn runtime(_app_handle: &AppHandle) -> Option<Arc<dyn GenerationBackgroundRuntime>> {
    #[cfg(target_os = "android")]
    {
        use tauri::Manager;

        let Some(state) =
            _app_handle.try_state::<Arc<AndroidGenerationBackgroundRuntime<tauri::Wry>>>()
        else {
            tracing::warn!(
                "Android generation background plugin is unavailable; generation will continue without background protection"
            );
            return None;
        };
        let runtime: Arc<dyn GenerationBackgroundRuntime> = state.inner().clone();
        return Some(runtime);
    }

    #[cfg(target_os = "ios")]
    {
        Some(Arc::new(IosGenerationBackgroundRuntime::new(
            _app_handle.clone(),
        )))
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    None
}

#[cfg(target_os = "android")]
mod android {
    use std::sync::Arc;

    use serde::Serialize;
    use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
    use tauri::{Manager, Runtime};
    use tt_domain::errors::DomainError;
    use tt_ports::generation_background::{
        GenerationBackgroundOutcome, GenerationBackgroundRuntime,
    };

    const PLUGIN_IDENTIFIER: &str = "com.tauritavern.client";

    pub(crate) fn plugin<R: Runtime>() -> TauriPlugin<R> {
        Builder::new("generation-background")
            .setup(|app, api| {
                let handle = match api
                    .register_android_plugin(PLUGIN_IDENTIFIER, "AiGenerationPlugin")
                {
                    Ok(handle) => handle,
                    Err(error) => {
                        tracing::warn!(
                            "Failed to register Android generation background plugin; generation will continue without background protection: {error}"
                        );
                        return Ok(());
                    }
                };
                if !app.manage(Arc::new(AndroidGenerationBackgroundRuntime { handle })) {
                    tracing::warn!("Android generation background runtime is already managed");
                }
                Ok(())
            })
            .build()
    }

    pub(super) struct AndroidGenerationBackgroundRuntime<R: Runtime> {
        handle: PluginHandle<R>,
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct StartArgs<'a> {
        task_id: &'a str,
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct FinishArgs<'a> {
        task_id: &'a str,
        outcome: &'static str,
        status_code: u16,
        notify_completion: bool,
    }

    impl<R: Runtime> GenerationBackgroundRuntime for AndroidGenerationBackgroundRuntime<R> {
        fn start(&self, task_id: &str, _user_visible: bool) -> Result<(), DomainError> {
            self.handle
                .run_mobile_plugin::<()>("start", StartArgs { task_id })
                .map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to start Android generation foreground service: {error}"
                    ))
                })
        }

        fn finish(
            &self,
            task_id: &str,
            outcome: GenerationBackgroundOutcome,
            notify_completion: bool,
        ) -> Result<(), DomainError> {
            let (outcome, status_code) = match outcome {
                GenerationBackgroundOutcome::Succeeded => ("succeeded", 0),
                GenerationBackgroundOutcome::Failed { status_code } => {
                    ("failed", status_code.unwrap_or_default())
                }
                GenerationBackgroundOutcome::Cancelled => ("cancelled", 0),
            };
            self.handle
                .run_mobile_plugin::<()>(
                    "finish",
                    FinishArgs {
                        task_id,
                        outcome,
                        status_code,
                        notify_completion,
                    },
                )
                .map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to finish Android generation foreground service: {error}"
                    ))
                })
        }
    }
}

#[cfg(target_os = "android")]
use android::AndroidGenerationBackgroundRuntime;
#[cfg(target_os = "android")]
pub(crate) use android::plugin;

#[cfg(target_os = "ios")]
mod ios {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::ptr::NonNull;
    use std::sync::{Arc, Mutex};

    use block2::RcBlock;
    use dispatch2::DispatchQueue;
    use objc2::rc::Retained;
    use objc2::runtime::AnyClass;
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_background_tasks::{
        BGContinuedProcessingTask, BGContinuedProcessingTaskRequest,
        BGContinuedProcessingTaskRequestSubmissionStrategy, BGTask, BGTaskScheduler,
    };
    use objc2_foundation::{NSProgressReporting, NSString};
    use objc2_ui_kit::{UIApplication, UIBackgroundTaskIdentifier, UIBackgroundTaskInvalid};
    use tauri::AppHandle;
    use tt_domain::errors::DomainError;
    use tt_ports::generation_background::{
        GenerationBackgroundOutcome, GenerationBackgroundRuntime,
    };

    const CONTINUED_TASK_PREFIX: &str = "com.tauritavern.client.ai-generation";

    thread_local! {
        static CONTINUED_TASKS: RefCell<HashMap<String, Retained<BGContinuedProcessingTask>>> =
            RefCell::new(HashMap::new());
    }

    struct ContinuedTaskState {
        identifier: String,
        completed_units: u64,
    }

    pub(super) struct IosGenerationBackgroundRuntime {
        app_handle: AppHandle,
        legacy_tasks: Arc<Mutex<HashMap<String, UIBackgroundTaskIdentifier>>>,
        continued_tasks: Arc<Mutex<HashMap<String, ContinuedTaskState>>>,
    }

    impl IosGenerationBackgroundRuntime {
        pub(super) fn new(app_handle: AppHandle) -> Self {
            Self {
                app_handle,
                legacy_tasks: Arc::new(Mutex::new(HashMap::new())),
                continued_tasks: Arc::new(Mutex::new(HashMap::new())),
            }
        }

        fn start_legacy(&self, task_id: &str) -> Result<(), DomainError> {
            let task_id = task_id.to_string();
            let active = self.legacy_tasks.clone();

            self.app_handle
                .run_on_main_thread(move || {
                    let marker = MainThreadMarker::new().expect("running on the iOS main thread");
                    let application = UIApplication::sharedApplication(marker);
                    let expiration_task_id = task_id.clone();
                    let expiration_active = active.clone();
                    let expiration_handler: RcBlock<dyn Fn()> = RcBlock::new(move || {
                        let identifier = expiration_active
                            .lock()
                            .expect("iOS background task mutex poisoned")
                            .remove(&expiration_task_id);
                        if let Some(identifier) = identifier {
                            let marker = MainThreadMarker::new()
                                .expect("iOS expiration handler runs on the main thread");
                            UIApplication::sharedApplication(marker).endBackgroundTask(identifier);
                            tracing::warn!(
                                task_id = expiration_task_id,
                                "iOS background protection expired; generation will continue if the app remains runnable"
                            );
                        }
                    });
                    let name = NSString::from_str("AI generation");
                    let identifier = application.beginBackgroundTaskWithName_expirationHandler(
                        Some(&name),
                        Some(&expiration_handler),
                    );

                    if identifier == unsafe { UIBackgroundTaskInvalid } {
                        tracing::warn!(
                            task_id,
                            "iOS refused the AI generation background task; generation will continue without background protection"
                        );
                        return;
                    }
                    if let Some(previous) = active
                        .lock()
                        .expect("iOS background task mutex poisoned")
                        .insert(task_id, identifier)
                    {
                        application.endBackgroundTask(previous);
                    }
                })
                .map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to dispatch iOS generation background task: {error}"
                    ))
                })
        }

        fn start_continued(&self, task_id: &str) -> Result<(), DomainError> {
            let task_id = task_id.to_string();
            let identifier = format!("{CONTINUED_TASK_PREFIX}.{}", uuid::Uuid::new_v4());
            let active = self.continued_tasks.clone();

            self.app_handle
                .run_on_main_thread(move || {
                    let scheduler = unsafe { BGTaskScheduler::sharedScheduler() };
                    let identifier_string = NSString::from_str(&identifier);
                    let launch_task_id = task_id.clone();
                    let launch_active = Arc::downgrade(&active);
                    let launch_handler: RcBlock<dyn Fn(NonNull<BGTask>)> =
                        RcBlock::new(move |raw_task: NonNull<BGTask>| {
                            let task = unsafe { Retained::retain(raw_task.as_ptr()) }
                                .expect("BGTaskScheduler supplied a null task")
                                .downcast::<BGContinuedProcessingTask>()
                                .expect("BGTaskScheduler supplied the wrong task type");
                            let Some(active) = launch_active.upgrade() else {
                                unsafe { task.setTaskCompletedWithSuccess(false) };
                                return;
                            };
                            let completed_units = active
                                .lock()
                                .expect("iOS continued task mutex poisoned")
                                .get(&launch_task_id)
                                .map(|state| state.completed_units);
                            let Some(completed_units) = completed_units else {
                                unsafe { task.setTaskCompletedWithSuccess(false) };
                                return;
                            };

                            let progress = task.progress();
                            progress.setTotalUnitCount(-1);
                            progress.setCompletedUnitCount(to_progress_units(completed_units));

                            let expiration_task = task.clone();
                            let expiration_task_id = launch_task_id.clone();
                            let expiration_active = Arc::downgrade(&active);
                            let expiration_handler: RcBlock<dyn Fn()> = RcBlock::new(move || {
                                if let Some(active) = expiration_active.upgrade() {
                                    active
                                        .lock()
                                        .expect("iOS continued task mutex poisoned")
                                        .remove(&expiration_task_id);
                                }
                                CONTINUED_TASKS.with(|tasks| {
                                    tasks.borrow_mut().remove(&expiration_task_id);
                                });
                                unsafe { expiration_task.setTaskCompletedWithSuccess(false) };
                                tracing::warn!(
                                    task_id = expiration_task_id,
                                    "iOS continued background protection expired; generation will continue if the app remains runnable"
                                );
                            });
                            unsafe { task.setExpirationHandler(Some(&expiration_handler)) };
                            CONTINUED_TASKS.with(|tasks| {
                                tasks.borrow_mut().insert(launch_task_id.clone(), task);
                            });
                        });

                    let registered = unsafe {
                        scheduler.registerForTaskWithIdentifier_usingQueue_launchHandler(
                            &identifier_string,
                            Some(DispatchQueue::main()),
                            &launch_handler,
                        )
                    };
                    if !registered {
                        tracing::warn!(
                            task_id,
                            "iOS rejected the AI generation continued task identifier; generation will continue without background protection"
                        );
                        return;
                    }

                    active
                        .lock()
                        .expect("iOS continued task mutex poisoned")
                        .insert(
                            task_id.clone(),
                            ContinuedTaskState {
                                identifier: identifier.clone(),
                                completed_units: 0,
                            },
                        );

                    let title = NSString::from_str("Preparing a reply");
                    let subtitle = NSString::from_str("This may take a moment");
                    let request = unsafe {
                        BGContinuedProcessingTaskRequest::initWithIdentifier_title_subtitle(
                            BGContinuedProcessingTaskRequest::alloc(),
                            &identifier_string,
                            &title,
                            &subtitle,
                        )
                    };
                    unsafe {
                        request.setStrategy(
                            BGContinuedProcessingTaskRequestSubmissionStrategy::Fail,
                        )
                    };
                    if let Err(error) = unsafe { scheduler.submitTaskRequest_error(&request) } {
                        active
                            .lock()
                            .expect("iOS continued task mutex poisoned")
                            .remove(&task_id);
                        tracing::warn!(
                            task_id,
                            "iOS refused the AI generation continued task; generation will continue without background protection: {error:?}"
                        );
                    }
                })
                .map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to dispatch iOS continued background task: {error}"
                    ))
                })
        }
    }

    impl GenerationBackgroundRuntime for IosGenerationBackgroundRuntime {
        fn start(&self, task_id: &str, user_visible: bool) -> Result<(), DomainError> {
            if user_visible && AnyClass::get(c"BGContinuedProcessingTaskRequest").is_some() {
                self.start_continued(task_id)
            } else {
                self.start_legacy(task_id)
            }
        }

        fn report_progress(&self, task_id: &str, completed_units: u64) -> Result<(), DomainError> {
            let task_id = task_id.to_string();
            let active = self.continued_tasks.clone();
            let mut tasks = active.lock().expect("iOS continued task mutex poisoned");
            let Some(state) = tasks.get_mut(&task_id) else {
                return Ok(());
            };
            state.completed_units = completed_units;
            drop(tasks);

            self.app_handle
                .run_on_main_thread(move || {
                    CONTINUED_TASKS.with(|tasks| {
                        if let Some(task) = tasks.borrow().get(&task_id) {
                            task.progress()
                                .setCompletedUnitCount(to_progress_units(completed_units));
                        }
                    });
                })
                .map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to dispatch iOS continued task progress: {error}"
                    ))
                })
        }

        fn finish(
            &self,
            task_id: &str,
            outcome: GenerationBackgroundOutcome,
            _notify_completion: bool,
        ) -> Result<(), DomainError> {
            let task_id = task_id.to_string();
            let legacy_tasks = self.legacy_tasks.clone();
            let continued_tasks = self.continued_tasks.clone();
            let succeeded = matches!(outcome, GenerationBackgroundOutcome::Succeeded);
            self.app_handle
                .run_on_main_thread(move || {
                    let legacy_identifier = legacy_tasks
                        .lock()
                        .expect("iOS background task mutex poisoned")
                        .remove(&task_id);
                    if let Some(identifier) = legacy_identifier {
                        let marker =
                            MainThreadMarker::new().expect("running on the iOS main thread");
                        UIApplication::sharedApplication(marker).endBackgroundTask(identifier);
                        return;
                    }

                    let continued = continued_tasks
                        .lock()
                        .expect("iOS continued task mutex poisoned")
                        .remove(&task_id);
                    let Some(continued) = continued else {
                        return;
                    };
                    let task = CONTINUED_TASKS.with(|tasks| tasks.borrow_mut().remove(&task_id));
                    if let Some(task) = task {
                        complete_continued_task(&task, succeeded);
                    } else {
                        unsafe {
                            BGTaskScheduler::sharedScheduler().cancelTaskRequestWithIdentifier(
                                &NSString::from_str(&continued.identifier),
                            )
                        };
                    }
                })
                .map_err(|error| {
                    DomainError::InternalError(format!(
                        "Failed to end iOS generation background task: {error}"
                    ))
                })
        }
    }

    fn to_progress_units(units: u64) -> i64 {
        units.min(i64::MAX as u64) as i64
    }

    fn complete_continued_task(task: &BGContinuedProcessingTask, succeeded: bool) {
        if succeeded {
            let progress = task.progress();
            let total = progress.completedUnitCount().max(1);
            progress.setTotalUnitCount(total);
            progress.setCompletedUnitCount(total);
        }
        unsafe { task.setTaskCompletedWithSuccess(succeeded) };
    }
}

#[cfg(target_os = "ios")]
use ios::IosGenerationBackgroundRuntime;
