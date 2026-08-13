# QuickJs 脚本执行引擎使用指南

## 概述

`tt-adapter-quickjs` 是一个基于 QuickJs 的 JavaScript 脚本执行引擎，专为 Tauri Tavern 的技能系统设计。它提供了安全的沙箱环境，允许技能通过脚本来扩展功能。

## 安全特性

### 1. 文件系统访问限制
- 脚本只能读写 **Agent 工作目录**内的文件
- 所有路径都会经过验证，防止目录遍历攻击
- 提供 `$fs` API 进行安全的文件操作

### 2. 模块加载限制
- `require()` 只能从以下目录加载模块：
  - 公共库目录 (`public_lib_dirs`)
  - 各个技能的 scripts 目录 (`skill_script_dirs`)
- 禁止加载任意路径的模块

### 3. 资源限制
- 内存限制：32MB
- 栈大小限制：256KB

## 可用的 API

### $fs - 文件系统 API

```javascript
// 读取文件内容 (路径相对于工作目录)
const content = await $fs.readText("data/config.json");

// 写入文件内容
await $fs.writeText("logs/output.txt", "Hello World");

// 列出目录内容
const files = await $fs.listFiles("data");
// 返回: [{ path: "data/file1.txt", kind: "file" }, ...]

// 检查文件是否存在
const exists = await $fs.exists("config.json");
```

### $worldInfo - 世界书 API

```javascript
// 读取所有激活的世界书条目
const result = await $worldInfo.readActivated();
// 返回: { entries: [{ uid, ref, content, constant, position, displayName, world }, ...] }

// 读取特定的世界书条目
const result = await $worldInfo.readEntries(["worldinfo:lore#1", "worldinfo:chars#2"]);
```

### $log - 日志 API

```javascript
$log.info("Information message", arg1, arg2);
$log.warn("Warning message");
$log.error("Error message");
$log.debug("Debug message");
```

## 脚本格式

技能脚本应该导出一个默认函数或 `main` 函数：

```javascript
// 方式 1: 导出默认函数
export default function(args) {
    // args 是调用时传入的参数对象
    const { input, options } = args;
    
    // 执行业务逻辑
    const result = processInput(input);
    
    // 返回结果
    return {
        success: true,
        data: result
    };
}

// 方式 2: 导出 main 函数
export function main(args) {
    return { result: "processed" };
}
```

## 创建技能脚本工具

### 1. 定义脚本工具描述符

```rust
use tt_adapter_quickjs::ScriptToolDescriptor;

let descriptor = ScriptToolDescriptor::create(
    "my-skill",           // 技能 ID
    "helper",             // 脚本名称 (对应 helper.js)
    Some("Helper Tool".to_string()),
    Some("A helper tool for processing data".to_string()),
    Some(json!({
        "type": "object",
        "properties": {
            "input": {"type": "string", "description": "Input data"}
        },
        "required": ["input"]
    })),
)?;
```

### 2. 初始化引擎和执行器

```rust
use std::sync::Arc;
use tt_adapter_quickjs::{QuickJsEngine, SandboxConfig, ScriptToolExecutor};

// 配置沙箱
let sandbox = SandboxConfig::new(
    work_dir,              // Agent 工作目录
    vec![public_lib_dir],  // 公共库目录
    vec![skill_scripts_dir], // 技能脚本目录
);

// 创建引擎
let engine = Arc::new(QuickJsEngine::new(sandbox)?);

// 创建执行器
let executor = ScriptToolExecutor::new(engine, work_dir);
```

### 3. 执行脚本

```rust
// 准备世界书数据 (从现有的 read_activated 结果解析)
let world_info_entries: Vec<ActivatedWorldInfoEntry> = activated_entries
    .iter()
    .enumerate()
    .filter_map(|(i, v)| ActivatedWorldInfoEntry::from_value(i, v))
    .collect();

// 执行脚本
let result = executor.execute(
    "my-skill",                    // 技能 ID
    "helper",                      // 脚本名称
    &json!({"input": "data"}),     // 参数
    world_info_entries,            // 激活的世界书条目
).await;

if result.success {
    println!("Result: {:?}", result.structured);
} else {
    eprintln!("Error: {}", result.content);
}
```

## 示例脚本

### 数据处理脚本

```javascript
// skills/my-skill/scripts/process.js

export default function(args) {
    const { text, format } = args;
    
    $log.info("Processing text", text);
    
    // 读取配置文件
    const config = JSON.parse(await $fs.readText("config/settings.json"));
    
    // 处理文本
    let result = text.trim();
    if (format === "uppercase") {
        result = result.toUpperCase();
    }
    
    // 写入结果
    await $fs.writeText("output/result.txt", result);
    
    // 访问世界书
    const worldData = await $worldInfo.readActivated();
    const loreEntries = worldData.entries.filter(e => e.constant);
    
    return {
        processed: result,
        loreCount: loreEntries.length,
        config: config
    };
}
```

### 模块导入示例

```javascript
// skills/my-skill/scripts/main.js

// 从公共库导入
const utils = require("public_lib/utils.js");

// 从同技能的其他脚本导入
const helper = require("./helper.js");

export default function(args) {
    const result = utils.process(args.data);
    return helper.format(result);
}
```

## 目录结构

```
work_dir/
├── skills/
│   └── my-skill/
│       └── scripts/
│           ├── main.js
│           └── helper.js
├── public_lib/
│   └── utils.js
├── config/
│   └── settings.json
└── output/
    └── result.txt
```

## 最佳实践

1. **错误处理**: 始终在脚本中捕获和处理异常
2. **异步操作**: 所有 API 都是异步的，使用 `await`
3. **路径安全**: 不要尝试访问工作目录外的路径
4. **模块化**: 将复杂逻辑拆分为多个模块
5. **日志记录**: 使用 `$log` API 记录重要信息以便调试

## 注意事项

- 脚本执行是同步的，避免长时间运行的操作
- 不要在脚本中使用 `eval()` 或动态代码执行
- 确保脚本文件使用 UTF-8 编码
- 模块路径使用正斜杠 `/` 分隔符
