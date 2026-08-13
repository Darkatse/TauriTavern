//! Logging API for scripts
//! 
//! Provides logging capabilities for debugging and information output.

use rquickjs::{Ctx, Result, Object, Function, Rest};
use log::{info, warn, error, debug};

/// Logging API exposed to scripts as $log
pub struct LogApi;

impl LogApi {
    pub fn new() -> Self {
        Self
    }

    /// Log an info message
    pub fn info(message: String, args: Rest<String>) {
        let formatted = format_args_with_rest(&message, args);
        info!("{}", formatted);
    }

    /// Log a warning message
    pub fn warn(message: String, args: Rest<String>) {
        let formatted = format_args_with_rest(&message, args);
        warn!("{}", formatted);
    }

    /// Log an error message
    pub fn error(message: String, args: Rest<String>) {
        let formatted = format_args_with_rest(&message, args);
        error!("{}", formatted);
    }

    /// Log a debug message
    pub fn debug(message: String, args: Rest<String>) {
        let formatted = format_args_with_rest(&message, args);
        debug!("{}", formatted);
    }

    /// Register the $log API object in the QuickJs context
    pub fn register<'js>(&self, ctx: &Ctx<'js>) -> Result<()> {
        let globals = ctx.globals();
        
        let log_obj = Object::new(ctx.clone())?;
        
        let info_fn = Function::new(ctx.clone(), |message: String, args: Rest<String>| {
            Self::info(message, args);
        })?;
        
        let warn_fn = Function::new(ctx.clone(), |message: String, args: Rest<String>| {
            Self::warn(message, args);
        })?;
        
        let error_fn = Function::new(ctx.clone(), |message: String, args: Rest<String>| {
            Self::error(message, args);
        })?;
        
        let debug_fn = Function::new(ctx.clone(), |message: String, args: Rest<String>| {
            Self::debug(message, args);
        })?;
        
        log_obj.set("info", info_fn)?;
        log_obj.set("warn", warn_fn)?;
        log_obj.set("error", error_fn)?;
        log_obj.set("debug", debug_fn)?;
        
        globals.set("$log", log_obj)?;
        
        Ok(())
    }
}

fn format_args_with_rest(message: &str, args: Rest<String>) -> String {
    if args.0.is_empty() {
        message.to_string()
    } else {
        format!("{} {}", message, args.0.join(" "))
    }
}
