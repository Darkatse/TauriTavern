use serde_json::Value;

const UNSET_SENTINEL: &str = "__@@UNSET@@__";

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

fn is_unset_sentinel(value: &Value) -> bool {
    value.as_str() == Some(UNSET_SENTINEL)
}

fn prune_unset_sentinels(value: &mut Value) {
    let Value::Object(object) = value else {
        return;
    };

    object.retain(|_, child| {
        if is_unset_sentinel(child) {
            return false;
        }

        prune_unset_sentinels(child);
        true
    });
}

pub(crate) fn merge_json_value_with_unset(current: &mut Value, updates: Value) {
    match (current, updates) {
        (Value::Object(current_object), Value::Object(updates_object)) => {
            for (key, mut value) in updates_object {
                if is_unset_sentinel(&value) {
                    current_object.remove(&key);
                    continue;
                }

                match current_object.get_mut(&key) {
                    Some(current_value) => merge_json_value_with_unset(current_value, value),
                    None => {
                        prune_unset_sentinels(&mut value);
                        current_object.insert(key, value);
                    }
                }
            }
        }
        (current_value, mut updates_value) => {
            prune_unset_sentinels(&mut updates_value);
            *current_value = updates_value;
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::merge_json_value_with_unset;

    #[test]
    fn merge_json_value_with_unset_deletes_nested_keys() {
        let mut current = json!({
            "data": {
                "extensions": {
                    "keep": true,
                    "remove": "old",
                    "nested": {
                        "remove": "old"
                    }
                }
            }
        });

        merge_json_value_with_unset(
            &mut current,
            json!({
                "data": {
                    "extensions": {
                        "remove": "__@@UNSET@@__",
                        "nested": {
                            "remove": "__@@UNSET@@__",
                            "add": 1
                        }
                    }
                }
            }),
        );

        assert_eq!(current.pointer("/data/extensions/keep"), Some(&json!(true)));
        assert_eq!(current.pointer("/data/extensions/remove"), None);
        assert_eq!(current.pointer("/data/extensions/nested/remove"), None);
        assert_eq!(
            current.pointer("/data/extensions/nested/add"),
            Some(&json!(1))
        );
    }

    #[test]
    fn merge_json_value_with_unset_prunes_replaced_objects() {
        let mut current = json!({
            "data": {
                "extensions": {
                    "nested": "legacy"
                }
            }
        });

        merge_json_value_with_unset(
            &mut current,
            json!({
                "data": {
                    "extensions": {
                        "nested": {
                            "remove": "__@@UNSET@@__",
                            "add": true
                        }
                    }
                }
            }),
        );

        assert_eq!(current.pointer("/data/extensions/nested/remove"), None);
        assert_eq!(
            current.pointer("/data/extensions/nested/add"),
            Some(&json!(true))
        );
    }
}
