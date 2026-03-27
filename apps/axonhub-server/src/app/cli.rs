use std::process;

use anyhow::Result;
use axonhub_config::{load, supported_config_aliases, supported_config_keys, PreviewFormat};
use clap::{ArgAction, Args, Command, CommandFactory, FromArgMatches, Parser, Subcommand};

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
    "  axonhub build-info         Show detailed build information\n",
    "  axonhub version            Show version\n",
    "  axonhub help               Show this help message\n",
    "\n",
    "Options:\n",
    "  -f, --format FORMAT       Output format for config preview (yml, json)\n",
);

pub(crate) const CONFIG_USAGE_TEXT: &str = "Usage: axonhub config <preview|validate|get>\n";

pub(crate) const CONFIG_GET_USAGE_HEADER: &str = "Usage: axonhub config get <key>\n\nAvailable keys:\n";

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

#[derive(Debug, Clone, Parser)]
#[command(
    name = "axonhub",
    bin_name = "axonhub",
    disable_help_flag = true,
    disable_help_subcommand = true,
    disable_version_flag = true
)]
pub(crate) struct AxonhubCliContract {
    #[arg(short = 'h', long = "help", action = ArgAction::SetTrue)]
    help: bool,
    #[arg(short = 'v', long = "version", action = ArgAction::SetTrue)]
    version: bool,
    #[command(subcommand)]
    command: Option<AxonhubTopLevelVerb>,
}

#[derive(Debug, Clone, Subcommand)]
enum AxonhubTopLevelVerb {
    Config(ConfigArgs),
    Version,
    Help,
    BuildInfo,
    #[allow(dead_code)]
    #[command(external_subcommand)]
    External(Vec<String>),
}

#[derive(Debug, Clone, Args)]
struct ConfigArgs {
    #[arg(hide = true, trailing_var_arg = true, allow_hyphen_values = true)]
    _tail: Vec<String>,
}

#[derive(Debug, Clone, Parser)]
#[command(
    name = "axonhub config",
    bin_name = "axonhub config",
    no_binary_name = true,
    disable_help_flag = true,
    disable_help_subcommand = true,
    disable_version_flag = true
)]
pub(crate) struct AxonhubConfigCliContract {
    #[command(subcommand)]
    command: Option<AxonhubConfigVerb>,
}

#[derive(Debug, Clone, Subcommand)]
enum AxonhubConfigVerb {
    Preview(ConfigPreviewArgs),
    Validate(ConfigValidateArgs),
    Get(ConfigGetArgs),
    #[allow(dead_code)]
    #[command(external_subcommand)]
    External(Vec<String>),
}

#[derive(Debug, Clone, Args)]
struct ConfigPreviewArgs {
    #[arg(hide = true, trailing_var_arg = true, allow_hyphen_values = true)]
    _tail: Vec<String>,
}

#[derive(Debug, Clone, Args)]
struct ConfigValidateArgs {
    #[arg(hide = true, trailing_var_arg = true, allow_hyphen_values = true)]
    _tail: Vec<String>,
}

#[derive(Debug, Clone, Args)]
struct ConfigGetArgs {
    #[arg(hide = true, trailing_var_arg = true, allow_hyphen_values = true)]
    _tail: Vec<String>,
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
                println!("Unsupported format: {value}");
                process::exit(1);
            });
            index += 2;
            continue;
        }

        index += 1;
    }

    let loaded = load().unwrap_or_else(|error| {
        println!("Failed to load config: {error}");
        process::exit(1);
    });
    let preview = loaded.preview(format).unwrap_or_else(|error| {
        println!("Failed to preview config: {error}");
        process::exit(1);
    });
    println!("{preview}");

    Ok(())
}

fn config_validate() -> Result<()> {
    let loaded = load().unwrap_or_else(|error| {
        println!("Failed to load config: {error}");
        process::exit(1);
    });
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
        print!("{}", config_get_usage_text());
        process::exit(1);
    }

    let key = &args[3];
    let loaded = load().unwrap_or_else(|error| {
        eprintln!("Failed to load config: {error}");
        process::exit(1);
    });

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

pub(crate) fn axonhub_cli_command() -> Command {
    AxonhubCliContract::command().ignore_errors(true)
}

pub(crate) fn axonhub_config_cli_command() -> Command {
    AxonhubConfigCliContract::command().ignore_errors(true)
}

fn print_config_usage() {
    print!("{CONFIG_USAGE_TEXT}");
}

pub(crate) fn config_get_usage_text() -> String {
    let mut usage = String::from(CONFIG_GET_USAGE_HEADER);

    for key in supported_config_keys() {
        usage.push_str("  ");
        usage.push_str(key.key);
        usage.push_str("    ");
        usage.push_str(key.description);
        usage.push('\n');
    }

    let aliases = supported_config_aliases();
    if !aliases.is_empty() {
        usage.push_str("\nLegacy aliases accepted by get/preview validation:\n");
        for alias in aliases {
            usage.push_str("  ");
            usage.push_str(alias.key);
            usage.push_str("    ");
            usage.push_str(alias.description);
            usage.push_str(" (canonical: ");
            usage.push_str(alias.canonical_key);
            usage.push_str(")\n");
        }
    }

    usage
}

pub(crate) fn parse_top_level_command(args: &[String]) -> TopLevelCommand {
    match parse_axonhub_cli(args) {
        Some(AxonhubCliContract { help: true, .. }) => TopLevelCommand::Help,
        Some(AxonhubCliContract { version: true, .. }) => TopLevelCommand::Version,
        Some(AxonhubCliContract {
            command: Some(AxonhubTopLevelVerb::Config(_)),
            ..
        }) => TopLevelCommand::Config,
        Some(AxonhubCliContract {
            command: Some(AxonhubTopLevelVerb::Version),
            ..
        }) => TopLevelCommand::Version,
        Some(AxonhubCliContract {
            command: Some(AxonhubTopLevelVerb::Help),
            ..
        }) => TopLevelCommand::Help,
        Some(AxonhubCliContract {
            command: Some(AxonhubTopLevelVerb::BuildInfo),
            ..
        }) => TopLevelCommand::BuildInfo,
        Some(AxonhubCliContract {
            command: Some(AxonhubTopLevelVerb::External(_)),
            ..
        })
        | Some(AxonhubCliContract { command: None, .. })
        | None => TopLevelCommand::StartServer,
    }
}

pub(crate) fn parse_config_command(args: &[String]) -> Option<ConfigCommand> {
    let config_args = args.get(2..)?;

    match parse_axonhub_config_cli(config_args) {
        Some(AxonhubConfigCliContract {
            command: Some(AxonhubConfigVerb::Preview(_)),
        }) => Some(ConfigCommand::Preview),
        Some(AxonhubConfigCliContract {
            command: Some(AxonhubConfigVerb::Validate(_)),
        }) => Some(ConfigCommand::Validate),
        Some(AxonhubConfigCliContract {
            command: Some(AxonhubConfigVerb::Get(_)),
        }) => Some(ConfigCommand::Get),
        Some(AxonhubConfigCliContract {
            command: Some(AxonhubConfigVerb::External(_)),
        })
        | Some(AxonhubConfigCliContract { command: None })
        | None => None,
    }
}

fn parse_axonhub_cli(args: &[String]) -> Option<AxonhubCliContract> {
    parse_command(axonhub_cli_command(), args)
}

fn parse_axonhub_config_cli(args: &[String]) -> Option<AxonhubConfigCliContract> {
    parse_command(axonhub_config_cli_command(), args)
}

fn parse_command<T>(mut command: Command, args: &[String]) -> Option<T>
where
    T: FromArgMatches,
{
    let matches = command.try_get_matches_from_mut(args).ok()?;
    T::from_arg_matches(&matches).ok()
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
