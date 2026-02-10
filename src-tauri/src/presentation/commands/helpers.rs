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
        logger::error(&format!("{}: {}", context, error));
        error.into()
    }
}
