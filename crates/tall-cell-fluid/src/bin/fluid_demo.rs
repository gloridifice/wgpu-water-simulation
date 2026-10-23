fn main() -> std::process::ExitCode {
    match tall_cell_fluid::demo::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("fluid-demo: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
