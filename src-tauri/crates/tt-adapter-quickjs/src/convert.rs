//! serde_json::Value → rquickjs 值转换（仅覆盖 JSON 可表达类型）。
//!
//! 反向转换（JS → JSON）已由引擎内 JS 侧 `JSON.stringify` 承担，
//! 不再需要 Rust 侧的 `js_to_json`。

use rquickjs::{Ctx, Value as JsValue};
use serde_json::Value as JsonValue;

pub(crate) fn json_to_js<'js>(
    ctx: &Ctx<'js>,
    value: &JsonValue,
) -> rquickjs::Result<JsValue<'js>> {
    match value {
        JsonValue::Null => Ok(JsValue::new_null(ctx.clone())),
        JsonValue::Bool(value) => Ok(JsValue::new_bool(ctx.clone(), *value)),
        JsonValue::Number(value) => {
            if let Some(float) = value.as_f64() {
                Ok(JsValue::new_number(ctx.clone(), float))
            } else {
                Ok(JsValue::new_number(
                    ctx.clone(),
                    value.to_string().parse().unwrap_or(0.0),
                ))
            }
        }
        JsonValue::String(value) => Ok(rquickjs::String::from_str(ctx.clone(), value)?.into_value()),
        JsonValue::Array(items) => {
            let array = rquickjs::Array::new(ctx.clone())?;
            for (index, item) in items.iter().enumerate() {
                array.set(index, json_to_js(ctx, item)?)?;
            }
            Ok(array.into_value())
        }
        JsonValue::Object(fields) => {
            let object = rquickjs::Object::new(ctx.clone())?;
            for (key, field) in fields.iter() {
                object.set(key.as_str(), json_to_js(ctx, field)?)?;
            }
            Ok(object.into_value())
        }
    }
}
