use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use crate::errors::DomainError;

pub const MAX_EXPANDED_TEXT_BYTES: usize = 16 * 1024 * 1024;

// Aliases share one captured string. No handlers or live frontend state are retained.
const FIELDS: &[(&str, &[&str])] = &[
    ("/names/user", &["user"]),
    ("/names/char", &["char"]),
    ("/names/group", &["group", "charifnotgroup"]),
    ("/names/groupNotMuted", &["groupnotmuted"]),
    ("/names/notChar", &["notchar"]),
    ("/character/charPrompt", &["charprompt"]),
    ("/character/charInstruction", &["charinstruction"]),
    ("/character/charJailbreak", &["charjailbreak"]),
    (
        "/character/description",
        &["description", "chardescription"],
    ),
    (
        "/character/personality",
        &["personality", "charpersonality"],
    ),
    ("/character/scenario", &["scenario", "charscenario"]),
    ("/character/persona", &["persona"]),
    ("/character/personaPosition", &["personaposition"]),
    ("/character/mesExamplesRaw", &["mesexamplesraw"]),
    ("/character/mesExamples", &["mesexamples"]),
    ("/character/charDepthPrompt", &["chardepthprompt"]),
    (
        "/character/creatorNotes",
        &["creatornotes", "charcreatornotes"],
    ),
    (
        "/character/version",
        &["version", "charversion", "char_version"],
    ),
    ("/character/firstMessage", &["greeting", "charfirstmessage"]),
    ("/system/model", &["model"]),
    ("/chat/lastMessageId", &["lastmessageid"]),
    ("/chat/lastSwipeId", &["lastswipeid"]),
    ("/chat/currentSwipeId", &["currentswipeid"]),
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrozenMacros {
    values: HashMap<String, Arc<str>>,
    alternate_greetings: Option<Vec<Arc<str>>>,
    local_variables: Option<HashMap<String, Arc<str>>>,
    global_variables: Option<HashMap<String, Arc<str>>>,
    outlets: Option<HashMap<String, Arc<str>>>,
}

impl FrozenMacros {
    pub fn from_context(
        context: &Value,
        extension_prompts: Option<&Value>,
    ) -> Result<Self, DomainError> {
        if !context.is_object() {
            return Err(DomainError::InvalidData(
                "macroContext must be an object".into(),
            ));
        }
        for group in ["names", "character", "system", "chat", "variableValues"] {
            if context.get(group).is_some_and(|value| !value.is_object()) {
                return Err(DomainError::InvalidData(format!(
                    "macroContext.{group} must be an object"
                )));
            }
        }
        let mut values = HashMap::new();
        for &(path, aliases) in FIELDS {
            let Some(value) = context.pointer(path) else {
                continue;
            };
            let text: Arc<str> = match value {
                Value::String(value) => Arc::from(value.as_str()),
                Value::Number(value) => Arc::from(value.to_string()),
                _ => {
                    return Err(DomainError::InvalidData(format!(
                        "macroContext{path} must be a string or number"
                    )));
                }
            };
            for &alias in aliases {
                values.insert(alias.to_string(), text.clone());
            }
        }
        if let Some(builtins) = text_map(context.get("builtins"), "macroContext.builtins")? {
            for (name, value) in builtins {
                values.insert(name.to_ascii_lowercase(), value);
            }
        }
        for (name, fallback) in [
            ("charifnotgroup", "char"),
            ("charinstruction", "charjailbreak"),
            ("charjailbreak", "charinstruction"),
            ("idle_duration", "idleduration"),
            ("maxprompttokens", "maxprompt"),
            ("maxcontexttokens", "maxcontext"),
            ("maxresponsetokens", "maxresponse"),
            ("instructuserprefix", "instructinput"),
            ("instructassistantprefix", "instructoutput"),
            ("instructassistantsuffix", "instructseparator"),
            ("instructfirstassistantprefix", "instructfirstoutput"),
            ("instructfirstoutputprefix", "instructfirstoutput"),
            ("instructlastassistantprefix", "instructlastoutput"),
            ("instructlastoutputprefix", "instructlastoutput"),
            ("instructfirstuserprefix", "instructfirstinput"),
            ("instructlastuserprefix", "instructlastinput"),
            ("instructsystem", "defaultsystemprompt"),
            ("instructsystemprompt", "defaultsystemprompt"),
            ("exampleseparator", "chatseparator"),
        ] {
            if let Some(value) = values.get(fallback).cloned() {
                values.entry(name.to_string()).or_insert(value);
            }
        }
        let alternate_greetings = context
            .pointer("/character/alternateGreetings")
            .map(|value| {
                let items = value.as_array().ok_or_else(|| {
                    DomainError::InvalidData(
                        "macroContext.character.alternateGreetings must be an array".into(),
                    )
                })?;
                items
                    .iter()
                    .map(|value| text_value(value, "macroContext.character.alternateGreetings[]"))
                    .collect::<Result<_, _>>()
            })
            .transpose()?;
        let outlets = extension_prompts
            .map(|value| {
                let prompts = value.as_object().ok_or_else(|| {
                    DomainError::InvalidData("frozen extensionPrompts must be an object".into())
                })?;
                prompts
                    .iter()
                    .filter_map(|(name, prompt)| {
                        name.strip_prefix("customWIOutlet_")
                            .map(|key| (key, prompt))
                    })
                    .map(|(key, prompt)| {
                        text_value(&prompt["value"], &format!("outlet::{key}"))
                            .map(|value| (key.to_string(), value))
                    })
                    .collect::<Result<_, _>>()
            })
            .transpose()?;
        Ok(Self {
            values,
            alternate_greetings,
            local_variables: text_map(
                context.pointer("/variableValues/local"),
                "macroContext.variableValues.local",
            )?,
            global_variables: text_map(
                context.pointer("/variableValues/global"),
                "macroContext.variableValues.global",
            )?,
            outlets,
        })
    }

    fn lookup(&self, expression: &str) -> Option<&str> {
        let (name, argument) = match expression.split_once("::") {
            Some((name, argument)) if !argument.contains("::") => {
                (name.trim(), Some(argument.trim()))
            }
            Some(_) => return None,
            None => (expression.trim(), None),
        };
        let name = name.to_ascii_lowercase();
        let Some(argument) = argument else {
            return match name.as_str() {
                "space" => Some(" "),
                "newline" => Some("\n"),
                "noop" => Some(""),
                _ => self.values.get(name.as_str()).map(AsRef::as_ref),
            };
        };
        match name.as_str() {
            "greeting" | "charfirstmessage" => {
                let digits = argument.strip_prefix('-').unwrap_or(argument);
                if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
                    return None;
                }
                if argument.parse::<i64>() == Ok(0) {
                    return self.values.get("greeting").map(AsRef::as_ref);
                }
                let greetings = self.alternate_greetings.as_ref()?;
                Some(
                    argument
                        .parse::<usize>()
                        .ok()
                        .and_then(|index| index.checked_sub(1))
                        .and_then(|index| greetings.get(index))
                        .map(AsRef::as_ref)
                        .unwrap_or(""),
                )
            }
            "getvar" | "getglobalvar" | "outlet" => {
                let values = match name.as_str() {
                    "getvar" => &self.local_variables,
                    "getglobalvar" => &self.global_variables,
                    _ => &self.outlets,
                };
                Some(
                    values
                        .as_ref()?
                        .get(argument)
                        .map(AsRef::as_ref)
                        .unwrap_or(""),
                )
            }
            "hasvar" | "varexists" | "hasglobalvar" | "globalvarexists" => {
                let values = if matches!(name.as_str(), "hasvar" | "varexists") {
                    &self.local_variables
                } else {
                    &self.global_variables
                };
                Some(if values.as_ref()?.contains_key(argument) {
                    "true"
                } else {
                    "false"
                })
            }
            _ => None,
        }
    }

    /// One pass over the source; inserted values are never scanned again.
    /// Unknown and nested expressions stay literal. Backslashes escape an opener.
    pub fn render<'a>(&self, text: &'a str, max_bytes: usize) -> Result<Cow<'a, str>, DomainError> {
        if text.len() > max_bytes {
            return Err(DomainError::InvalidData(format!(
                "macro input exceeds {max_bytes} bytes"
            )));
        }
        let bytes = text.as_bytes();
        let mut cursor = 0;
        let mut copied = 0;
        let mut output: Option<String> = None;
        while let Some(offset) = text[cursor..].find("{{") {
            let open = cursor + offset;
            let mut end = open + 2;
            let mut depth = 1;
            let mut nested = false;
            while end + 1 < bytes.len() && depth > 0 {
                match &bytes[end..end + 2] {
                    b"{{" => {
                        depth += 1;
                        nested = true;
                        end += 2;
                    }
                    b"}}" => {
                        depth -= 1;
                        end += 2;
                    }
                    _ => end += 1,
                }
            }
            if depth != 0 {
                break;
            }
            let slashes = bytes[..open]
                .iter()
                .rev()
                .take_while(|&&byte| byte == b'\\')
                .count();
            let value = if !nested && slashes % 2 == 0 {
                self.lookup(&text[open + 2..end - 2])
            } else {
                None
            };
            if slashes > 0 || value.is_some() {
                let output = output.get_or_insert_with(String::new);
                append(output, &text[copied..open - slashes], max_bytes)?;
                append(
                    output,
                    &text[open - slashes..open - slashes + slashes / 2],
                    max_bytes,
                )?;
                append(output, value.unwrap_or(&text[open..end]), max_bytes)?;
                copied = end;
            }
            cursor = end;
        }
        match output {
            Some(mut output) => {
                append(&mut output, &text[copied..], max_bytes)?;
                Ok(Cow::Owned(output))
            }
            None => Ok(Cow::Borrowed(text)),
        }
    }
}

fn text_value(value: &Value, path: &str) -> Result<Arc<str>, DomainError> {
    value
        .as_str()
        .map(Arc::from)
        .ok_or_else(|| DomainError::InvalidData(format!("{path} must be a string")))
}

fn text_map(
    value: Option<&Value>,
    path: &str,
) -> Result<Option<HashMap<String, Arc<str>>>, DomainError> {
    value
        .map(|value| {
            let values = value
                .as_object()
                .ok_or_else(|| DomainError::InvalidData(format!("{path} must be an object")))?;
            values
                .iter()
                .map(|(key, value)| {
                    text_value(value, &format!("{path}.{key}")).map(|value| (key.clone(), value))
                })
                .collect()
        })
        .transpose()
}

fn append(output: &mut String, text: &str, max_bytes: usize) -> Result<(), DomainError> {
    if text.len() > max_bytes.saturating_sub(output.len()) {
        return Err(DomainError::InvalidData(format!(
            "macro expansion exceeds {max_bytes} bytes"
        )));
    }
    output.push_str(text);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn replays_values_once_with_aliases_and_literal_expressions() {
        let macros = FrozenMacros::from_context(
            &json!({
                "names": { "char": "小明" },
                "character": { "description": "{{char}}\n第二行", "personaPosition": 0 }
            }),
            None,
        )
        .unwrap();
        assert_eq!(
            macros
                .render("{{CHAR}}: {{charDescription}} / {{personaPosition}}", 100)
                .unwrap(),
            "小明: {{char}}\n第二行 / 0"
        );
        for text in [
            "{{unknown}}",
            "{{setvar::x::1}}",
            "{{getvar::{{char}}}}",
            "{{char",
        ] {
            assert_eq!(macros.render(text, 100).unwrap(), text);
        }
        assert_eq!(
            macros.render(r"\{{char}} \\{{char}}", 100).unwrap(),
            r"{{char}} \小明"
        );
        assert!(macros.render("{{description}}", 17).is_err());
    }

    #[test]
    fn reads_literal_arguments_from_frozen_arrays_and_dictionaries() {
        let macros = FrozenMacros::from_context(&json!({
            "character": { "firstMessage": "Main", "alternateGreetings": ["{{char}}", "Second"] },
            "variableValues": {
                "local": { "Name": "7", "name": "lowercase", "empty": "" },
                "global": { "Name": "8" }
            },
            "builtins": { "maxPrompt": "4096" }
        }), Some(&json!({
            "customWIOutlet_Lore": { "value": "Frozen lore" },
            "other": { "value": "Not an outlet" }
        }))).unwrap();
        assert_eq!(macros.render(
            "{{greeting::0}}|{{charFirstMessage::01}}|{{greeting::2}}|{{ GETVAR :: Name }}|{{getvar::name}}|{{getglobalvar::Name}}|{{outlet::Lore}}|{{maxPromptTokens}}",
            512,
        ).unwrap(), "Main|{{char}}|Second|7|lowercase|8|Frozen lore|4096");
        assert_eq!(macros.render(
            "{{greeting::-1}}{{greeting::999999999999999999999999}}{{getvar::missing}}{{outlet::lore}}/{{hasvar::empty}}/{{varexists::missing}}/{{hasglobalvar::Name}}",
            512,
        ).unwrap(), "/true/false/true");
        for source in [
            "{{greeting::1.5}}",
            "{{getvar::Name::extra}}",
            "{{getvar::{{char}}}}",
            "{{setvar::Name::1}}",
        ] {
            assert_eq!(macros.render(source, 100).unwrap(), source);
        }
        assert_eq!(
            FrozenMacros::default()
                .render("{{getvar::Name}}", 100)
                .unwrap(),
            "{{getvar::Name}}"
        );
        assert!(
            FrozenMacros::from_context(
                &json!({ "variableValues": { "local": { "Name": 7 } } }),
                None
            )
            .is_err()
        );
    }
}
