#[cfg(target_os = "macos")]
fn main() {
    use std::{path::PathBuf, time::Duration};

    let mut arguments = std::env::args_os().skip(1);
    let mut journal = None::<PathBuf>;
    let mut old_pid = None::<u32>;
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--journal") => journal = arguments.next().map(PathBuf::from),
            Some("--old-pid") => {
                old_pid = arguments
                    .next()
                    .and_then(|value| value.to_str().and_then(|value| value.parse().ok()));
            }
            _ => {
                eprintln!("usage: kanvibe-updater --journal ABSOLUTE_PATH --old-pid PID");
                std::process::exit(64);
            }
        }
    }
    let Some(journal) = journal else {
        eprintln!("native update helper requires --journal");
        std::process::exit(64);
    };
    let Some(old_pid) = old_pid else {
        eprintln!("native update helper requires --old-pid");
        std::process::exit(64);
    };
    let health_timeout = std::env::var("KANVIBE_QA_UPDATE_HEALTH_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| (1_000..=30_000).contains(value))
        .map(Duration::from_millis)
        .unwrap_or_else(|| Duration::from_secs(30));
    if let Err(error) =
        kanvibe_app::native_updater::run_update_helper(journal, old_pid, health_timeout)
    {
        eprintln!("native update helper failed: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("KanVibe native update replacement requires macOS.");
    std::process::exit(78);
}
