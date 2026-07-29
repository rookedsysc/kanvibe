fn main() {
    kanvibe_app::install_native_panic_hook();
    if std::env::args().any(|argument| argument == "--rollback-electron-db") {
        let result = kanvibe_app::NativeUiLaunchConfig::from_env().and_then(|config| {
            kanvibe_app::rollback_native_database_to_electron_backup(&config.database_path)
                .map(|safety_path| (config.database_path, safety_path))
        });
        match result {
            Ok((database_path, safety_path)) => {
                eprintln!(
                    "restored Electron database at `{}`; preserved native snapshot at `{}`",
                    database_path.display(),
                    safety_path.display()
                );
                return;
            }
            Err(error) => {
                eprintln!("KanVibe database rollback failed: {error}");
                std::process::exit(1);
            }
        }
    }

    match kanvibe_app::run() {
        Ok(mode) => eprintln!("kanvibe native scaffold mode: {mode:?}"),
        Err(error) => {
            kanvibe_app::append_native_crash_log(&kanvibe_app::native_diagnostic_line(
                "fatal-error",
                "startup",
                &error.to_string(),
                None,
            ));
            eprintln!("kanvibe native failed: {error}");
            std::process::exit(1);
        }
    }
}
