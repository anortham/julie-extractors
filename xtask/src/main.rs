fn main() -> std::process::ExitCode {
    xtask::test_tiers::run_from_env_args(std::env::args_os())
}
