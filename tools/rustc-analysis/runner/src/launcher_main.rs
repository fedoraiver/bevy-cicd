use std::{env, path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).map(PathBuf::from).collect();
    let [output, source_root, launcher_lock, inputs @ ..] = args.as_slice() else {
        eprintln!(
            "Usage: bevy-cicd-schedule-launcher OUTPUT SOURCE_ROOT CARGO_LOCK [SOURCE_INPUTS]"
        );
        return ExitCode::FAILURE;
    };
    let mut app = caller::build_app();
    let result = match inputs {
        [] => bevy_schedule_export::launcher::dump_schedules(
            &mut app,
            output,
            source_root,
            launcher_lock,
        ),
        [filename] => std::fs::read(filename)
            .map_err(Into::into)
            .and_then(|bytes| serde_json::from_slice(&bytes).map_err(Into::into))
            .and_then(|files| {
                bevy_schedule_export::launcher::dump_workspace_schedules(
                    &mut app,
                    output,
                    source_root,
                    launcher_lock,
                    files,
                )
            }),
        _ => Err("Only one source input list is accepted".into()),
    };
    if let Err(error) = result {
        eprintln!("Schedule export failed: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
