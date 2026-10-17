use std::collections::HashSet;
use std::sync::Arc;

use tauri::AppHandle;
use tt_ports::generation_background::GenerationBackgroundRuntime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(target_os = "android", derive(serde::Serialize))]
#[cfg_attr(target_os = "android", serde(rename_all = "camelCase"))]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) struct KeepAliveState {
    pub(crate) active: bool,
    pub(crate) needs_cpu: bool,
    pub(crate) persistent: bool,
}

#[derive(Debug, Default)]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) struct KeepAliveTracker {
    active_tasks: HashSet<String>,
    persistent: bool,
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
impl KeepAliveTracker {
    pub(crate) fn state(&self) -> KeepAliveState {
        let generations_running = !self.active_tasks.is_empty();
        KeepAliveState {
            active: generations_running || self.persistent,
            needs_cpu: generations_running,
            persistent: self.persistent,
        }
    }

    pub(crate) fn start_task(&mut self, task_id: &str) -> KeepAliveState {
        let trimmed = task_id.trim();
        if !trimmed.is_empty() && !self.active_tasks.contains(trimmed) {
            self.active_tasks.insert(trimmed.to_owned());
        }
        self.state()
    }

    pub(crate) fn finish_task(&mut self, task_id: &str) -> (KeepAliveState, bool) {
        let trimmed = task_id.trim();
        let removed = !trimmed.is_empty() && self.active_tasks.remove(trimmed);
        let state = self.state();
        (state, removed && !state.needs_cpu)
    }

    pub(crate) fn set_persistent(&mut self, persistent: bool) -> (KeepAliveState, bool) {
        let previous = self.state();
        self.persistent = persistent;
        let next = self.state();
        (next, next != previous)
    }
}

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
        Some(runtime)
    }

    #[cfg(target_os = "ios")]
    {
        Some(Arc::new(IosGenerationBackgroundRuntime::new(
            _app_handle.clone(),
        )))
    }

    // OHOS has no native generation-background adapter yet.
    #[cfg(any(desktop, target_env = "ohos"))]
    None
}

pub(crate) async fn request_notification_permission_if_needed(_app_handle: &AppHandle) {
    #[cfg(target_os = "android")]
    {
        use tauri_plugin_notification::{NotificationExt, PermissionState};

        let app_handle = _app_handle.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            let notification = app_handle.notification();
            if matches!(
                notification.permission_state(),
                Ok(PermissionState::Prompt | PermissionState::PromptWithRationale)
            ) && let Err(error) = notification.request_permission()
            {
                tracing::warn!("Failed to request Android notification permission: {error}");
            }
        })
        .await;
    }
}

pub(crate) fn set_persistent_keep_alive(_app_handle: &AppHandle, _persistent: bool) {
    #[cfg(target_os = "android")]
    {
        use tauri::Manager;

        let Some(state) =
            _app_handle.try_state::<Arc<AndroidGenerationBackgroundRuntime<tauri::Wry>>>()
        else {
            tracing::warn!(
                "Android generation background plugin is unavailable; cannot update persistent keep-alive state"
            );
            return;
        };
        if let Err(error) = state.set_persistent(_persistent) {
            tracing::warn!("Failed to update Android persistent keep-alive state: {error}");
        }
    }
}

#[cfg(target_os = "android")]
mod android {
    use std::sync::{Arc, Mutex};

    use serde::Serialize;
    use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
    use tauri::{Manager, Runtime};
    use tt_domain::errors::DomainError;
    use tt_ports::generation_background::{
        GenerationBackgroundOutcome, GenerationBackgroundRuntime,
    };

    use super::{KeepAliveState, KeepAliveTracker};

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
                if !app.manage(Arc::new(AndroidGenerationBackgroundRuntime {
                    handle,
                    tracker: Mutex::new(KeepAliveTracker::default()),
                })) {
                    tracing::warn!("Android generation background runtime is already managed");
                }
                Ok(())
            })
            .build()
    }

    pub(super) struct AndroidGenerationBackgroundRuntime<R: Runtime> {
        handle: PluginHandle<R>,
        tracker: Mutex<KeepAliveTracker>,
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct SyncStateArgs {
        #[serde(flatten)]
        state: KeepAliveState,
        #[serde(skip_serializing_if = "Option::is_none")]
        outcome: Option<&'static str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        status_code: Option<u16>,
        #[serde(skip_serializing_if = "Option::is_none")]
        notify_completion: Option<bool>,
    }

    impl<R: Runtime> AndroidGenerationBackgroundRuntime<R> {
        pub(super) fn set_persistent(&self, persistent: bool) -> Result<(), DomainError> {
            let mut tracker = self
                .tracker
                .lock()
                .expect("Android keep-alive tracker mutex poisoned");
            let (state, changed) = tracker.set_persistent(persistent);
            if !changed {
                return Ok(());
            }
            self.sync_state(
                state,
                None,
                "Failed to update Android background keep-alive service",
            )
        }

        fn sync_state(
            &self,
            state: KeepAliveState,
            completion: Option<(GenerationBackgroundOutcome, bool)>,
            context: &'static str,
        ) -> Result<(), DomainError> {
            let (outcome, status_code, notify_completion) = match completion {
                Some((GenerationBackgroundOutcome::Succeeded, notify)) => {
                    (Some("succeeded"), Some(0), Some(notify))
                }
                Some((GenerationBackgroundOutcome::Failed { status_code }, notify)) => (
                    Some("failed"),
                    Some(status_code.unwrap_or_default()),
                    Some(notify),
                ),
                Some((GenerationBackgroundOutcome::Cancelled, notify)) => {
                    (Some("cancelled"), Some(0), Some(notify))
                }
                None => (None, None, None),
            };
            self.handle
                .run_mobile_plugin::<()>(
                    "syncState",
                    SyncStateArgs {
                        state,
                        outcome,
                        status_code,
                        notify_completion,
                    },
                )
                .map_err(|error| DomainError::InternalError(format!("{context}: {error}")))
        }
    }

    impl<R: Runtime> GenerationBackgroundRuntime for AndroidGenerationBackgroundRuntime<R> {
        fn start(&self, task_id: &str, _user_visible: bool) -> Result<(), DomainError> {
            let mut tracker = self
                .tracker
                .lock()
                .expect("Android keep-alive tracker mutex poisoned");
            let state = tracker.start_task(task_id);
            self.sync_state(
                state,
                None,
                "Failed to start Android generation foreground service",
            )
        }

        fn finish(
            &self,
            task_id: &str,
            outcome: GenerationBackgroundOutcome,
            notify_completion: bool,
        ) -> Result<(), DomainError> {
            let mut tracker = self
                .tracker
                .lock()
                .expect("Android keep-alive tracker mutex poisoned");
            let (state, last_generation_finished) = tracker.finish_task(task_id);
            if !last_generation_finished {
                return Ok(());
            }
            self.sync_state(
                state,
                Some((outcome, notify_completion)),
                "Failed to finish Android generation foreground service",
            )
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

#[cfg(test)]
mod tests {
    use super::{KeepAliveState, KeepAliveTracker};

    enum Step {
        Start(&'static str),
        Finish(&'static str, bool),
        SetPersistent(bool, bool),
    }

    #[test]
    fn keep_alive_tracker_computes_expected_state_across_transitions() {
        let mut tracker = KeepAliveTracker::default();
        assert_eq!(
            tracker.state(),
            KeepAliveState {
                active: false,
                needs_cpu: false,
                persistent: false,
            }
        );

        let cases = [
            (
                Step::SetPersistent(false, false),
                KeepAliveState {
                    active: false,
                    needs_cpu: false,
                    persistent: false,
                },
            ),
            (
                Step::Start("task-1"),
                KeepAliveState {
                    active: true,
                    needs_cpu: true,
                    persistent: false,
                },
            ),
            (
                Step::Start("task-2"),
                KeepAliveState {
                    active: true,
                    needs_cpu: true,
                    persistent: false,
                },
            ),
            (
                Step::Finish("task-1", false),
                KeepAliveState {
                    active: true,
                    needs_cpu: true,
                    persistent: false,
                },
            ),
            (
                Step::SetPersistent(true, true),
                KeepAliveState {
                    active: true,
                    needs_cpu: true,
                    persistent: true,
                },
            ),
            (
                Step::Finish("task-2", true),
                KeepAliveState {
                    active: true,
                    needs_cpu: false,
                    persistent: true,
                },
            ),
            (
                Step::SetPersistent(false, true),
                KeepAliveState {
                    active: false,
                    needs_cpu: false,
                    persistent: false,
                },
            ),
            (
                Step::SetPersistent(true, true),
                KeepAliveState {
                    active: true,
                    needs_cpu: false,
                    persistent: true,
                },
            ),
            (
                Step::Start("task-3"),
                KeepAliveState {
                    active: true,
                    needs_cpu: true,
                    persistent: true,
                },
            ),
            (
                Step::SetPersistent(false, true),
                KeepAliveState {
                    active: true,
                    needs_cpu: true,
                    persistent: false,
                },
            ),
            (
                Step::Finish("task-3", true),
                KeepAliveState {
                    active: false,
                    needs_cpu: false,
                    persistent: false,
                },
            ),
        ];

        for (index, (step, expected)) in cases.into_iter().enumerate() {
            let actual = match step {
                Step::Start(task_id) => tracker.start_task(task_id),
                Step::Finish(task_id, expected_last_finished) => {
                    let (state, last_finished) = tracker.finish_task(task_id);
                    assert_eq!(
                        last_finished, expected_last_finished,
                        "step {index}: unexpected last_finished flag"
                    );
                    state
                }
                Step::SetPersistent(persistent, expected_changed) => {
                    let (state, changed) = tracker.set_persistent(persistent);
                    assert_eq!(
                        changed, expected_changed,
                        "step {index}: unexpected changed flag"
                    );
                    state
                }
            };
            assert_eq!(actual, expected, "step {index}: unexpected KeepAliveState");
        }
    }
}
