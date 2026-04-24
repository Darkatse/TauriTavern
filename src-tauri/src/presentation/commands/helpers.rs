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

fn should_log_as_warning(error: &CommandError) -> bool {
    matches!(
        error,
        CommandError::TooManyRequests(_) | CommandError::Cancelled(_)
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
            logger::warn(&message);
        } else {
            logger::error(&message);
        }

        command_error
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::errors::GENERATION_CANCELLED_BY_USER_MESSAGE;

    use super::*;

    #[test]
    fn should_log_cancelled_as_warning() {
        assert!(should_log_as_warning(&CommandError::Cancelled(
            GENERATION_CANCELLED_BY_USER_MESSAGE.to_string()
        )));
    }

    #[test]
    fn should_not_log_internal_server_error_as_warning() {
        assert!(!should_log_as_warning(&CommandError::InternalServerError(
            "Boom".to_string()
        )));
    }
}
