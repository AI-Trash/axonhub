use std::process;

use anyhow::{Context, Result};
use axonhub_config::{load, PreviewFormat};

use super::build_info::{show_build_info, show_version};
use super::server::start_server;

pub(crate) const HELP_TEXT: &str = concat!(
    "AxonHub AI Gateway\n",
    "\n",
    "Usage:\n",
    "  axonhub                    Start the server (default)\n",
    "  axonhub config preview     Preview configuration\n",
    "  axonhub config validate    Validate configuration\n",
    "  axonhub config get <key>   Get a specific config value\n",
    "  axonhub version            Show version\n",
    "  axonhub build-info         Show build information\n",
    "  axonhub help               Show this help message\n",
    "\n",
    "Options:\n",
    "  -f, --format FORMAT        Output format for config preview (yml, json)\n",
);

pub(crate) const CONFIG_USAGE_TEXT: &str = "Usage: axonhub config <preview|validate|get>\n";

pub(crate) const CONFIG_GET_USAGE_TEXT: &str = concat!(
    "Usage: axonhub config get <key>\n",
    "\n",
    "Available keys:\n",
    "  server.port       Server port number\n",
    "  server.name       Server name\n",
    "  server.base_path  Server base path\n",
    "  server.debug      Server debug mode\n",
    "  db.dialect        Database dialect\n",
    "  db.dsn            Database DSN\n",
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TopLevelCommand {
    StartServer,
    Config,
    Version,
    Help,
    BuildInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigCommand {
    Preview,
    Validate,
    Get,
}

pub(crate) async fn run(args: &[String]) -> Result<()> {
    match parse_top_level_command(args) {
        TopLevelCommand::Config => {
            handle_config_command(args)?;
            Ok(())
        }
        TopLevelCommand::Version => {
            show_version();
            Ok(())
        }
        TopLevelCommand::Help => {
            show_help();
            Ok(())
        }
        TopLevelCommand::BuildInfo => {
            show_build_info();
            Ok(())
        }
        TopLevelCommand::StartServer => start_server().await,
    }
}

fn handle_config_command(args: &[String]) -> Result<()> {
    match parse_config_command(args) {
        Some(ConfigCommand::Preview) => config_preview(args),
        Some(ConfigCommand::Validate) => config_validate(),
        Some(ConfigCommand::Get) => config_get(args),
        _ => {
            print_config_usage();
            process::exit(1);
        }
    }
}

fn config_preview(args: &[String]) -> Result<()> {
    let mut format = PreviewFormat::Yaml;
    let mut index = 3;

    while index < args.len() {
        if matches!(args[index].as_str(), "--format" | "-f") {
            let value = args.get(index + 1).map(String::as_str).unwrap_or_default();
            format = PreviewFormat::parse(value).unwrap_or_else(|| {
                eprintln!("Unsupported format: {value}");
                process::exit(1);
            });
            index += 2;
            continue;
        }

        index += 1;
    }

    let loaded = load().context("Failed to load config")?;
    println!("{}", loaded.preview(format)?);

    Ok(())
}

fn config_validate() -> Result<()> {
    let loaded = load().context("Failed to load config")?;
    let errors = loaded.config.validation_errors();

    if errors.is_empty() {
        println!("Configuration is valid!");
        return Ok(());
    }

    println!("Configuration validation failed:");
    for error in errors {
        println!("  - {error}");
    }

    process::exit(1);
}

fn config_get(args: &[String]) -> Result<()> {
    if args.len() < 4 {
        print!("{CONFIG_GET_USAGE_TEXT}");
        process::exit(1);
    }

    let key = &args[3];
    let loaded = load().context("Failed to load config")?;

    if let Some(value) = loaded.get(key) {
        println!("{}", format_json_value(&value)?);
    } else {
        eprintln!("Unknown config key: {key}");
        process::exit(1);
    }

    Ok(())
}

fn show_help() {
    print!("{HELP_TEXT}");
}

fn print_config_usage() {
    print!("{CONFIG_USAGE_TEXT}");
}

pub(crate) fn parse_top_level_command(args: &[String]) -> TopLevelCommand {
    match args.get(1).map(String::as_str) {
        Some("config") => TopLevelCommand::Config,
        Some("version" | "--version" | "-v") => TopLevelCommand::Version,
        Some("help" | "--help" | "-h") => TopLevelCommand::Help,
        Some("build-info") => TopLevelCommand::BuildInfo,
        _ => TopLevelCommand::StartServer,
    }
}

pub(crate) fn parse_config_command(args: &[String]) -> Option<ConfigCommand> {
    match args.get(2).map(String::as_str) {
        Some("preview") => Some(ConfigCommand::Preview),
        Some("validate") => Some(ConfigCommand::Validate),
        Some("get") => Some(ConfigCommand::Get),
        _ => None,
    }
}

fn format_json_value(value: &serde_json::Value) -> Result<String> {
    match value {
        serde_json::Value::Null => Ok("null".to_owned()),
        serde_json::Value::Bool(boolean) => Ok(boolean.to_string()),
        serde_json::Value::Number(number) => Ok(number.to_string()),
        serde_json::Value::String(string) => Ok(string.clone()),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            Ok(serde_json::to_string_pretty(value)?)
        }
    }
}
