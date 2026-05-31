mod args;
mod commands;

fn main() -> std::process::ExitCode {
    commands::run_from_env()
}
