//! serde_json::Value 与 rquickjs 值互转（仅覆盖 JSON 可表达类型）。

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

/// 仅测试使用的 JS → JSON 转换（引擎返回值边界已改用 JS 侧 JSON.stringify）。
#[cfg(test)]
pub(crate) fn js_to_json<'js>(
    _ctx: &Ctx<'js>,
    value: &JsValue<'js>,
) -> rquickjs::Result<JsonValue> {
    if value.is_undefined() || value.is_null() {
        return Ok(JsonValue::Null);
    }
    if let Some(value) = value.as_bool() {
        return Ok(JsonValue::Bool(value));
    }
    if let Some(value) = value.as_number() {
        if let Some(int) = as_exact_int(value) {
            return Ok(JsonValue::Number(int.into()));
        }
        return serde_json::Number::from_f64(value)
            .map(JsonValue::Number)
            .ok_or(rquickjs::Error::Unknown);
    }
    if let Some(value) = value.as_string() {
        let text = value.to_string()?;
        return Ok(JsonValue::String(text));
    }
    if let Some(array) = value.as_array() {
        let mut items = Vec::with_capacity(array.len());
        for item in array.iter::<JsValue>() {
            items.push(js_to_json(_ctx, &item?)?);
        }
        return Ok(JsonValue::Array(items));
    }
    if let Some(object) = value.as_object() {
        let mut fields = serde_json::Map::new();
        for property in object.props::<rquickjs::String, JsValue>() {
            let (key, field) = property?;
            fields.insert(key.to_string()?, js_to_json(_ctx, &field)?);
        }
        return Ok(JsonValue::Object(fields));
    }
    Ok(JsonValue::Null)
}

#[cfg(test)]
fn as_exact_int(value: f64) -> Option<i64> {
    if value.fract() == 0.0 && value.is_finite() && value.abs() <= 9.007_199_254_740_992e15 {
        Some(value as i64)
    } else {
        None
    }
}
