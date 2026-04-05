// TriviumDB 多命名空间实例管理器
// 为扩展插件提供向量检索 + 图谱存储能力，每个命名空间对应一个独立的 .tdb 文件。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use triviumdb::Database;

/// TriviumDB 实例管理器
///
/// 关键设计原则：
/// - 每个扩展 namespace 对应一个独立的 .tdb 文件，数据完全隔离
/// - 所有数据库实例在应用生命周期内保持打开状态，避免重复 I/O
/// - 线程安全：内部使用 Arc<Mutex<>> 保护并发访问
pub struct TriviumStoreManager {
    /// 数据库存储根目录（通常在 data_root/_trivium/ 下）
    store_root: PathBuf,
    /// 已打开的数据库实例缓存（按 namespace 索引）
    instances: Mutex<HashMap<String, Arc<Mutex<Database<f32>>>>>,
}

impl TriviumStoreManager {
    /// 从 TT 的 data_root 创建管理器
    ///
    /// 数据库文件将存储在 `{data_root}/default-user/_trivium/` 目录中
    pub fn new(data_root: &Path) -> Self {
        let store_root = data_root.join("default-user").join("_trivium");
        Self {
            store_root,
            instances: Mutex::new(HashMap::new()),
        }
    }

    /// 校验命名空间合法性（防止路径穿越攻击）
    fn validate_namespace(namespace: &str) -> Result<(), String> {
        if namespace.is_empty() {
            return Err("命名空间不能为空".to_string());
        }
        if namespace.len() > 128 {
            return Err("命名空间长度不能超过 128 字符".to_string());
        }
        // 只允许字母、数字、短划线和下划线
        if !namespace
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(
                "命名空间只允许包含字母、数字、短划线和下划线".to_string(),
            );
        }
        // 禁止路径穿越
        if namespace.contains("..") || namespace.contains('/') || namespace.contains('\\') {
            return Err("命名空间包含非法路径字符".to_string());
        }
        Ok(())
    }

    /// 获取或打开指定命名空间的数据库实例
    ///
    /// 如果实例已存在则直接返回缓存，否则创建新数据库文件并缓存。
    /// `dim` 参数仅在首次创建时生效，后续打开同一个 namespace 会忽略此参数。
    pub fn open_or_get(
        &self,
        namespace: &str,
        dim: usize,
    ) -> Result<Arc<Mutex<Database<f32>>>, String> {
        Self::validate_namespace(namespace)?;

        let mut instances = self
            .instances
            .lock()
            .map_err(|e| format!("获取实例锁失败: {}", e))?;

        // 命中缓存时直接返回
        if let Some(db) = instances.get(namespace) {
            return Ok(Arc::clone(db));
        }

        // 确保目录存在
        std::fs::create_dir_all(&self.store_root)
            .map_err(|e| format!("创建存储目录失败: {}", e))?;

        let db_path = self
            .store_root
            .join(format!("{}.tdb", namespace));
        let db_path_str = db_path
            .to_str()
            .ok_or_else(|| "数据库路径包含非UTF-8字符".to_string())?;

        tracing::info!(
            "正在打开 TriviumDB 实例: namespace={}, dim={}, path={}",
            namespace,
            dim,
            db_path_str
        );

        let mut db = Database::<f32>::open(db_path_str, dim)
            .map_err(|e| format!("打开 TriviumDB 失败 [{}]: {}", namespace, e))?;

        // 启用后台自动压实（每 300 秒），减轻 WAL 文件增长
        db.enable_auto_compaction(Duration::from_secs(300));

        let db = Arc::new(Mutex::new(db));
        instances.insert(namespace.to_string(), Arc::clone(&db));

        tracing::info!(
            "TriviumDB 实例已就绪: namespace={}",
            namespace
        );

        Ok(db)
    }

    /// 手动持久化指定命名空间的数据库
    pub fn flush(&self, namespace: &str) -> Result<(), String> {
        let instances = self
            .instances
            .lock()
            .map_err(|e| format!("获取实例锁失败: {}", e))?;

        if let Some(db) = instances.get(namespace) {
            let mut db = db
                .lock()
                .map_err(|e| format!("获取数据库锁失败: {}", e))?;
            db.flush()
                .map_err(|e| format!("持久化失败 [{}]: {}", namespace, e))?;
        }
        Ok(())
    }

    /// 持久化所有已打开的数据库实例
    pub fn flush_all(&self) -> Result<(), String> {
        let instances = self
            .instances
            .lock()
            .map_err(|e| format!("获取实例锁失败: {}", e))?;

        for (namespace, db) in instances.iter() {
            let mut db = db
                .lock()
                .map_err(|e| format!("获取数据库锁失败 [{}]: {}", namespace, e))?;
            if let Err(e) = db.flush() {
                tracing::error!("持久化 TriviumDB 失败 [{}]: {}", namespace, e);
            }
        }
        Ok(())
    }

    /// 关闭指定命名空间的数据库实例
    pub fn close(&self, namespace: &str) -> Result<(), String> {
        // 先 flush 再移除引用
        self.flush(namespace)?;

        let mut instances = self
            .instances
            .lock()
            .map_err(|e| format!("获取实例锁失败: {}", e))?;
        instances.remove(namespace);

        tracing::info!("TriviumDB 实例已关闭: namespace={}", namespace);
        Ok(())
    }

    /// 列出所有已打开的命名空间
    pub fn list_namespaces(&self) -> Result<Vec<String>, String> {
        let instances = self
            .instances
            .lock()
            .map_err(|e| format!("获取实例锁失败: {}", e))?;
        Ok(instances.keys().cloned().collect())
    }
}
