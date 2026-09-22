use fuzz_common::{run, Mode, RunReport, Target};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

#[derive(Clone, Copy)]
enum ReplayMode {
    Native,
    Wasm,
    Both,
}

fn main() {
    let mut target = None;
    let mut mode = None;
    let mut input = None;
    let mut seeds = false;
    let mut args = env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--target" => target = Some(required_value(&mut args, "--target")),
            "--mode" => mode = Some(required_value(&mut args, "--mode")),
            "--input" => input = Some(PathBuf::from(required_value(&mut args, "--input"))),
            "--seeds" => seeds = true,
            _ => panic!("unknown argument: {argument}"),
        }
    }

    let target = Target::from_str(target.as_deref().expect("--target is required"))
        .unwrap_or_else(|error| panic!("{error}"));
    let mode = match mode.as_deref().expect("--mode is required") {
        "native" => ReplayMode::Native,
        "wasm" => ReplayMode::Wasm,
        "both" => ReplayMode::Both,
        value => panic!("unknown replay mode: {value}"),
    };
    assert!(
        seeds ^ input.is_some(),
        "choose exactly one of --seeds or --input"
    );

    let inputs = if let Some(path) = input {
        vec![path]
    } else {
        seed_files(target)
    };
    assert!(!inputs.is_empty(), "no replay inputs found for {target}");

    for path in inputs {
        let bytes = fs::read(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        match mode {
            ReplayMode::Native => print_report(&path, "native", run(target, &bytes, Mode::Native)),
            ReplayMode::Wasm => print_report(&path, "wasm", run(target, &bytes, Mode::Wasm)),
            ReplayMode::Both => {
                let native = run(target, &bytes, Mode::Native);
                let wasm = run(target, &bytes, Mode::Wasm);
                assert_eq!(
                    native,
                    wasm,
                    "native/Wasm divergence for {} and {}",
                    target,
                    path.display()
                );
                print_report(&path, "native=wasm", native);
            }
        }
    }
}

fn required_value(args: &mut impl Iterator<Item = String>, flag: &str) -> String {
    args.next()
        .unwrap_or_else(|| panic!("{flag} requires a value"))
}

fn seed_files(target: Target) -> Vec<PathBuf> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("seeds")
        .join(target.as_str());
    let mut files: Vec<_> = fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()))
        .map(|entry| entry.expect("failed to read seed directory entry").path())
        .filter(|path| path.is_file())
        .collect();
    files.sort();
    files
}

fn print_report(path: &Path, mode: &str, report: RunReport) {
    println!(
        "{} {} input={:?} operations={} applied={} rejected={} noops={} state={:016x}",
        path.display(),
        mode,
        report.input,
        report.operations,
        report.applied,
        report.rejected,
        report.noops,
        report.state
    );
}
