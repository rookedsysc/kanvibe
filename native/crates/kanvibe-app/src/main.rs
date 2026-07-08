fn main() {
    match kanvibe_app::run() {
        Ok(mode) => eprintln!("kanvibe native scaffold mode: {mode:?}"),
        Err(error) => {
            eprintln!("kanvibe native failed: {error}");
            std::process::exit(1);
        }
    }
}
