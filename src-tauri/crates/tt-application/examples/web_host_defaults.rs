#[path = "support/web_host_defaults.rs"]
mod defaults;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let defaults = defaults::native_defaults()?;
    println!("{}", serde_json::to_string_pretty(&defaults)?);
    Ok(())
}
