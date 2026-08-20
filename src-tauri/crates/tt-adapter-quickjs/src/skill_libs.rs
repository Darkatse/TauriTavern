//! 预置第三方 skill 脚本库（单文件 ESM），编译期内嵌进二进制。
//! 通过 rquickjs `BuiltinLoader` 注册到运行时，脚本经带命名空间的
//! 模块名导入，避免与 skill 自带模块或未来的 npm 解析冲突。
//! 与 `@tauritavern/runtime/v1`（宿主 Runtime API）区分开。

/// 内置第三方库的命名空间前缀。
pub const BUILTIN_MODULE_PREFIX: &str = "@tauritavern/vendor/";

/// 返回所有内嵌公共库的 `(模块名, 源码)` 列表。
pub fn builtin_modules() -> Vec<(&'static str, &'static str)> {
    vec![
        ("@tauritavern/vendor/dayjs", include_str!("../resources/vendor/dayjs.js")),
        ("@tauritavern/vendor/es-toolkit", include_str!("../resources/vendor/es-toolkit.js")),
        ("@tauritavern/vendor/fast-xml-parser", include_str!("../resources/vendor/fast-xml-parser.js")),
        ("@tauritavern/vendor/marked", include_str!("../resources/vendor/marked.js")),
        ("@tauritavern/vendor/papaparse", include_str!("../resources/vendor/papaparse.js")),
        ("@tauritavern/vendor/slugify", include_str!("../resources/vendor/slugify.js")),
    ]
}
