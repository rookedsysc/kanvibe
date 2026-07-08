use std::{env, error::Error, fs, path::PathBuf};

use kanvibe_i18n::Locale;

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let args = env::args().skip(1).collect::<Vec<_>>();

    if args.first().is_some_and(|arg| arg == "readonly-board") {
        let repo_root = flag_value(&args, "--repo-root")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".."));
        let locale = flag_value(&args, "--locale")
            .and_then(Locale::parse)
            .unwrap_or(Locale::En);
        let report = qa_harness::read_only_board_report(repo_root, locale)?;
        let report_json = serde_json::to_string_pretty(&report)?;

        if let Some(output) = flag_value(&args, "--output").map(PathBuf::from) {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(output, report_json)?;
        } else {
            println!("{report_json}");
        }

        return Ok(());
    }

    if args.first().is_some_and(|arg| arg == "board-interactions") {
        let repo_root = flag_value(&args, "--repo-root")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".."));
        let report = qa_harness::board_interaction_report(repo_root)?;
        let report_json = serde_json::to_string_pretty(&report)?;

        if let Some(output) = flag_value(&args, "--output").map(PathBuf::from) {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(output, report_json)?;
        } else {
            println!("{report_json}");
        }

        return Ok(());
    }

    if args.first().is_some_and(|arg| arg == "task-detail") {
        let repo_root = flag_value(&args, "--repo-root")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".."));
        let report = qa_harness::task_detail_report(repo_root)?;
        let report_json = serde_json::to_string_pretty(&report)?;

        if let Some(output) = flag_value(&args, "--output").map(PathBuf::from) {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(output, report_json)?;
        } else {
            println!("{report_json}");
        }

        return Ok(());
    }

    if args.first().is_some_and(|arg| arg == "git-diff") {
        let repo_root = flag_value(&args, "--repo-root")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".."));
        let report = qa_harness::git_diff_report(repo_root)?;
        let report_json = serde_json::to_string_pretty(&report)?;

        if let Some(output) = flag_value(&args, "--output").map(PathBuf::from) {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(output, report_json)?;
        } else {
            println!("{report_json}");
        }

        return Ok(());
    }

    if args.first().is_some_and(|arg| arg == "notifications-hooks") {
        let repo_root = flag_value(&args, "--repo-root")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".."));
        let report = qa_harness::notification_hooks_report(repo_root)?;
        let report_json = serde_json::to_string_pretty(&report)?;

        if let Some(output) = flag_value(&args, "--output").map(PathBuf::from) {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(output, report_json)?;
        } else {
            println!("{report_json}");
        }

        return Ok(());
    }

    if args
        .first()
        .is_some_and(|arg| arg == "settings-layout-remote")
    {
        let repo_root = flag_value(&args, "--repo-root")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".."));
        let report = qa_harness::settings_layout_remote_report(repo_root)?;
        let report_json = serde_json::to_string_pretty(&report)?;

        if let Some(output) = flag_value(&args, "--output").map(PathBuf::from) {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(output, report_json)?;
        } else {
            println!("{report_json}");
        }

        return Ok(());
    }

    if args.first().is_some_and(|arg| arg == "full-parity") {
        let repo_root = flag_value(&args, "--repo-root")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".."));
        let report = qa_harness::full_parity_report(&repo_root)?;
        let report_json = serde_json::to_string_pretty(&report)?;

        if let Some(output_dir) = flag_value(&args, "--output-dir").map(PathBuf::from) {
            fs::create_dir_all(&output_dir)?;
            fs::write(output_dir.join("full-parity.json"), &report_json)?;
            fs::write(
                output_dir.join("QA_REPORT.md"),
                qa_harness::full_parity_markdown(&report),
            )?;
        } else if let Some(output) = flag_value(&args, "--output").map(PathBuf::from) {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(output, report_json)?;
        } else {
            println!("{report_json}");
        }

        return Ok(());
    }

    if args.first().is_some_and(|arg| arg == "qa-control") {
        let repo_root = flag_value(&args, "--repo-root")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".."));
        let report = qa_harness::scenario_control_protocol_report(repo_root)?;
        let report_json = serde_json::to_string_pretty(&report)?;

        if let Some(output) = flag_value(&args, "--output").map(PathBuf::from) {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(output, report_json)?;
        } else {
            println!("{report_json}");
        }

        return Ok(());
    }

    if args.first().is_some_and(|arg| arg == "qa-replay-plan") {
        let repo_root = flag_value(&args, "--repo-root")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".."));
        let report = qa_harness::qa_control_replay_plan_report(repo_root)?;
        let report_json = serde_json::to_string_pretty(&report)?;

        if let Some(output) = flag_value(&args, "--output").map(PathBuf::from) {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(output, report_json)?;
        } else {
            println!("{report_json}");
        }

        return Ok(());
    }

    if args.first().is_some_and(|arg| arg == "qa-replay-execute") {
        let repo_root = flag_value(&args, "--repo-root")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".."));
        let report = if let Some(socket_path) = flag_value(&args, "--socket").map(PathBuf::from) {
            qa_harness::qa_control_replay_execution_report(repo_root, socket_path)?
        } else {
            qa_harness::qa_control_replay_smoke_report(repo_root)?
        };
        let report_json = serde_json::to_string_pretty(&report)?;

        if let Some(output) = flag_value(&args, "--output").map(PathBuf::from) {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(output, report_json)?;
        } else {
            println!("{report_json}");
        }

        return Ok(());
    }

    if args.first().is_some_and(|arg| arg == "qa-app-launch") {
        let repo_root = flag_value(&args, "--repo-root")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".."));
        let app_binary = flag_value(&args, "--app-binary").map(PathBuf::from);
        let report = qa_harness::native_app_launch_report(repo_root, app_binary)?;
        let report_json = serde_json::to_string_pretty(&report)?;

        if let Some(output) = flag_value(&args, "--output").map(PathBuf::from) {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(output, report_json)?;
        } else {
            println!("{report_json}");
        }

        return Ok(());
    }

    if args.first().is_some_and(|arg| arg == "qa-app-replay") {
        let repo_root = flag_value(&args, "--repo-root")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".."));
        let app_binary = flag_value(&args, "--app-binary").map(PathBuf::from);
        let report = qa_harness::native_app_replay_report(repo_root, app_binary)?;
        let report_json = serde_json::to_string_pretty(&report)?;

        if let Some(output) = flag_value(&args, "--output").map(PathBuf::from) {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(output, report_json)?;
        } else {
            println!("{report_json}");
        }

        return Ok(());
    }

    if args
        .first()
        .is_some_and(|arg| arg == "native-visual-parity")
    {
        let repo_root = flag_value(&args, "--repo-root")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".."));
        let artifact_root = flag_value(&args, "--artifact-root").map(PathBuf::from);
        let report = qa_harness::native_visual_parity_report(repo_root, artifact_root)?;
        let report_json = serde_json::to_string_pretty(&report)?;

        if let Some(output) = flag_value(&args, "--output").map(PathBuf::from) {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(output, report_json)?;
        } else {
            println!("{report_json}");
        }

        return Ok(());
    }

    if args.first().is_some_and(|arg| arg == "native-performance") {
        let repo_root = flag_value(&args, "--repo-root")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".."));
        let app_bundle = flag_value(&args, "--app-bundle").map(PathBuf::from);
        let dmg = flag_value(&args, "--dmg").map(PathBuf::from);
        let release_binary = flag_value(&args, "--release-binary").map(PathBuf::from);
        let report =
            qa_harness::native_performance_report(repo_root, app_bundle, dmg, release_binary)?;
        let report_json = serde_json::to_string_pretty(&report)?;

        if let Some(output) = flag_value(&args, "--output").map(PathBuf::from) {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(output, report_json)?;
        } else {
            println!("{report_json}");
        }

        return Ok(());
    }

    println!(
        "qa-harness scaffold: {} scenarios in {}",
        qa_harness::scenario_count(),
        qa_harness::SCENARIO_DIR_FROM_REPO_ROOT
    );

    Ok(())
}

fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].as_str())
}
