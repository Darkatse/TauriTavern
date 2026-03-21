use serde_json::Value;

pub(crate) fn merge_json_value(current: &mut Value, updates: Value) {
    match (current, updates) {
        (Value::Object(current_object), Value::Object(updates_object)) => {
            for (key, value) in updates_object {
                match current_object.get_mut(&key) {
                    Some(current_value) => merge_json_value(current_value, value),
                    None => {
                        current_object.insert(key, value);
                    }
                }
            }
        }
        (current_value, updates_value) => *current_value = updates_value,
    }
}
