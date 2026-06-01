use std::ffi::OsString;
use std::process::ExitCode;

pub fn run_from_env_args(args: impl IntoIterator<Item = OsString>) -> ExitCode {
    let args = args
        .into_iter()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    if args == ["test", "list"] {
        for name in crate::test_tiers::tier_names() {
            println!("{name}");
        }
        return ExitCode::SUCCESS;
    }
    if args == ["release", "package-list"] {
        print!("{}", crate::release::render_release_package_list());
        return ExitCode::SUCCESS;
    }
    if args.first().map(String::as_str) == Some("release")
        && args.get(1).map(String::as_str) == Some("package")
    {
        return crate::release::run_package_from_args(&args[1..]);
    }
    if args.first().map(String::as_str) == Some("dogfood") {
        return crate::dogfood::run_from_args(&args[1..]);
    }
    if args.first().map(String::as_str) == Some("performance") {
        return crate::performance::run_from_args(&args[1..]);
    }

    match crate::test_tiers::plan_from_args(args) {
        Ok(plan) => crate::test_tiers::run_plan(plan),
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(2)
        }
    }
}
