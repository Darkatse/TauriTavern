use std::fmt::Display;

use crate::infrastructure::logging::logger;
use crate::presentation::errors::CommandError;

pub fn ensure_ios_policy_allows(
    ios_policy: &crate::domain::ios_policy::IosPolicyActivationReport,
    allowed: bool,
    capability: &'static str,
) -> Result<(), CommandError> {
    if ios_policy.scope == crate::domain::ios_policy::IosPolicyScope::Ios && !allowed {
        return Err(CommandError::Unauthorized(format!(
            "iOS policy disabled capability: {capability}"
        )));
    }

    Ok(())
}

pub fn log_command(command: impl AsRef<str>) {
    logger::debug(&format!("Command: {}", command.as_ref()));
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

        match &command_error {
            CommandError::TooManyRequests(_) => logger::warn(&message),
            _ => logger::error(&message),
        }

        command_error
    }
}
