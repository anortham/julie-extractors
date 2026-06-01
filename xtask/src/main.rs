fn main() -> std::process::ExitCode {
    xtask::commands::run_from_env_args(std::env::args_os())
}
