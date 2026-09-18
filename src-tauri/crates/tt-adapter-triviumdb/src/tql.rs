use std::collections::BTreeMap;

use serde_json::Value;
use triviumdb::query::tql_ast::TqlStatement;
use triviumdb::query::tql_executor::TqlValue;
use triviumdb::query::tql_lexer::{TqlLexer, TqlToken};
use triviumdb::query::tql_parser::parse_tql_statement;
use triviumdb::{Database, TriviumError};
use tt_contracts::database::{QueryResult, QueryValue, SubgraphEdge};

use crate::operations::node;

pub(crate) fn query(
    db: &mut Database<f32>,
    text: &str,
    params: &BTreeMap<String, Value>,
) -> Result<QueryResult, TriviumError> {
    let bound = bind(text, params)?;
    match parse_tql_statement(&bound).map_err(TriviumError::QueryParse)? {
        TqlStatement::Query(_) => Ok(QueryResult::Query {
            rows: db
                .tql(&bound)?
                .into_iter()
                .map(|row| {
                    row.into_iter()
                        .map(|(name, value)| (name, query_value(value)))
                        .collect()
                })
                .collect(),
        }),
        TqlStatement::Mutation(_) => {
            let result = db.tql_mut(&bound)?;
            Ok(QueryResult::Mutation {
                affected: result.affected,
                created_ids: result.created_ids,
            })
        }
    }
}

/// Native prepared queries only accept scalar read parameters in 0.8.8.
/// Use its lexer to expand JSON values once, outside strings/comments. A dollar
/// token followed by ':' is a document-filter key ($eq, $and, ...), not a parameter.
fn bind(text: &str, params: &BTreeMap<String, Value>) -> Result<String, TriviumError> {
    let tokens = TqlLexer::new(text)
        .tokenize_with_positions()
        .map_err(|error| TriviumError::QueryParse(error.to_string()))?;
    let mut result = String::with_capacity(text.len());
    let mut cursor = 0;
    for pair in tokens.windows(2) {
        let TqlToken::DollarOp(name) = &pair[0].token else {
            continue;
        };
        if pair[1].token == TqlToken::Colon {
            continue;
        }
        let value = params
            .get(&name[1..])
            .ok_or_else(|| TriviumError::InvalidInput(format!("Missing TQL parameter {name}")))?;
        result.push_str(&text[cursor..pair[0].byte_start]);
        literal(value, &mut result);
        cursor = pair[0].byte_start + name.len();
    }
    result.push_str(&text[cursor..]);
    Ok(result)
}

// TQL string escapes differ from JSON (notably \r and \u). Quote for the native
// lexer, preserving all other characters literally, rather than JSON-stringifying.
fn quote(text: &str, output: &mut String) {
    output.push('\'');
    for ch in text.chars() {
        if matches!(ch, '\'' | '\\') {
            output.push('\\');
        }
        output.push(ch);
    }
    output.push('\'');
}

fn literal(value: &Value, output: &mut String) {
    match value {
        Value::String(text) => quote(text, output),
        Value::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                literal(value, output);
            }
            output.push(']');
        }
        Value::Object(values) => {
            output.push('{');
            for (index, (key, value)) in values.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                quote(key, output);
                output.push(':');
                literal(value, output);
            }
            output.push('}');
        }
        _ => output.push_str(&value.to_string()),
    }
}

fn query_value(value: TqlValue<f32>) -> QueryValue {
    match value {
        TqlValue::Node(value) => {
            QueryValue::Node(node(value.id, value.vector, value.payload, value.edges))
        }
        TqlValue::Edge(value) => QueryValue::Edge(SubgraphEdge {
            source_id: value.source_id,
            target_id: value.target_id,
            label: value.label,
            weight: value.weight,
            metadata: value.metadata,
        }),
        TqlValue::Int(value) => QueryValue::Integer(value),
        TqlValue::Float(value) => QueryValue::Float(value),
        TqlValue::String(value) => QueryValue::String(value),
        TqlValue::Bool(value) => QueryValue::Bool(value),
        TqlValue::Path(value) => QueryValue::Path(value),
        TqlValue::List(value) => QueryValue::List(value),
        TqlValue::Null => QueryValue::Null,
    }
}
