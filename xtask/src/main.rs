//! 저장소 작업 도구. `cargo xtask <명령>`으로 실행한다.

mod parity;

use std::process::ExitCode;

const USAGE: &str = "사용법: cargo xtask parity check [--area <area>]... [--list-unmapped]";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let argument_refs: Vec<&str> = arguments.iter().map(String::as_str).collect();

    let outcome = match argument_refs.as_slice() {
        ["parity", "check", options @ ..] => {
            parity::CheckOptions::parse(options).and_then(|options| parity::run_check(&options))
        }
        _ => Err(USAGE.to_string()),
    };

    match outcome {
        Ok(is_passing) if is_passing => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}
