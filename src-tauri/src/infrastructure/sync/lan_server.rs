use axum::{
    extract::{DefaultBodyLimit, Multipart, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use local_ip_address::local_ip;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use tracing::{error, info};

use crate::infrastructure::persistence::data_archive::{run_export_data_archive, run_import_data_archive};

use qrcode::QrCode;

/// 同步服务器状态
#[derive(Debug, Clone, serde::Serialize)]
pub struct SyncServerStatus {
    pub is_running: bool,
    pub address: Option<String>,
    pub port: u16,
}

/// 同步服务器管理器
pub struct LanSyncServer {
    status: Arc<Mutex<SyncServerStatus>>,
    stop_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    data_root: PathBuf,
}

impl LanSyncServer {
    pub fn new(data_root: PathBuf) -> Self {
        Self {
            status: Arc::new(Mutex::new(SyncServerStatus {
                is_running: false,
                address: None,
                port: 8080, // 默认端口
            })),
            stop_tx: Arc::new(Mutex::new(None)),
            data_root,
        }
    }

    /// 启动同步服务器
    pub async fn start(&self) -> Result<String, String> {
        let mut status = self.status.lock().await;
        if status.is_running {
            return Ok(status.address.clone().unwrap_or_default());
        }

        let ip = local_ip().map_err(|e| format!("获取本地 IP 失败: {}", e))?;
        let addr = SocketAddr::from((ip, status.port));
        let addr_str = format!("http://{}:{}", ip, status.port);

        let (tx, rx) = oneshot::channel();
        *self.stop_tx.lock().await = Some(tx);

        let status_clone = Arc::clone(&self.status);
        let data_root = self.data_root.clone();

        let app = Router::new()
            .route("/status", get(handle_status))
            .route("/download", get(handle_download))
            .route("/upload", post(handle_upload))
            .layer(CorsLayer::permissive())
            .layer(DefaultBodyLimit::max(1024 * 1024 * 1024)) // 限制 1GB
            .with_state(Arc::new(ServerState {
                data_root,
                status: status_clone,
            }));

        info!("正在启动局域网同步服务器: {}", addr_str);

        tokio::spawn(async move {
            let listener = match tokio::net::TcpListener::bind(addr).await {
                Ok(l) => l,
                Err(e) => {
                    error!("绑定端口失败: {}", e);
                    return;
                }
            };

            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    rx.await.ok();
                    info!("局域网同步服务器正在关闭...");
                })
                .await
                .unwrap();
        });

        status.is_running = true;
        status.address = Some(addr_str.clone());
        Ok(addr_str)
    }

    /// 停止同步服务器
    pub async fn stop(&self) {
        let mut stop_tx = self.stop_tx.lock().await;
        if let Some(tx) = stop_tx.take() {
            let _ = tx.send(());
        }
        let mut status = self.status.lock().await;
        status.is_running = false;
        status.address = None;
    }

    /// 获取服务器状态
    pub async fn get_status(&self) -> SyncServerStatus {
        self.status.lock().await.clone()
    }

    /// 生成二维码的 SVG 字符串
    pub fn generate_qr_code(&self, text: &str) -> Result<String, String> {
        let code = QrCode::new(text.as_bytes()).map_err(|e| format!("生成二维码失败: {}", e))?;
        let svg = code.render::<qrcode::render::svg::Color>()
            .min_dimensions(200, 200)
            .build();
        Ok(svg)
    }
}

struct ServerState {
    data_root: PathBuf,
    status: Arc<Mutex<SyncServerStatus>>,
}

async fn handle_status(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    let status = state.status.lock().await;
    axum::Json(status.clone())
}

async fn handle_download(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    info!("收到局域网同步下载请求");
    
    let temp_zip = std::env::temp_dir().join(format!("tauritavern_sync_{}.zip", uuid::Uuid::new_v4()));
    
    // 调用现有的导出逻辑
    let result = run_export_data_archive(
        &state.data_root,
        &temp_zip,
        &mut |_, _, _| {}, // 暂不报告进度
        &|| false,         // 不取消
    );

    match result {
        Ok(_) => {
            let file_content = match tokio::fs::read(&temp_zip).await {
                Ok(c) => c,
                Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("读取备份文件失败: {}", e)).into_response(),
            };
            
            // 删除临时文件
            let _ = tokio::fs::remove_file(&temp_zip).await;

            axum::response::Response::builder()
                .header("Content-Type", "application/zip")
                .header("Content-Disposition", "attachment; filename=\"tauritavern_backup.zip\"")
                .body(axum::body::Body::from(file_content))
                .unwrap()
                .into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("导出数据失败: {}", e)).into_response(),
    }
}

async fn handle_upload(
    State(state): State<Arc<ServerState>>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    info!("收到局域网同步上传请求");

    while let Some(field) = multipart.next_field().await.unwrap() {
        let name = field.name().unwrap().to_string();
        if name == "file" {
            let data = field.bytes().await.unwrap();
            let temp_zip = std::env::temp_dir().join(format!("tauritavern_upload_{}.zip", uuid::Uuid::new_v4()));
            
            if let Err(e) = tokio::fs::write(&temp_zip, data).await {
                return (StatusCode::INTERNAL_SERVER_ERROR, format!("保存上传文件失败: {}", e)).into_response();
            }

            // 调用现有的导入逻辑
            let workspace = state.data_root.parent().unwrap_or(&state.data_root).join("sync_workspace");
            let result = run_import_data_archive(
                &state.data_root,
                &temp_zip,
                &workspace,
                &mut |_, _, _| {},
                &|| false,
            );

            // 删除临时文件
            let _ = tokio::fs::remove_file(&temp_zip).await;

            return match result {
                Ok(_) => (StatusCode::OK, "数据同步导入成功！请重启应用以生效。").into_response(),
                Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("导入数据失败: {}", e)).into_response(),
            };
        }
    }

    (StatusCode::BAD_REQUEST, "未找到上传的文件").into_response()
}
