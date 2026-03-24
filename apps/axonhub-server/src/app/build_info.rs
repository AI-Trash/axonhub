use std::env;
use std::fmt::{self, Display, Formatter};
use std::sync::OnceLock;
use std::time::Instant;

static START_TIME: OnceLock<Instant> = OnceLock::new();

const VERSION: &str = include_str!("../../../../internal/build/VERSION");
const BUILD_COMMIT: Option<&str> = option_env!("AXONHUB_BUILD_COMMIT");
const BUILD_TIME: Option<&str> = option_env!("AXONHUB_BUILD_TIME");
const GO_VERSION: Option<&str> = option_env!("AXONHUB_BUILD_GO_VERSION");
const RUST_VERSION: Option<&str> = option_env!("AXONHUB_BUILD_RUST_VERSION");
const GO_VERSION_FALLBACK: &str = "n/a (Rust build)";

pub(crate) fn version() -> &'static str {
    VERSION.trim()
}

pub(crate) fn show_version() {
    println!("{}", version());
}

pub(crate) fn show_build_info() {
    println!("{}", BuildInfo::current());
}

pub(crate) struct BuildInfo {
    version: &'static str,
    commit: Option<&'static str>,
    build_time: Option<&'static str>,
    go_version: Option<&'static str>,
    rust_version: Option<&'static str>,
    platform: String,
    uptime: String,
}

impl BuildInfo {
    pub(crate) fn current() -> Self {
        Self {
            version: version(),
            commit: BUILD_COMMIT,
            build_time: BUILD_TIME,
            go_version: GO_VERSION,
            rust_version: RUST_VERSION,
            platform: format!("{}/{}", env::consts::OS, env::consts::ARCH),
            uptime: humantime::format_duration(start_time().elapsed()).to_string(),
        }
    }
}

fn start_time() -> &'static Instant {
    START_TIME.get_or_init(Instant::now)
}

impl Display for BuildInfo {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "Version: {}", self.version)?;

        if let Some(commit) = self.commit.filter(|value| !value.is_empty()) {
            writeln!(formatter, "Commit: {commit}")?;
        }

        if let Some(build_time) = self.build_time.filter(|value| !value.is_empty()) {
            writeln!(formatter, "Build Time: {build_time}")?;
        }

        writeln!(
            formatter,
            "Go Version: {}",
            self.go_version
                .filter(|value| !value.is_empty())
                .unwrap_or(GO_VERSION_FALLBACK)
        )?;

        if let Some(rust_version) = self.rust_version.filter(|value| !value.is_empty()) {
            writeln!(formatter, "Rust Version: {rust_version}")?;
        }

        writeln!(formatter, "Platform: {}", self.platform)?;
        write!(formatter, "Uptime: {}", self.uptime)
    }
}
