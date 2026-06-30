use std::sync::Arc;

use tauri::AppHandle;

use crate::application::services::data_archive_service::{
    DataArchiveJobRegistry, DataArchiveService,
};
use crate::application::services::data_change_reconciler::DataChangeReconciler;
use crate::infrastructure::persistence::data_archive_adapters::{
    DataDirectoryDataRootInitializer, FileDataArchiveExecutor, TauriDataArchiveFileGateway,
};

pub(super) fn build(
    app_handle: &AppHandle,
    data_change_reconciler: Arc<dyn DataChangeReconciler>,
) -> Arc<DataArchiveService> {
    Arc::new(DataArchiveService::new(
        Arc::new(DataArchiveJobRegistry::new()),
        tauri::async_runtime::handle().inner().clone(),
        Arc::new(FileDataArchiveExecutor),
        Arc::new(TauriDataArchiveFileGateway::new(app_handle.clone())),
        Arc::new(DataDirectoryDataRootInitializer),
        data_change_reconciler,
    ))
}
