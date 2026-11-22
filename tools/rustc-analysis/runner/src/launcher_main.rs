use std::{env, path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).map(PathBuf::from).collect();
    let [output, source_root, launcher_lock] = args.as_slice() else {
        eprintln!("Usage: bevy-cicd-schedule-launcher OUTPUT SOURCE_ROOT CARGO_LOCK");
        return ExitCode::FAILURE;
    };
    let mut app = caller::build_app();
    if let Err(error) =
        bevy_schedule_export::launcher::dump_schedules(&mut app, output, source_root, launcher_lock)
    {
        eprintln!("Schedule export failed: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
