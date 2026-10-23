use fuzz_common::pool_general::{replay, RegressionSeed, StepReport};
use serde::Serialize;
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

#[derive(Serialize)]
struct SeedResult<'a> {
    seed: &'a str,
    status: &'static str,
    steps: Option<[StepReport; 10]>,
    error: Option<String>,
}

fn seed_paths(path: &Path) -> Result<Vec<PathBuf>, String> {
    if path.is_file() {
        return Ok(vec![path.to_owned()]);
    }
    let mut paths = fs::read_dir(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    paths.retain(|path| {
        path.extension()
            .is_some_and(|extension| extension == "json")
    });
    paths.sort();
    if paths.is_empty() {
        return Err(format!("{}: no JSON regression seeds", path.display()));
    }
    Ok(paths)
}

fn run_seed(path: &Path) -> Result<[StepReport; 10], String> {
    let data = fs::read(path).map_err(|e| e.to_string())?;
    let seed: RegressionSeed = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
    match std::panic::catch_unwind(|| replay(&seed)) {
        Ok(result) => result,
        Err(panic) => Err(panic
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| panic.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unexpected panic (non-string payload)".to_string())),
    }
}

fn main() -> std::process::ExitCode {
    let mut args = std::env::args_os().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: replay_pool_general <seed.json|seed-directory>");
        return std::process::ExitCode::FAILURE;
    };
    if args.next().is_some() {
        eprintln!("usage: replay_pool_general <seed.json|seed-directory>");
        return std::process::ExitCode::FAILURE;
    }
    let paths = match seed_paths(Path::new(&path)) {
        Ok(paths) => paths,
        Err(error) => {
            eprintln!("{error}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    let mut failed = false;
    for path in paths {
        let name = path.display().to_string();
        let report = match run_seed(&path) {
            Ok(steps) => SeedResult {
                seed: &name,
                status: "passed",
                steps: Some(steps),
                error: None,
            },
            Err(error) => {
                eprintln!("{name}: {error}");
                failed = true;
                SeedResult {
                    seed: &name,
                    status: "failed",
                    steps: None,
                    error: Some(error),
                }
            }
        };
        if let Err(error) = serde_json::to_writer(&mut stdout, &report)
            .map_err(io::Error::other)
            .and_then(|_| writeln!(stdout))
        {
            eprintln!("writing replay report: {error}");
            return std::process::ExitCode::FAILURE;
        }
    }
    if failed {
        std::process::ExitCode::FAILURE
    } else {
        std::process::ExitCode::SUCCESS
    }
}
