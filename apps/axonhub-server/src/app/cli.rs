use std::io::IsTerminal;
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
    long_about = None,
    disable_help_subcommand = true
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
#[command(arg_required_else_help = true, disable_help_subcommand = true)]
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
    let format: PreviewFormat = args.format.into();
    let stdout_is_terminal = std::io::stdout().is_terminal();
    let loaded = load_for_cli().unwrap_or_else(|error| {
        println!("Failed to load config: {error}");
        process::exit(1);
    });
    let preview = loaded.preview(format).unwrap_or_else(|error| {
        println!("Failed to preview config: {error}");
        process::exit(1);
    });
    println!("{}", format_preview_for_terminal(&preview, format, stdout_is_terminal));

    Ok(())
}

fn print_help() -> Result<()> {
    let mut command = axonhub_cli_command();
    command.print_long_help()?;
    println!();
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
        .disable_help_subcommand(true)
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
        ConfigKey::DbDsn => Value::from(config.db.dsn.clone()),
    }
}
pub(crate) fn parse_axonhub_cli(
    args: &[String],
) -> std::result::Result<AxonhubCliContract, clap::Error> {
    let normalized = normalize_help_aliases(args);
    parse_command(axonhub_cli_command(), &normalized)
}

fn normalize_help_aliases(args: &[String]) -> Vec<String> {
    match args {
        [bin, help] if help == "help" => vec![bin.clone(), "--help".to_owned()],
        [bin, config, help] if config == "config" && help == "help" => {
            vec![bin.clone(), config.clone(), "--help".to_owned()]
        }
        _ => args.to_vec(),
    }
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

fn format_preview_for_terminal(preview: &str, format: PreviewFormat, is_terminal: bool) -> String {
    match (format, is_terminal) {
        (PreviewFormat::Yaml, true) => highlight_yaml_preview(preview),
        _ => preview.to_owned(),
    }
}

fn highlight_yaml_preview(preview: &str) -> String {
    preview
        .lines()
        .map(highlight_yaml_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn highlight_yaml_line(line: &str) -> String {
    const RESET: &str = "\u{1b}[0m";
    const COMMENT: &str = "\u{1b}[90m";
    const KEY: &str = "\u{1b}[36m";
    const STRING: &str = "\u{1b}[32m";
    const NUMBER: &str = "\u{1b}[33m";
    const BOOLEAN: &str = "\u{1b}[35m";

    fn highlight_scalar(value: &str) -> String {
        const RESET: &str = "\u{1b}[0m";
        const COMMENT: &str = "\u{1b}[90m";
        const STRING: &str = "\u{1b}[32m";
        const NUMBER: &str = "\u{1b}[33m";
        const BOOLEAN: &str = "\u{1b}[35m";

        let trimmed = value.trim();
        if trimmed.is_empty() {
            return value.to_owned();
        }

        let leading_len = value.len() - value.trim_start().len();
        let trailing_len = value.len() - value.trim_end().len();
        let leading = &value[..leading_len];
        let trailing = &value[value.len() - trailing_len..];
        let core = value.trim();

        let color = if core.starts_with('#') {
            COMMENT
        } else if matches!(core, "true" | "false" | "null" | "~") {
            BOOLEAN
        } else if core.parse::<i64>().is_ok() || core.parse::<f64>().is_ok() {
            NUMBER
        } else {
            STRING
        };

        format!("{leading}{color}{core}{RESET}{trailing}")
    }

    if line.is_empty() {
        return String::new();
    }

    let trimmed = line.trim_start();
    let indent = &line[..line.len() - trimmed.len()];

    if trimmed.starts_with('#') {
        return format!("{indent}{COMMENT}{trimmed}{RESET}");
    }

    let (prefix, body) = match trimmed.strip_prefix("- ") {
        Some(rest) => ("- ", rest),
        None => ("", trimmed),
    };

    if let Some(separator) = yaml_key_separator_index(body) {
        let key = &body[..separator];
        let remainder = &body[separator + 1..];
        return format!(
            "{indent}{prefix}{KEY}{key}{RESET}:{}",
            highlight_scalar(remainder)
        );
    }

    format!("{indent}{prefix}{}", highlight_scalar(body))
}

fn yaml_key_separator_index(value: &str) -> Option<usize> {
    value.char_indices().find_map(|(index, ch)| {
        if ch != ':' {
            return None;
        }

        let next = value[index + ch.len_utf8()..].chars().next();
        match next {
            None => Some(index),
            Some(next) if next.is_whitespace() => Some(index),
            _ => None,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> AxonhubCliContract {
        let args = args.iter().map(|value| value.to_string()).collect::<Vec<_>>();
        parse_axonhub_cli(&args).expect("cli should parse")
    }

    #[test]
    fn parse_help_subcommand_alias_returns_display_help_error() {
        let args = ["axonhub", "help"]
            .into_iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>();
        let error = parse_axonhub_cli(&args).expect_err("help should stop with display help");

        assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    }

    #[test]
    fn parse_help_flag_returns_display_help_error() {
        let args = ["axonhub", "--help"]
            .into_iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>();
        let error = parse_axonhub_cli(&args).expect_err("--help should stop with display help");

        assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    }

    #[test]
    fn parse_config_help_subcommand_alias_returns_display_help_error() {
        let args = ["axonhub", "config", "help"]
            .into_iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>();
        let error = parse_axonhub_cli(&args).expect_err("config help should stop with display help");

        assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    }

    #[test]
    fn cli_command_passes_clap_debug_assertions() {
        axonhub_cli_command().debug_assert();
        axonhub_config_cli_command().debug_assert();
    }

    #[test]
    fn yaml_preview_output_is_highlighted_for_terminal() {
        let output = format_preview_for_terminal(
            "server:\n  port: 8090\n  debug: false\n  name: AxonHub",
            PreviewFormat::Yaml,
            true,
        );

        assert!(output.contains("\u{1b}[36mserver\u{1b}[0m:"));
        assert!(output.contains("\u{1b}[33m8090\u{1b}[0m"));
        assert!(output.contains("\u{1b}[35mfalse\u{1b}[0m"));
        assert!(output.contains("\u{1b}[32mAxonHub\u{1b}[0m"));
    }

    #[test]
    fn yaml_preview_output_remains_plain_text_when_not_terminal() {
        let output = format_preview_for_terminal(
            "server:\n  port: 8090\n  debug: false\n  name: AxonHub",
            PreviewFormat::Yaml,
            false,
        );
        assert!(!output.contains("\u{1b}["));
    }

    #[test]
    fn json_preview_output_remains_plain_text() {
        let output = format_preview_for_terminal(
            "{\n  \"server\": {\n    \"port\": 8090\n  }\n}",
            PreviewFormat::Json,
            true,
        );
        assert!(!output.contains("\u{1b}["));
    }
}
