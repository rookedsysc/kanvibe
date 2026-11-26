//! 원격 바이너리 진입점. 부트스트랩 loader가 `<mode> <namespace> <buildHash>`로 실행한다.

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let exit_code = kanvibe_remote_agent::run_command_line(&arguments);
    std::process::exit(exit_code);
}
