//! 预置公共 skill 脚本库（单文件 ESM），编译期内嵌进二进制。
//! 通过 rquickjs `BuiltinLoader` 注册到运行时，脚本经带命名空间的
//! 模块名导入，避免与 skill 自带模块或未来的 npm 解析冲突。

/// 内置库的命名空间前缀。
pub const BUILTIN_MODULE_PREFIX: &str = "@tauritavern/runtime/";

/// 返回所有内嵌公共库的 `(模块名, 源码)` 列表。
pub fn builtin_modules() -> Vec<(&'static str, &'static str)> {
    vec![
        ("@tauritavern/runtime/dayjs", include_str!("../resources/skill-libs/dayjs.js")),
        ("@tauritavern/runtime/es-toolkit", include_str!("../resources/skill-libs/es-toolkit.js")),
        ("@tauritavern/runtime/fast-xml-parser", include_str!("../resources/skill-libs/fast-xml-parser.js")),
        ("@tauritavern/runtime/marked", include_str!("../resources/skill-libs/marked.js")),
        ("@tauritavern/runtime/papaparse", include_str!("../resources/skill-libs/papaparse.js")),
        ("@tauritavern/runtime/slugify", include_str!("../resources/skill-libs/slugify.js")),
    ]
}
