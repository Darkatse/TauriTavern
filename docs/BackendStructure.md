# TauriTavern 后端结构

本文档详细描述TauriTavern的Rust后端架构，包括模块组织、数据流和扩展指南。

## 1. 架构概述

TauriTavern的后端采用Clean Architecture架构，将代码组织为多个层次，每个层次有明确的职责和依赖方向。这种架构提供了良好的可测试性、可维护性和灵活性。

### 1.1 架构层次

```
┌─────────────────────────────────────────┐
│                                         │
│  Presentation Layer (Tauri Commands)    │
│                                         │
└───────────────────┬─────────────────────┘
                    │
                    ▼
┌─────────────────────────────────────────┐
│                                         │
│  Application Layer (Services)           │
│                                         │
└───────────────────┬─────────────────────┘
                    │
                    ▼
┌─────────────────────────────────────────┐
│                                         │
│  Domain Layer (Models, Repositories)    │
│                                         │
└───────────────────┬─────────────────────┘
                    │
                    ▼
┌─────────────────────────────────────────┐
│                                         │
│  Infrastructure Layer (Implementations) │
│                                         │
└─────────────────────────────────────────┘
```

### 1.2 依赖规则

- 外层可以依赖内层，但内层不能依赖外层
- 内层定义接口，外层实现接口
- 所有层次都可以使用领域模型和错误类型

## 2. 目录结构

```
src-tauri/
├── src/
│   ├── main.rs                # 应用入口点
│   ├── lib.rs                 # 库入口点
│   ├── app.rs                 # 应用状态与运行时启动编排
│   ├── app/
│   │   └── bootstrap.rs       # 仓库/服务装配（依赖构建）
│   ├── domain/                # 领域层
│   │   ├── models/            # 领域模型
│   │   ├── repositories/      # 仓库接口
│   │   └── errors.rs          # 领域错误
│   ├── application/           # 应用层
│   │   ├── services/          # 业务服务
│   │   └── dto/               # 数据传输对象
│   ├── infrastructure/        # 基础设施层
│   │   ├── persistence/       # 持久化实现
│   │   ├── repositories/      # 仓库实现
│   │   ├── apis/              # 外部API集成
│   │   └── logging/           # 日志系统
│   └── presentation/          # 表示层
│       ├── commands/          # Tauri命令
│       │   ├── helpers.rs     # 命令日志/错误映射公共工具
│       │   └── registry.rs    # 命令注册清单（invoke handler）
│       └── errors.rs          # 命令错误
├── Cargo.toml                 # Rust依赖配置
└── tauri.conf.json            # Tauri配置
```

## 3. 核心组件

### 3.1 领域层 (Domain)

领域层包含业务核心概念和规则，与技术实现细节无关。

#### 3.1.1 模型 (Models)

模型代表业务领域中的核心对象，如角色、聊天、用户等。

```rust
// 示例: 角色模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Character {
    pub id: String,
    pub name: String,
    pub description: String,
    pub personality: String,
    pub first_message: Option<String>,
    pub avatar_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

#### 3.1.2 仓库接口 (Repository Interfaces)

仓库接口定义了实体的持久化操作，但不指定具体实现。

```rust
// 示例: 角色仓库接口
#[async_trait]
pub trait CharacterRepository: Send + Sync {
    async fn find_by_id(&self, id: &str) -> Result<Option<Character>, DomainError>;
    async fn find_all(&self) -> Result<Vec<Character>, DomainError>;
    async fn save(&self, character: &Character) -> Result<(), DomainError>;
    async fn delete(&self, id: &str) -> Result<(), DomainError>;
}
```

#### 3.1.3 领域错误 (Domain Errors)

定义领域操作可能遇到的错误类型。

```rust
// 示例: 领域错误
#[derive(Error, Debug)]
pub enum DomainError {
    #[error("Entity not found: {0}")]
    NotFound(String),

    #[error("Invalid data: {0}")]
    InvalidData(String),

    #[error("Operation not permitted: {0}")]
    PermissionDenied(String),

    #[error("Authentication error: {0}")]
    AuthenticationError(String),

    #[error("Internal error: {0}")]
    InternalError(String),
}
```

### 3.2 应用层 (Application)

应用层协调领域对象完成用户用例，实现业务流程。

#### 3.2.1 服务 (Services)

服务封装特定用例的业务逻辑，协调多个领域对象。

当前与 AI 相关的新增服务包括：

- `ChatCompletionService`：封装 Chat Completion 状态检查与生成流程（当前支持 OpenAI / Claude / Gemini(MakerSuite) / Custom OpenAI-compatible）。
- `TokenizationService`：统一 token 计数、编码/解码、logit bias token 映射（基于 `tiktoken-rs`，非 OpenAI 模型先 fallback）。

`ChatCompletionService` 已按 provider 能力拆分为模块目录：

- `application/services/chat_completion_service/config.rs`：provider 配置与密钥解析（含 custom base URL / header 解析）。
- `application/services/chat_completion_service/payload/*`：按 provider 构建上游请求体。
- `application/services/chat_completion_service/custom_parameters.rs`：custom body/header 参数解析。

```rust
// 示例: 角色服务
pub struct CharacterService {
    repository: Arc<dyn CharacterRepository>,
}

impl CharacterService {
    pub fn new(repository: Arc<dyn CharacterRepository>) -> Self {
        Self { repository }
    }

    pub async fn get_character(&self, id: &str) -> Result<Option<Character>, DomainError> {
        self.repository.find_by_id(id).await
    }

    pub async fn create_character(&self, character: Character) -> Result<Character, DomainError> {
        // 验证角色数据
        self.validate_character(&character)?;

        // 保存角色
        self.repository.save(&character).await?;

        Ok(character)
    }

    // 其他方法...
}
```

#### 3.2.2 数据传输对象 (DTOs)

DTOs用于在应用层和表示层之间传输数据，隔离领域模型。

```rust
// 示例: 创建角色DTO
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateCharacterDto {
    pub name: String,
    pub description: String,
    pub personality: String,
    pub first_message: Option<String>,
    pub avatar_url: Option<String>,
}

// 示例: 角色响应DTO
#[derive(Debug, Serialize, Deserialize)]
pub struct CharacterResponseDto {
    pub id: String,
    pub name: String,
    pub description: String,
    pub personality: String,
    pub first_message: Option<String>,
    pub avatar_url: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
```

### 3.3 基础设施层 (Infrastructure)

基础设施层提供技术实现，如数据库访问、外部API集成等。

在 AI 场景下，基础设施层新增了两类实现：

- `HttpChatCompletionRepository`：统一封装 OpenAI / Claude / Gemini(MakerSuite) / Custom OpenAI-compatible 的 HTTP 调用与响应规范化。
- `TiktokenTokenizerRepository`：负责 tokenizer 计数与编解码的底层实现。

`HttpChatCompletionRepository` 同样按 provider 拆分为模块目录：

- `infrastructure/apis/http_chat_completion_repository/openai.rs`
- `infrastructure/apis/http_chat_completion_repository/claude.rs`
- `infrastructure/apis/http_chat_completion_repository/makersuite.rs`
- `infrastructure/apis/http_chat_completion_repository/normalizers.rs`

#### 3.3.1 仓库实现 (Repository Implementations)

实现领域层定义的仓库接口，提供具体的持久化逻辑。

扩展仓库 `FileExtensionRepository` 已按职责拆分为子模块（如 GitHub 快照访问、元数据解析、临时目录与替换策略），主仓储仅保留装配与用例编排逻辑，以降低单文件复杂度并提升可测试性。

```rust
// 示例: 文件系统角色仓库
pub struct FileCharacterRepository {
    directory: PathBuf,
}

#[async_trait]
impl CharacterRepository for FileCharacterRepository {
    async fn find_by_id(&self, id: &str) -> Result<Option<Character>, DomainError> {
        let file_path = self.directory.join(format!("{}.json", id));

        if !file_path.exists() {
            return Ok(None);
        }

        match read_json_file::<Character>(&file_path) {
            Ok(character) => Ok(Some(character)),
            Err(e) => Err(DomainError::Repository(e.to_string())),
        }
    }

    // 其他方法实现...
}
```

#### 3.3.2 持久化工具 (Persistence Utilities)

提供通用的文件系统操作和数据序列化功能。

```rust
// 示例: 文件系统工具
pub fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<T, DomainError> {
    let file = File::open(path)
        .map_err(|e| DomainError::Repository(format!("Failed to open file: {}", e)))?;

    let reader = BufReader::new(file);
    serde_json::from_reader(reader)
        .map_err(|e| DomainError::Repository(format!("Failed to parse JSON: {}", e)))
}

pub fn write_json_file<T: Serialize>(path: &Path, data: &T) -> Result<(), DomainError> {
    let file = File::create(path)
        .map_err(|e| DomainError::Repository(format!("Failed to create file: {}", e)))?;

    let writer = BufWriter::new(file);
    serde_json::to_writer_pretty(writer, data)
        .map_err(|e| DomainError::Repository(format!("Failed to write JSON: {}", e)))
}
```

#### 3.3.3 日志系统 (Logging)

提供统一的日志记录功能。

```rust
// 示例: 日志模块
pub fn init() {
    env_logger::init();
}

pub fn debug(message: &str) {
    log::debug!("{}", message);
}

pub fn info(message: &str) {
    log::info!("{}", message);
}

pub fn warn(message: &str) {
    log::warn!("{}", message);
}

pub fn error(message: &str) {
    log::error!("{}", message);
}
```

### 3.4 表示层 (Presentation)

表示层负责处理用户交互，将请求转发给应用层，并将结果返回给用户。

#### 3.4.1 Tauri命令 (Commands)

Tauri命令是前端与后端通信的桥梁，通过IPC机制暴露给前端。

```rust
// 示例: 角色命令（使用公共 helper）
use crate::presentation::commands::helpers::{log_command, map_command_error};

#[tauri::command]
pub async fn get_all_characters(
    shallow: bool,
    app_state: State<'_, Arc<AppState>>,
) -> Result<Vec<CharacterDto>, CommandError> {
    log_command(format!("get_all_characters (shallow: {})", shallow));

    app_state
        .character_service
        .get_all_characters(shallow)
        .await
        .map_err(map_command_error("Failed to get all characters"))
}

#[tauri::command]
pub async fn create_character(
    dto: CreateCharacterDto,
    app_state: State<'_, Arc<AppState>>,
) -> Result<CharacterDto, CommandError> {
    log_command(format!("create_character {}", dto.name));

    app_state
        .character_service
        .create_character(dto)
        .await
        .map_err(map_command_error("Failed to create character"))
}
```

#### 3.4.2 命令错误 (Command Errors)

定义命令执行过程中可能遇到的错误，并提供适当的错误处理。

```rust
// 示例: 命令错误
#[derive(Debug, Error, Serialize)]
pub enum CommandError {
    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Internal server error: {0}")]
    InternalServerError(String),
}

// 从领域错误转换为命令错误
impl From<DomainError> for CommandError {
    fn from(error: DomainError) -> Self {
        match error {
            DomainError::NotFound(msg) => CommandError::NotFound(msg),
            DomainError::InvalidData(msg) => CommandError::BadRequest(msg),
            DomainError::PermissionDenied(msg) => CommandError::Unauthorized(msg),
            DomainError::AuthenticationError(msg) => CommandError::Unauthorized(msg),
            DomainError::InternalError(msg) => CommandError::InternalServerError(msg),
        }
    }
}
```

#### 3.4.3 命令注册与解耦

当前实现将命令注册集中在 `presentation/commands/registry.rs`，由 `lib.rs` 统一挂载：

```rust
// lib.rs
use presentation::commands::registry::invoke_handler;

tauri::Builder::default()
    // ...
    .invoke_handler(invoke_handler())
```

这样可以避免在 `lib.rs` 中直接维护超长命令列表，命令增减时只需更新 `registry.rs`。

### 3.5 聊天后端链路（Payload-First + Byte Transport，2026-02重构）

本次聊天链路重构目标是对齐 SillyTavern 上游业务逻辑与文件系统语义，同时降低字段漂移/数据丢失风险。

#### 3.5.1 设计原则

- **Payload First**：聊天保存/读取优先以 JSONL 原始 payload 为边界，而非先强制转领域模型再落盘。
- **Control/Data Plane 分离**：小参数走 JSON invoke（control plane），大 payload 走 raw bytes / 文件路径（data plane）。
- **字段保真**：`ChatMetadata`、`ChatMessage`、`MessageExtra` 通过 `#[serde(flatten)] additional` 保留未知字段。
- **完整性优先**：保存链路内置 `chat_metadata.integrity` 校验；冲突时返回 `integrity`，由前端决定是否 `force` 覆盖。
- **兼容上游文件系统**：目录与命名策略对齐 SillyTavern，便于用户无损迁移。
- **移动端优先规避峰值内存**：Android 读走 `asset://` 路径，写走“前端临时文件 -> 后端按路径原子落盘”。

#### 3.5.2 分层职责

- `domain/repositories/chat_repository.rs`
  - 保留 typed API（`get_chat` / `save` / `search_chats` 等）用于领域操作。
  - payload API 扩展为三类：对象数组（`*_payload`）、字节流（`*_payload_bytes`）、路径/文件直通（`*_payload_path`、`*_payload_from_path`）。
- `application/services/chat_service.rs`
  - `save_chat` 直接调用 payload 写接口，不做 typed -> payload 二次构建。
  - 新增 character/group 的 bytes、path、from-file 服务方法；`save_*_from_file` 走仓库路径直通，避免整文件读入内存。
- `presentation/commands/chat_commands.rs`
  - 保留 JSON payload 命令：`get_chat_payload` / `save_chat` / `get_group_chat` / `save_group_chat` 等。
  - 新增 raw/path/from-file 命令：`get_chat_payload_raw`、`get_chat_payload_path`、`save_chat_payload_raw`、`save_chat_payload_from_file`、`get_group_chat_raw`、`get_group_chat_path`、`save_group_chat_raw`、`save_group_chat_from_file`。
  - raw 保存命令通过 header 传递元信息；后端对 header 值执行 percent decode，兼容非 ASCII 名称。
- `infrastructure/repositories/file_chat_repository/`
  - `repository_impl.rs`：统一编排 typed/payload/bytes/path 写入入口。
  - `payload.rs`：完整性校验、字节写入、文件路径直通写入（原子替换 + 备份）。
  - `jsonl_utils.rs`：`parse_jsonl_bytes`、`write_jsonl_bytes_file`、`read_first_non_empty_jsonl_line` 等基础能力。
  - 完整性校验改为“首个非空行”解析，避免为校验反序列化整个文件。

#### 3.5.3 关键链路

1. 桌面/iOS 角色聊天读取（高性能路径）  
前端 `invoke(get_chat_payload_raw)` -> 命令返回 bytes -> 前端按 JSONL 解析。

2. Android 角色聊天读取  
前端 `invoke(get_chat_payload_path)` -> `convertFileSrc(..., 'asset')` -> `fetch(asset://...)`。

3. 桌面/iOS 角色聊天保存（高性能路径）  
前端 raw bytes + headers -> `save_chat_payload_raw` -> `ChatService::save_chat_payload_bytes` -> 仓库原子写入 + 备份。

4. Android 角色聊天保存  
前端先落临时 JSONL 文件 -> `save_chat_payload_from_file` -> `ChatService::save_chat_from_file` -> `save_chat_payload_from_path`。

5. 群聊读写链路  
与角色聊天完全对称：`get_group_chat_raw/get_group_chat_path/save_group_chat_raw/save_group_chat_from_file`。

6. 完整性冲突  
当现有文件的 `chat_metadata.integrity` 与待写入 payload 不一致且 `force=false` 时，仓库返回 `DomainError::InvalidData("integrity")`，前端映射为冲突提示并可二次 `force` 覆盖。

7. Header 编码约定（raw 保存）  
`x-character-name` / `x-file-name` / `x-chat-id` 在前端使用 URI 百分号编码（ASCII 安全），后端解码后再进入服务层。

#### 3.5.4 文件系统与备份语义

- 角色聊天：`default-user/chats/<character_id>/*.jsonl`
- 群聊：`default-user/group chats/*.jsonl`
- 聊天备份：`default-user/backups/chat_<sanitized_name>_<timestamp>.jsonl`
- 备份节流：10 秒（对齐上游默认）
- 单会话备份上限：50（对齐上游默认）
- 全局备份上限：无限（默认）

## 4. 应用状态管理

应用状态管理负责初始化和管理应用的全局状态，包括服务实例和配置。

### 4.1 AppState

`AppState`结构体包含应用的全局状态，如服务实例和数据目录。

```rust
// app.rs（示意）
mod bootstrap;

pub struct AppState {
    pub data_directory: DataDirectory,
    pub character_service: Arc<CharacterService>,
    pub chat_service: Arc<ChatService>,
    pub user_service: Arc<UserService>,
    pub settings_service: Arc<SettingsService>,
    pub user_directory_service: Arc<UserDirectoryService>,
    pub secret_service: Arc<SecretService>,
    pub content_service: Arc<ContentService>,
    pub extension_service: Arc<ExtensionService>,
    pub avatar_service: Arc<AvatarService>,
    pub group_service: Arc<GroupService>,
    pub background_service: Arc<BackgroundService>,
    pub theme_service: Arc<ThemeService>,
    pub preset_service: Arc<PresetService>,
}

impl AppState {
    pub async fn new(app_handle: AppHandle, data_root: &Path) -> Result<Self, DomainError> {
        // 初始化目录
        let data_directory = bootstrap::initialize_data_directory(data_root).await?;
        // 统一装配仓库与服务
        let services = bootstrap::build_services(&app_handle, &data_directory);

        Ok(Self {
            data_directory,
            character_service: services.character_service,
            chat_service: services.chat_service,
            user_service: services.user_service,
            settings_service: services.settings_service,
            user_directory_service: services.user_directory_service,
            secret_service: services.secret_service,
            content_service: services.content_service,
            extension_service: services.extension_service,
            avatar_service: services.avatar_service,
            group_service: services.group_service,
            background_service: services.background_service,
            theme_service: services.theme_service,
            preset_service: services.preset_service,
        })
    }
}
```

### 4.2 数据目录管理

`DataDirectory`负责管理应用的数据目录结构。

```rust
// 示例: 数据目录管理
pub struct DataDirectory {
    root: PathBuf,
    default_user: PathBuf,
}

impl DataDirectory {
    pub fn new(root: &Path) -> Result<Self, DomainError> {
        let default_user = root.join("default-user");

        // 创建目录结构
        Self::create_directory_structure(root, &default_user)?;

        Ok(Self {
            root: root.to_path_buf(),
            default_user: default_user.to_path_buf(),
        })
    }

    fn create_directory_structure(root: &Path, default_user: &Path) -> Result<(), DomainError> {
        // 创建根目录
        if !root.exists() {
            fs::create_dir_all(root).map_err(|e| {
                DomainError::InternalError(format!("Failed to create root directory: {}", e))
            })?;
        }

        // 创建默认用户目录
        if !default_user.exists() {
            fs::create_dir_all(default_user).map_err(|e| {
                DomainError::InternalError(format!("Failed to create default user directory: {}", e))
            })?;
        }

        // 创建默认用户子目录
        let default_user_dirs = [
            "characters",
            "chats",
            "backups",
            "User Avatars",
            "backgrounds",
            "thumbnails",
            "thumbnails/bg",
            "thumbnails/avatar",
            "worlds",
            "user",
            "user/images",
            "groups",
            "group chats",
            "NovelAI Settings",
            "KoboldAI Settings",
            "OpenAI Settings",
            "TextGen Settings",
            "themes",
            "movingUI",
            "extensions",
            "instruct",
            "context",
            "QuickReplies",
            "assets",
            "user/workflows",
            "user/files",
            "vectors",
            "sysprompt",
            "reasoning",
        ];

        for dir in default_user_dirs.iter() {
            let path = default_user.join(dir);
            if !path.exists() {
                fs::create_dir_all(&path).map_err(|e| {
                    DomainError::InternalError(format!("Failed to create directory {}: {}", dir, e))
                })?;
            }
        }

        Ok(())
    }

    // 获取各目录路径
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn default_user(&self) -> &Path {
        &self.default_user
    }

    pub fn characters(&self) -> PathBuf {
        self.default_user.join("characters")
    }

    pub fn chats(&self) -> PathBuf {
        self.default_user.join("chats")
    }

    pub fn backups(&self) -> PathBuf {
        self.default_user.join("backups")
    }

    pub fn settings(&self) -> PathBuf {
        self.default_user.clone()
    }

    pub fn users(&self) -> PathBuf {
        self.default_user.join("user")
    }

    // 其他目录访问方法...
}
```

## 5. 后端API

TauriTavern的后端API通过Tauri命令暴露给前端。以下是主要API类别：

### 5.1 角色管理API

| 命令 | 描述 | 参数 | 返回值 |
|------|------|------|--------|
| `get_characters` | 获取所有角色 | `shallow: bool` | `Vec<CharacterDto>` |
| `get_character` | 获取单个角色 | `name: String` | `CharacterDto` |
| `create_character` | 创建新角色 | `CreateCharacterDto` | `CharacterDto` |
| `update_character` | 更新角色 | `name: String, UpdateCharacterDto` | `CharacterDto` |
| `delete_character` | 删除角色 | `DeleteCharacterDto` | `()` |
| `import_character` | 导入角色 | `ImportCharacterDto` | `CharacterDto` |
| `export_character` | 导出角色 | `ExportCharacterDto` | `()` |
| `create_with_avatar` | 创建带头像的角色 | `CreateWithAvatarDto` | `CharacterDto` |
| `update_avatar` | 更新角色头像 | `UpdateAvatarDto` | `()` |
| `rename_character` | 重命名角色 | `RenameCharacterDto` | `CharacterDto` |

### 5.2 聊天管理API

| 命令 | 描述 | 参数 | 返回值 |
|------|------|------|--------|
| `get_all_chats` | 获取所有聊天 | 无 | `Vec<ChatDto>` |
| `get_character_chats` | 获取角色的聊天 | `character_name: String` | `Vec<ChatDto>` |
| `get_chat` | 获取单个聊天 | `character_name: String, file_name: String` | `ChatDto` |
| `get_chat_payload` | 获取角色聊天原始JSONL对象数组 | `character_name: String, file_name: String` | `Vec<Value>` |
| `get_chat_payload_raw` | 获取角色聊天原始JSONL字节流 | `characterName: String, fileName: String, allowNotFound?: bool` | `InvokeResponse(bytes)` |
| `get_chat_payload_path` | 获取角色聊天文件绝对路径 | `characterName: String, fileName: String` | `String` |
| `create_chat` | 创建新聊天 | `CreateChatDto` | `ChatDto` |
| `delete_chat` | 删除聊天 | `character_name: String, file_name: String` | `()` |
| `add_message` | 添加消息 | `AddMessageDto` | `ChatDto` |
| `rename_chat` | 重命名聊天 | `RenameChatDto` | `()` |
| `search_chats` | 搜索聊天 | `query: String, character_filter: Option<String>` | `Vec<ChatSearchResultDto>` |
| `import_chat` | 导入聊天（legacy typed） | `ImportChatDto` | `ChatDto` |
| `import_character_chats` | 导入角色聊天（payload链路） | `ImportCharacterChatsDto` | `Vec<String>` |
| `export_chat` | 导出聊天 | `ExportChatDto` | `()` |
| `save_chat` | 保存角色聊天payload（含 integrity/force） | `SaveChatDto` | `()` |
| `save_chat_payload_raw` | 以 raw bytes 保存角色聊天 | `body: bytes, headers: x-character-name/x-file-name/x-force` | `()` |
| `save_chat_payload_from_file` | 从现有JSONL文件路径保存角色聊天 | `SaveChatFromFileDto` | `()` |
| `backup_chat` | 触发聊天备份 | `character_name: String, file_name: String` | `()` |
| `clear_chat_cache` | 清理聊天缓存 | 无 | `()` |

### 5.2.1 群聊Payload命令

| 命令 | 描述 | 参数 | 返回值 |
|------|------|------|--------|
| `get_group_chat` | 获取群聊原始JSONL对象数组 | `GetGroupChatDto` | `Vec<Value>` |
| `get_group_chat_raw` | 获取群聊原始JSONL字节流 | `GetGroupChatDto, allowNotFound?: bool` | `InvokeResponse(bytes)` |
| `get_group_chat_path` | 获取群聊文件绝对路径 | `GetGroupChatDto` | `String` |
| `save_group_chat` | 保存群聊payload（含 integrity/force） | `SaveGroupChatDto` | `()` |
| `save_group_chat_raw` | 以 raw bytes 保存群聊 | `body: bytes, headers: x-chat-id/x-force` | `()` |
| `save_group_chat_from_file` | 从现有JSONL文件路径保存群聊 | `SaveGroupChatFromFileDto` | `()` |
| `delete_group_chat` | 删除群聊聊天文件 | `DeleteGroupChatDto` | `()` |
| `rename_group_chat` | 重命名群聊聊天文件 | `RenameGroupChatDto` | `()` |
| `import_group_chat_payload` | 导入群聊JSONL文件 | `ImportGroupChatDto` | `String` |

### 5.3 群组管理API

| 命令 | 描述 | 参数 | 返回值 |
|------|------|------|--------|
| `get_all_groups` | 获取所有群组 | 无 | `Vec<GroupDto>` |
| `get_group` | 获取单个群组 | `id: String` | `GroupDto` |
| `create_group` | 创建新群组 | `CreateGroupDto` | `GroupDto` |
| `update_group` | 更新群组 | `id: String, UpdateGroupDto` | `GroupDto` |
| `delete_group` | 删除群组 | `id: String` | `()` |

### 5.4 背景壁纸API

| 命令 | 描述 | 参数 | 返回值 |
|------|------|------|--------|
| `get_all_backgrounds` | 获取所有背景 | 无 | `Vec<String>` |
| `delete_background` | 删除背景 | `DeleteBackgroundDto` | `()` |
| `rename_background` | 重命名背景 | `RenameBackgroundDto` | `()` |
| `upload_background` | 上传背景 | `filename: String, data: Vec<u8>` | `String` |

### 5.5 主题管理API

| 命令 | 描述 | 参数 | 返回值 |
|------|------|------|--------|
| `save_theme` | 保存主题 | `SaveThemeDto` | `()` |
| `delete_theme` | 删除主题 | `DeleteThemeDto` | `()` |
| `get_all_themes` | 获取所有主题 | 无 | `Vec<ThemeDto>` |
| `get_theme` | 获取单个主题 | `name: String` | `ThemeDto` |

### 5.6 设置API

| 命令 | 描述 | 参数 | 返回值 |
|------|------|------|--------|
| `get_settings` | 获取设置 | 无 | `SettingsDto` |
| `update_settings` | 更新设置 | `UpdateSettingsDto` | `SettingsDto` |
| `reset_settings` | 重置设置 | 无 | `SettingsDto` |

### 5.7 密钥管理API

| 命令 | 描述 | 参数 | 返回值 |
|------|------|------|--------|
| `write_secret` | 写入密钥 | `WriteSecretDto` | `String` |
| `read_secret_state` | 读取密钥状态 | 无 | `HashMap<String, bool>` |
| `view_secrets` | 查看所有密钥 | 无 | `HashMap<String, String>` |
| `find_secret` | 查找特定密钥 | `FindSecretDto` | `SecretValueDto` |

### 5.8 系统API

| 命令 | 描述 | 参数 | 返回值 |
|------|------|------|--------|
| `get_version` | 获取版本 | 无 | `String` |
| `get_client_version` | 获取客户端版本 | 无 | `VersionInfo` |
| `is_ready` | 检查系统就绪状态 | 无 | `bool` |
| `emit_event` | 发送事件到前端 | `EmitEventDto` | `()` |

## 6. 错误处理

TauriTavern采用分层的错误处理策略，确保错误信息在传递过程中不丢失上下文。

### 6.1 错误类型层次

1. **领域错误 (DomainError)**: 业务规则违反、实体验证失败等
2. **基础设施错误 (InfrastructureError)**: 文件系统错误、网络错误等
3. **应用错误 (ApplicationError)**: 用例执行失败
4. **命令错误 (CommandError)**: 前端请求处理失败

### 6.2 错误转换

错误在层与层之间传递时进行转换，保留原始错误信息但适应当前层的上下文。

```rust
// 示例: 错误转换链
// 基础设施错误 -> 领域错误 -> 应用错误 -> 命令错误 -> 前端
```

### 6.3 错误日志

所有错误都应记录到日志系统，便于调试和问题排查。

```rust
// 示例: 错误日志记录
fn handle_error(error: &CommandError) {
    match error {
        CommandError::NotFound(msg) => logger::warn(&format!("Not found: {}", msg)),
        CommandError::BadRequest(msg) => logger::warn(&format!("Bad request: {}", msg)),
        CommandError::Forbidden(msg) => logger::warn(&format!("Forbidden: {}", msg)),
        CommandError::Internal(msg) => logger::error(&format!("Internal error: {}", msg)),
    }
}
```

## 7. 扩展指南

### 7.1 添加新模型

添加新模型时，应遵循以下步骤：

1. 在`domain/models`中定义模型结构
2. 在`domain/repositories`中定义仓库接口
3. 在`infrastructure/repositories`中实现仓库接口
4. 在`application/services`中创建服务
5. 在`application/dto`中定义数据传输对象
6. 在`presentation/commands`中添加命令
7. 在`app/bootstrap.rs`中注册仓库和服务构建逻辑，并在`app.rs`的`AppState`中挂载

### 7.2 添加新API

添加新API时，应遵循以下步骤：

1. 在`application/dto`中定义请求和响应DTO
2. 在`presentation/commands`中添加命令函数
3. 在`presentation/commands/registry.rs`中注册命令
4. 更新前端`tauri-bridge.js`和相关API文件

### 7.3 集成外部服务

集成外部服务时，应遵循以下步骤：

1. 在`infrastructure/apis`中创建服务客户端
2. 在`application/services`中创建服务适配器
3. 在`app/bootstrap.rs`中初始化服务装配
4. 在`presentation/commands`中暴露API

### 7.4 使用Tauri资源系统

在TauriTavern中，我们使用Tauri的资源系统进行文件访问，确保跨平台兼容性：

1. 使用`resolveResource`解析资源路径
2. 使用Tauri的文件系统API进行文件操作
3. 在仓库实现中统一使用资源路径

```rust
// 示例: 使用Tauri资源系统访问文件
pub async fn read_file(path: &str) -> Result<String, DomainError> {
    // 解析资源路径
    let resource_path = resolve_resource(path)
        .await
        .map_err(|e| DomainError::Repository(format!("Failed to resolve resource: {}", e)))?;

    // 读取文件内容
    let content = read_text_file(&resource_path)
        .await
        .map_err(|e| DomainError::Repository(format!("Failed to read file: {}", e)))?;

    Ok(content)
}
```

### 7.5 聊天链路持续维护约束

为避免聊天文件腐化或字段丢失，后续维护需遵循以下约束：

1. **新增聊天字段时优先走透传**  
若字段仅用于前端/上游兼容且不参与领域规则，优先落在 `flatten additional`，避免硬编码映射。

2. **写路径优先使用 payload API**  
涉及文件级同步（导入、恢复、迁移、group chat、批量操作）时，优先调用 `save_chat_payload` / `save_group_chat_payload`，避免 typed -> payload 重建造成信息丢失。

3. **不要绕过 integrity 逻辑**  
所有写入链路必须通过仓库统一入口，保留 `force` 开关和冲突信号 `integrity`。

4. **目录结构改动必须同步三处**  
- `DataDirectory`  
- `bootstrap` 仓库注入  
- 文档与前端路由约定（`chat-routes.js`）

5. **变更后至少覆盖以下测试面**  
- payload 字段保真（metadata/message/extra）  
- integrity 冲突与 force 覆盖  
- group chat 读写删改  
- 导入命名冲突去重

6. **角色导入不应提前写入初始 chat 文件**  
角色卡导入阶段只负责角色资产与角色数据落盘；首条消息与 swipe 结构由聊天链路在“首次打开会话”时生成，避免把 `alternate_greetings` 折损成单条 `mes`。

7. **raw 命令 header 必须保持 ASCII 安全**  
若 header 携带角色名/会话名/群聊 ID，前端必须先 URI 编码；后端统一解码，避免 WebView `Headers` 非 Latin-1 异常。

8. **移动端大文件写入优先走 from-file**  
Android 及低内存场景下，避免把大 JSONL 一次性塞入 IPC body，优先走临时文件 + `save_*_from_file`。

## 8. 测试策略

### 8.1 单元测试

每个模块应有对应的单元测试，特别是领域和应用层。

```rust
// 示例: 服务单元测试
#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::repositories::mock_character_repository::MockCharacterRepository;

    #[tokio::test]
    async fn test_get_character_by_id() {
        // 准备
        let repository = Arc::new(MockCharacterRepository::new());
        let service = CharacterService::new(repository.clone());

        let character = Character::new(
            "test-id".to_string(),
            "Test Character".to_string(),
            "Description".to_string(),
            "Personality".to_string(),
            None,
            None,
        );

        repository.save(&character).await.unwrap();

        // 执行
        let result = service.get_character("test-id").await.unwrap();

        // 验证
        assert!(result.is_some());
        let found = result.unwrap();
        assert_eq!(found.id, "test-id");
        assert_eq!(found.name, "Test Character");
    }
}
```

### 8.2 集成测试

集成测试验证多个组件的协作，特别是基础设施和应用层的交互。

```rust
// 示例: 集成测试
#[cfg(test)]
mod integration_tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_character_persistence() {
        // 准备临时目录
        let temp_dir = tempdir().unwrap();
        let repo = FileCharacterRepository::new(temp_dir.path().to_path_buf());
        let service = CharacterService::new(Arc::new(repo));

        // 创建角色
        let character = Character::new(
            "test-id".to_string(),
            "Test Character".to_string(),
            "Description".to_string(),
            "Personality".to_string(),
            None,
            None,
        );

        // 保存角色
        service.create_character(character.clone()).await.unwrap();

        // 读取角色
        let found = service.get_character("test-id").await.unwrap().unwrap();

        // 验证
        assert_eq!(found.id, character.id);
        assert_eq!(found.name, character.name);
    }
}
```

### 8.3 端到端测试

端到端测试验证整个系统的功能，从前端到后端。

```rust
// 示例: 端到端测试框架
#[cfg(test)]
mod e2e_tests {
    use tauri::test::{mock_builder, mock_context};

    #[test]
    fn test_character_creation() {
        // 创建测试应用
        let app = mock_builder()
            .build()
            .expect("Failed to build mock app");

        // 执行命令
        let result: Result<CharacterResponseDto, CommandError> = app
            .invoke_handler(tauri::generate_handler![create_character])
            .invoke("create_character", &CreateCharacterDto {
                name: "Test Character".to_string(),
                description: "Description".to_string(),
                personality: "Personality".to_string(),
                first_message: None,
                avatar_url: None,
            })
            .expect("Failed to invoke command");

        // 验证结果
        assert!(result.is_ok());
        let character = result.unwrap();
        assert_eq!(character.name, "Test Character");
    }
}
```

## 9. 性能考虑

### 9.1 异步处理

TauriTavern使用Tokio异步运行时，确保IO操作不阻塞主线程。

```rust
// 示例: 异步处理
#[tauri::command]
pub async fn process_large_file(
    app_state: State<'_, Arc<AppState>>,
    path: String,
) -> Result<ProcessingResultDto, CommandError> {
    // 异步处理文件
    app_state.file_service.process_file(&path).await
        .map(|result| ProcessingResultDto::from(result))
        .map_err(|e| CommandError::from(e))
}
```

### 9.2 异步初始化

在Tauri应用中，避免在setup钩子中使用block_on，因为它会阻塞主线程。相反，使用tauri::async_runtime::spawn进行异步初始化。

```rust
// 示例: 异步初始化
.setup(move |app| {
    // 获取AppHandle
    let app_handle = app.handle();

    // 获取应用数据目录
    let app_data_dir = app_handle.path().app_data_dir()
        .expect("Failed to get app data directory");

    // 构建数据根目录
    let data_root = app_data_dir.join("data");

    // 在异步任务中初始化AppState
    let app_handle_clone = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        // 初始化应用程序状态
        match AppState::new(app_handle_clone.clone(), &data_root).await {
            Ok(state) => {
                // 管理应用程序状态（整个AppState）
                app_handle_clone.manage(Arc::new(state));

                // 通知前端应用程序已准备就绪
                app_handle_clone.emit_all("app-ready", ())
                    .expect("Failed to emit app-ready event");
            },
            Err(e) => {
                logger::error(&format!("Failed to initialize application state: {}", e));
            }
        }
    });

    Ok(())
})
```

### 9.3 资源管理

合理管理内存和文件句柄，避免资源泄漏。

```rust
// 示例: 资源管理
pub async fn read_large_file(path: &Path) -> Result<String, DomainError> {
    // 使用作用域确保文件自动关闭
    let content = {
        let file = File::open(path)
            .map_err(|e| DomainError::Repository(e.to_string()))?;

        let mut reader = BufReader::new(file);
        let mut content = String::new();
        reader.read_to_string(&mut content)
            .map_err(|e| DomainError::Repository(e.to_string()))?;

        content
    }; // 文件在这里自动关闭

    Ok(content)
}
```

### 9.3 缓存策略

适当使用缓存减少重复计算和IO操作。

```rust
// 示例: 简单缓存
pub struct CachedRepository<T> {
    inner: Arc<dyn Repository<T>>,
    cache: Mutex<HashMap<String, (T, Instant)>>,
    ttl: Duration,
}

impl<T: Clone> CachedRepository<T> {
    pub fn new(inner: Arc<dyn Repository<T>>, ttl: Duration) -> Self {
        Self {
            inner,
            cache: Mutex::new(HashMap::new()),
            ttl,
        }
    }

    pub async fn get(&self, id: &str) -> Result<Option<T>, DomainError> {
        // 检查缓存
        {
            let cache = self.cache.lock().await;
            if let Some((value, timestamp)) = cache.get(id) {
                if timestamp.elapsed() < self.ttl {
                    return Ok(Some(value.clone()));
                }
            }
        }

        // 缓存未命中，从底层仓库获取
        let result = self.inner.get(id).await?;

        // 更新缓存
        if let Some(value) = &result {
            let mut cache = self.cache.lock().await;
            cache.insert(id.to_string(), (value.clone(), Instant::now()));
        }

        Ok(result)
    }
}
```
