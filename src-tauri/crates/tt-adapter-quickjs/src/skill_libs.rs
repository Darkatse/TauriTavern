//! 预置公共 skill 脚本库（单文件 ESM），编译期内嵌进二进制。
//! 通过 rquickjs `BuiltinLoader` 注册到运行时，脚本可经裸模块名导入。

/// 返回所有内嵌公共库的 `(模块名, 源码)` 列表。
/// 模块名是裸导入名（不含 `.js` 后缀），与 `BuiltinLoader` 注册键一致。
pub fn builtin_modules() -> Vec<(&'static str, &'static str)> {
    vec![
        ("dayjs", include_str!("../../tauritavern/resources/skill-libs/dayjs.js")),
        ("es-toolkit", include_str!("../../tauritavern/resources/skill-libs/es-toolkit.js")),
        ("fast-xml-parser", include_str!("../../tauritavern/resources/skill-libs/fast-xml-parser.js")),
        ("marked", include_str!("../../tauritavern/resources/skill-libs/marked.js")),
        ("papaparse", include_str!("../../tauritavern/resources/skill-libs/papaparse.js")),
        ("slugify", include_str!("../../tauritavern/resources/skill-libs/slugify.js")),
    ]
}
