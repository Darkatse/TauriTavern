use std::fmt::Display;

use crate::infrastructure::logging::logger;
use crate::presentation::errors::CommandError;

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
