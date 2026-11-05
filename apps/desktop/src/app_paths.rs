use std::fs::{self, OpenOptions};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use color_eyre::eyre::{Result, eyre};
use tracing_subscriber::Layer;
use tracing_subscriber::fmt::writer::MakeWriterExt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Whether this is a local build (`HEX_LOCAL_BUILD=1`): made from a checkout
/// for its owner's own machine, so it never checks for or offers updates.
pub fn is_local_build() -> bool {
    option_env!("HEX_LOCAL_BUILD") == Some("1")
}

/// Whether this is a macOS `WordInk Test.app` build (`HEX_TEST_BUILD=1`), which
/// keeps its data, settings, and user config apart from the installed WordInk.
pub fn is_macos_test_build() -> bool {
    cfg!(target_os = "macos") && option_env!("HEX_TEST_BUILD") == Some("1")
}

/// The name menus show, so a test build running beside Hex reads as itself.
#[cfg(target_os = "macos")]
pub fn app_name() -> &'static str {
    if is_macos_test_build() {
        "WordInk Test"
    } else {
        "WordInk"
    }
}

pub fn support_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("HEX_APPLICATION_SUPPORT_DIR") {
        return Ok(path.into());
    }
    let data =
        dirs::data_dir().ok_or_else(|| eyre!("application data directory is unavailable"))?;
    let folder = if is_macos_test_build() {
        "wordink-test"
    } else {
        "wordink"
    };
    Ok(data.join(folder))
}

pub fn logs_dir() -> Result<PathBuf> {
    Ok(support_dir()?.join("logs"))
}

/// The root of this binary's tracing targets. `module_path!()` begins with the
/// binary's crate name (`wordink`), not the Cargo package name (`hex`), so a
/// hard-coded `hex` prefix matches no event at all.
pub fn crate_root() -> &'static str {
    module_path!().split("::").next().unwrap_or("wordink")
}

/// Creates the logs directory, appends process diagnostics to `process.log`
/// alongside stderr, installs the global tracing subscriber, and routes Ctrl-C
/// to `shutdown`. Returns the logs directory.
pub fn init_process_logging(shutdown: &'static AtomicBool) -> Result<PathBuf> {
    let log_dir = logs_dir()?;
    fs::create_dir_all(&log_dir)?;
    let process_log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("process.log"))?;
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| {
            tracing_subscriber::EnvFilter::new(format!("{}=info", crate_root()))
        });
    // Process diagnostics keep their own filter; the debug log sees Hex's
    // events at its own level, and only while it is on.
    let process = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr.and(Mutex::new(process_log)))
        .with_filter(filter);
    let debug =
        crate::debug_log::BridgeLayer.with_filter(tracing_subscriber::filter::dynamic_filter_fn(
            |meta, _| crate::debug_log::bridge_wants(meta),
        ));
    tracing_subscriber::registry()
        .with(process)
        .with(debug)
        .init();
    log_panics();
    ctrlc::set_handler(|| shutdown.store(true, Ordering::Relaxed))?;
    Ok(log_dir)
}

/// Records every panic in `process.log` before the installed report runs.
/// Desktop apps have no console, so the default stderr report is otherwise
/// lost and a crashed worker thread leaves no trace.
fn log_panics() {
    let report = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!(
            thread = std::thread::current().name().unwrap_or("unnamed"),
            backtrace = %std::backtrace::Backtrace::force_capture(),
            "panic: {info}"
        );
        report(info);
    }));
}

pub fn opencode_workspace() -> Result<PathBuf> {
    Ok(support_dir()?.join("opencode"))
}

pub fn local_api_discovery_file() -> Result<PathBuf> {
    Ok(support_dir()?.join("local-api.json"))
}

pub fn personal_commands_status_file() -> Result<PathBuf> {
    Ok(support_dir()?.join("personal-commands.json"))
}

/// The config hash the person allowed to use `fetch` and run programs. It
/// lives here, not in the config folder, so a config cannot allow itself.
pub fn transform_trust_file() -> Result<PathBuf> {
    Ok(support_dir()?.join("transform-trust.json"))
}

pub fn personal_commands_workspace() -> Result<PathBuf> {
    user_config_dir()
}

/// The per-user config folder holding `hex.config.ts` on every OS: `~/.config/wordink`, or `~/.config/wordink-test` for `WordInk Test.app`.
fn user_config_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| eyre!("home directory is unavailable"))?;
    Ok(user_config_dir_in(&home, is_macos_test_build()))
}

fn user_config_dir_in(home: &std::path::Path, test_build: bool) -> PathBuf {
    home.join(".config").join(if test_build {
        "wordink-test"
    } else {
        "wordink"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_data_folder_is_wordink_not_hex() {
        if std::env::var_os("HEX_APPLICATION_SUPPORT_DIR").is_none() {
            let dir = support_dir().unwrap();
            assert_eq!(dir.file_name().unwrap(), "wordink");
        }
    }

    #[test]
    fn each_build_has_its_own_config_folder() {
        let home = std::path::Path::new("/Users/kit");
        assert_eq!(
            user_config_dir_in(home, false),
            home.join(".config").join("wordink")
        );
        assert_eq!(
            user_config_dir_in(home, true),
            home.join(".config").join("wordink-test")
        );
    }
}
