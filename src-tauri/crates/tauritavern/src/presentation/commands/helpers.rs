use std::fmt::Display;

use tauri::{Manager, Resource, Webview};

use crate::presentation::errors::CommandError;

/// A page reload cannot await JS cleanup, so the host closes the page's resources of type `T`. In-flight operations
/// retain their own Arc.
pub(crate) fn close_page_resources<T: Resource>(webview: &Webview) {
    let mut table = webview.resources_table();
    let ids: Vec<_> = table
        .names()
        .map(|(rid, _)| rid)
        .filter(|rid| table.get::<T>(*rid).is_ok())
        .collect();
    for rid in ids {
        let _ = table.close(rid);
    }
}

pub fn ensure_ios_policy_allows(
    ios_policy: &tt_domain::ios_policy::IosPolicyActivationReport,
    allowed: bool,
    capability: &'static str,
) -> Result<(), CommandError> {
    if ios_policy.scope == tt_domain::ios_policy::IosPolicyScope::Ios && !allowed {
        return Err(CommandError::Forbidden(format!(
            "iOS policy disabled capability: {capability}"
        )));
    }

    Ok(())
}

pub fn log_command(command: impl AsRef<str>) {
    tracing::debug!(
        target: crate::observability_targets::COMMAND,
        command = %command.as_ref(),
        "Command invoked",
    );
}

fn should_log_as_warning(error: &CommandError) -> bool {
    matches!(
        error,
        CommandError::TooManyRequests(_) | CommandError::Cancelled(_) | CommandError::Conflict(_)
    )
}

pub fn map_command_error<E>(context: impl AsRef<str>) -> impl FnOnce(E) -> CommandError
where
    E: Display + Into<CommandError>,
{
    let context = context.as_ref().to_string();

    move |error| {
        let error_text = error.to_string();
        let command_error: CommandError = error.into();
        let message = format!("{}: {}", context, error_text);

        if should_log_as_warning(&command_error) {
            tracing::warn!(
                target: crate::observability_targets::COMMAND,
                context = %context,
                error = %error_text,
                "Command returned expected failure",
            );
        } else {
            log_user_visible_error(&message);
        }

        command_error
    }
}

pub fn log_user_visible_error(message: impl AsRef<str>) {
    let message = message.as_ref();
    tracing::error!(
        target: crate::observability_targets::USER_VISIBLE_ERROR,
        "{message}",
    );
}

pub fn normalize_request_id(id: &str) -> Result<String, CommandError> {
    let trimmed = id.trim();
    if trimmed.is_empty() || trimmed.len() > 128 {
        return Err(CommandError::BadRequest(
            "Invalid request id length".to_string(),
        ));
    }

    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(CommandError::BadRequest(
            "Invalid request id characters".to_string(),
        ));
    }

    Ok(trimmed.to_string())
}
