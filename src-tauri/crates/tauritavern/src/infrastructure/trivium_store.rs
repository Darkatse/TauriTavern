// TriviumDB 多命名空间实例管理器
// 为扩展插件提供向量检索、知识图谱与智能存储能力，每个命名空间对应一个独立的 .tdb 文件。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use triviumdb::Database;
use triviumdb::database::{Config, StorageMode};
use triviumdb::storage::wal::SyncMode;

/// 打开配置选项（对应 TriviumDB 0.8.5 高级特性矩阵）
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriviumOpenOptions {
    pub dim: Option<usize>,
    /// 存储引擎模式："mmap"（零拷贝极速热启动）| "rom"（便携单文件打包）
    pub storage_mode: Option<String>,
    /// WAL 预写日志安全级别："normal" | "full" | "off"（大批量导入时可关闭换取极致写入吞吐）
    pub sync_mode: Option<String>,
    /// 是否在打开时预加载全文索引（默认 false，按需加载以节约内存）
    pub load_text_index: Option<bool>,
    /// 是否允许在查询/数据规模满足时自动构建 QuIVer SOTA 向量图索引（默认 true）
    pub auto_build_quiver: Option<bool>,
    /// 内核内存预算（单位 MiB，0 表示不限制）
    pub memory_limit_mb: Option<usize>,
}

/// TriviumDB 实例管理器
///
/// 核心设计原则：
/// - 每个扩展命名空间（namespace）对应独立的 .tdb 文件，实现存储物理隔离
/// - 数据库实例在应用生命周期内常驻缓存，复用句柄与 QuIVer 索引加速
/// - 全懒加载机制：未被插件调用前，零内核常驻、零磁盘文件/目录产生
/// - 线程安全：外层 Arc + Mutex 保护并发安全访问
pub struct TriviumStoreManager {
    /// 数据库存储根目录（位于 {data_root}/default-user/_trivium/）
    store_root: PathBuf,
    /// 已打开的数据库实例缓存表（以 namespace 为键）
    instances: Mutex<HashMap<String, Arc<Mutex<Database<f32>>>>>,
}

impl TriviumStoreManager {
    /// 基于 TauriTavern 的数据根目录构造实例管理器
    pub fn new(data_root: &Path) -> Self {
        let store_root = data_root.join("default-user").join("_trivium");
        Self {
            store_root,
            instances: Mutex::new(HashMap::new()),
        }
    }

    /// 校验命名空间字符合法性，防御路径穿越
    pub fn validate_namespace(namespace: &str) -> Result<(), String> {
        if namespace.is_empty() {
            return Err("命名空间不能为空".to_string());
        }
        if namespace.len() > 128 {
            return Err("命名空间长度不能超过 128 字符".to_string());
        }
        if !namespace
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err("命名空间只允许包含英文字母、数字、短划线(-)和下划线(_)".to_string());
        }
        if namespace.contains("..") || namespace.contains('/') || namespace.contains('\\') {
            return Err("命名空间包含非法路径跳转字符".to_string());
        }
        Ok(())
    }

    /// 基础打开或获取已打开的数据库句柄
    pub fn open_or_get(
        &self,
        namespace: &str,
        dim: usize,
    ) -> Result<Arc<Mutex<Database<f32>>>, String> {
        self.open_or_get_with_options(
            namespace,
            TriviumOpenOptions {
                dim: Some(dim),
                ..Default::default()
            },
        )
    }

    /// 高级打开或获取数据库实例（支持 0.8.5 全部存储引擎与硬件配置）
    pub fn open_or_get_with_options(
        &self,
        namespace: &str,
        options: TriviumOpenOptions,
    ) -> Result<Arc<Mutex<Database<f32>>>, String> {
        Self::validate_namespace(namespace)?;

        let mut instances = self
            .instances
            .lock()
            .map_err(|_| "获取实例管理器互斥锁失败".to_string())?;

        if let Some(instance) = instances.get(namespace) {
            return Ok(Arc::clone(instance));
        }

        // 按需懒加载创建存储根目录
        if let Err(err) = std::fs::create_dir_all(&self.store_root) {
            tracing::warn!("创建 TriviumDB 根存储目录失败: {}", err);
        }

        let db_file_path = self.store_root.join(format!("{}.tdb", namespace));
        let db_path_str = db_file_path
            .to_str()
            .ok_or_else(|| "数据库路径包含非 UTF-8 字符".to_string())?;

        let dim = options.dim.unwrap_or(1536);
        let mut config = Config {
            dim,
            ..Default::default()
        };

        if let Some(ref sm) = options.storage_mode {
            if sm.eq_ignore_ascii_case("rom") {
                config.storage_mode = StorageMode::Rom;
            } else {
                config.storage_mode = StorageMode::Mmap;
            }
        }
        if let Some(ref sy) = options.sync_mode {
            config.sync_mode = match sy.to_lowercase().as_str() {
                "full" => SyncMode::Full,
                "off" => SyncMode::Off,
                _ => SyncMode::Normal,
            };
        }
        if let Some(lti) = options.load_text_index {
            config.load_text_index = lti;
        }
        if let Some(abq) = options.auto_build_quiver {
            config.auto_build_quiver = abq;
        }
        if let Some(mb) = options.memory_limit_mb {
            config.memory_limit = mb * 1024 * 1024;
        }

        tracing::info!(
            "正在以配置 [存储模式={:?}, 同步模式={:?}, QuIVer={}] 打开 TriviumDB: {}",
            config.storage_mode,
            config.sync_mode,
            config.auto_build_quiver,
            db_path_str
        );

        let db = Database::<f32>::open_with_config(db_path_str, config)
            .map_err(|e| format!("打开 TriviumDB 数据库失败: {}", e))?;

        let instance = Arc::new(Mutex::new(db));
        instances.insert(namespace.to_string(), Arc::clone(&instance));

        Ok(instance)
    }

    /// 持久化指定命名空间的全部未落盘数据
    pub fn flush(&self, namespace: &str) -> Result<(), String> {
        Self::validate_namespace(namespace)?;

        let instance = {
            let instances = self
                .instances
                .lock()
                .map_err(|_| "获取实例管理器互斥锁失败".to_string())?;
            instances.get(namespace).cloned()
        };

        if let Some(db_arc) = instance {
            let mut db = db_arc
                .lock()
                .map_err(|_| "获取数据库锁失败".to_string())?;
            db.flush()
                .map_err(|e| format!("刷新数据库数据落盘失败: {}", e))?;
        }

        Ok(())
    }

    /// 关闭并释放指定命名空间的数据库句柄
    pub fn close(&self, namespace: &str) -> Result<(), String> {
        Self::validate_namespace(namespace)?;

        let instance = {
            let mut instances = self
                .instances
                .lock()
                .map_err(|_| "获取实例管理器互斥锁失败".to_string())?;
            instances.remove(namespace)
        };

        if let Some(db_arc) = instance {
            let mut db = db_arc
                .lock()
                .map_err(|_| "获取数据库锁失败".to_string())?;
            db.close()
                .map_err(|e| format!("关闭数据库失败: {}", e))?;
        }

        Ok(())
    }

    /// 列出所有当前已打开的命名空间
    pub fn list_namespaces(&self) -> Result<Vec<String>, String> {
        let instances = self
            .instances
            .lock()
            .map_err(|_| "获取实例管理器互斥锁失败".to_string())?;
        Ok(instances.keys().cloned().collect())
    }

    /// 刷新所有处于活跃状态的数据库
    #[allow(dead_code)]
    pub fn flush_all(&self) {
        let instances = match self.instances.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => return,
        };

        for (ns, db_arc) in instances {
            if let Ok(mut db) = db_arc.lock()
                && let Err(e) = db.flush()
            {
                tracing::warn!("定时/自动刷盘命名空间 [{}] 失败: {}", ns, e);
            }
        }
    }
}
