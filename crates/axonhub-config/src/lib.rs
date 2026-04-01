mod contract;
mod env;
mod legacy;
mod loader;
mod preview;
mod types;
mod validation;

#[cfg(test)]
mod tests;

pub use contract::{
    supported_config_aliases, supported_config_keys, SupportedConfigAlias, SupportedConfigKey,
    SUPPORTED_DB_DIALECTS,
};
pub use loader::{config_search_paths, load, load_for_cli, LoadedConfig};
pub use preview::PreviewFormat;
pub use types::*;
