use std::process;

use anyhow::Result;
use serde_json::Value;
use axonhub_config::{load_for_cli, PreviewFormat};
use clap::{
    error::ErrorKind, Arg, ArgAction, Args, Command, CommandFactory, FromArgMatches, Parser, Subcommand,
    ValueEnum,
};

use super::build_info::{show_build_info, show_version};
use super::server::start_server;

#[derive(Debug, Clone, Parser)]
#[command(
    name = "axonhub",
    bin_name = "axonhub",
    about = "AxonHub AI Gateway",
    long_about = None
)]
pub(crate) struct AxonhubCliContract {
    #[command(subcommand)]
    command: Option<AxonhubTopLevelVerb>,
}

#[derive(Debug, Clone, Subcommand)]
enum AxonhubTopLevelVerb {
    #[command(about = "Configuration helpers")]
    Config(ConfigArgs),
    #[command(about = "Show version")]
    Version,
    #[command(about = "Show detailed build metadata")]
    BuildInfo,
    #[allow(dead_code)]
    #[command(external_subcommand)]
    External(Vec<String>),
}

#[derive(Debug, Clone, Args)]
#[command(arg_required_else_help = true)]
struct ConfigArgs {
    #[command(subcommand)]
    command: AxonhubConfigVerb,
}

#[derive(Debug, Clone, Subcommand)]
enum AxonhubConfigVerb {
    #[command(about = "Preview configuration")]
    Preview(ConfigPreviewArgs),
    #[command(about = "Validate configuration")]
    Validate,
    #[command(about = "Get a specific config value")]
    Get(ConfigGetArgs),
}

#[derive(Debug, Clone, Args)]
struct ConfigPreviewArgs {
    #[arg(
        short,
        long,
        value_enum,
        default_value_t = CliPreviewFormat::Yml,
        help = "Output format for config preview"
    )]
    format: CliPreviewFormat,
}

#[derive(Debug, Clone, Args)]
struct ConfigGetArgs {
    #[arg(value_enum, help = "Configuration key to inspect")]
    key: ConfigKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CliPreviewFormat {
    #[value(name = "yml", alias = "yaml")]
    Yml,
    #[value(name = "json")]
    Json,
}

impl From<CliPreviewFormat> for PreviewFormat {
    fn from(value: CliPreviewFormat) -> Self {
        match value {
            CliPreviewFormat::Yml => PreviewFormat::Yaml,
            CliPreviewFormat::Json => PreviewFormat::Json,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ConfigKey {
    #[value(name = "server.port", help = "Server port number")]
    ServerPort,
    #[value(name = "server.name", help = "Server name")]
    ServerName,
    #[value(name = "server.base_path", help = "Server base path")]
    ServerBasePath,
    #[value(name = "server.debug", help = "Server debug mode")]
    ServerDebug,
    #[value(name = "db.dialect", help = "Database dialect")]
    DbDialect,
    #[value(name = "db.dsn", help = "Database DSN")]
    DbDsn,
}

pub(crate) async fn run(args: &[String]) -> Result<()> {
    let cli = match parse_axonhub_cli(args) {
        Ok(cli) => cli,
        Err(error) => match error.kind() {
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
                error.print()?;
                return Ok(());
            }
            _ => error.exit(),
        },
    };

    match cli.command {
        Some(AxonhubTopLevelVerb::Config(config)) => handle_config_command(config),
        Some(AxonhubTopLevelVerb::Version) => {
            show_version();
            Ok(())
        }
        Some(AxonhubTopLevelVerb::BuildInfo) => {
            show_build_info();
            Ok(())
        }
        Some(AxonhubTopLevelVerb::External(_)) | None => start_server().await,
    }
}

fn handle_config_command(args: ConfigArgs) -> Result<()> {
    match args.command {
        AxonhubConfigVerb::Preview(args) => config_preview(args),
        AxonhubConfigVerb::Validate => config_validate(),
        AxonhubConfigVerb::Get(args) => config_get(args),
    }
}

fn config_preview(args: ConfigPreviewArgs) -> Result<()> {
    let loaded = load_for_cli().unwrap_or_else(|error| {
        println!("Failed to load config: {error}");
        process::exit(1);
    });
    let preview = loaded.preview(args.format.into()).unwrap_or_else(|error| {
        println!("Failed to preview config: {error}");
        process::exit(1);
    });
    println!("{preview}");

    Ok(())
}

fn config_validate() -> Result<()> {
    let loaded = load_for_cli().unwrap_or_else(|error| {
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

fn config_get(args: ConfigGetArgs) -> Result<()> {
    let loaded = load_for_cli().unwrap_or_else(|error| {
        eprintln!("Failed to load config: {error}");
        process::exit(1);
    });

    let value = cli_config_get_value(&loaded.config, args.key);
    println!("{}", format_json_value(&value)?);

    Ok(())
}

pub(crate) fn axonhub_cli_command() -> Command {
    AxonhubCliContract::command()
        .disable_version_flag(true)
        .arg(
            Arg::new("version")
                .short('v')
                .long("version")
                .action(ArgAction::Version)
                .help("Print version information"),
        )
        .version(super::build_info::version())
        .long_version(super::build_info::version())
}

pub(crate) fn axonhub_config_cli_command() -> Command {
    let mut command = axonhub_cli_command();
    command
        .find_subcommand_mut("config")
        .expect("config subcommand should exist")
        .clone()
}

fn cli_config_get_value(config: &axonhub_config::Config, key: ConfigKey) -> Value {
    match key {
        ConfigKey::ServerPort => Value::from(config.server.port),
        ConfigKey::ServerName => Value::from(config.server.name.clone()),
        ConfigKey::ServerBasePath => Value::from(config.server.base_path.clone()),
        ConfigKey::ServerDebug => Value::from(config.server.debug),
        ConfigKey::DbDialect => Value::from(config.db.dialect.clone()),
        ConfigKey::DbDsn => Value::from(config.db.dsn.clone()),
    }
}
pub(crate) fn parse_axonhub_cli(
    args: &[String],
) -> std::result::Result<AxonhubCliContract, clap::Error> {
    parse_command(axonhub_cli_command(), args)
}

fn parse_command<T>(mut command: Command, args: &[String]) -> std::result::Result<T, clap::Error>
where
    T: FromArgMatches,
{
    let matches = command.try_get_matches_from_mut(args)?;
    T::from_arg_matches(&matches)
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
