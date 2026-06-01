mod args;
mod commands;
mod discovery;
mod extraction;
mod paths;

fn main() -> std::process::ExitCode {
    commands::run_from_env()
}
