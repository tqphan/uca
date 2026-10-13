// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `uca-replay`: feeds a `.ucarec` recording into the engine with a fake clock
//! and prints its decisions, or compares them with an expected file (11).
//!
//! `uca-replay <file.ucarec> --config <config dir> [--profile <id>] [--expect <file>]`

use std::path::PathBuf;
use std::process::ExitCode;

use uca_app::recording::{first_difference, replay};
use uca_config::store::{ConfigStore, ConfigUpdate, ProfilePaths};

const USAGE: &str =
    "usage: uca-replay <file.ucarec> --config <config dir> [--profile <id>] [--expect <file>]";

struct Args {
    recording: PathBuf,
    config: PathBuf,
    profile: Option<String>,
    expect: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut recording = None;
    let mut config = None;
    let mut profile = None;
    let mut expect = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(a) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        match a.to_str() {
            Some("--config") => config = Some(PathBuf::from(value("--config")?)),
            Some("--profile") => profile = Some(value("--profile")?.to_string_lossy().into_owned()),
            Some("--expect") => expect = Some(PathBuf::from(value("--expect")?)),
            Some(flag) if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            _ if recording.is_none() => recording = Some(PathBuf::from(a)),
            _ => return Err("only one recording at a time".into()),
        }
    }
    Ok(Args {
        recording: recording.ok_or("no recording given")?,
        config: config.ok_or("--config is required")?,
        profile,
        expect,
    })
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let text = match std::fs::read_to_string(&args.recording) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("cannot read {}: {e}", args.recording.display());
            return ExitCode::from(2);
        }
    };
    // The profile id comes from the header unless given.
    let profile_id = match args.profile.clone().or_else(|| header_profile(&text)) {
        Some(p) if !p.is_empty() => p,
        _ => {
            eprintln!("the recording names no profile; pass --profile <id>");
            return ExitCode::from(2);
        }
    };
    let paths = ProfilePaths {
        config_dir: args.config.clone(),
        profile_id,
    };
    let config = match ConfigStore::load(paths).1 {
        ConfigUpdate::Applied(c) => c,
        ConfigUpdate::Rejected(issues) => {
            for i in issues {
                eprintln!("config: {i}");
            }
            return ExitCode::from(2);
        }
    };
    let replayed = match replay(&text, config) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    for w in &replayed.warnings {
        eprintln!("warning: {w}");
    }
    let Some(expect) = args.expect else {
        for line in &replayed.output {
            println!("{line}");
        }
        return ExitCode::SUCCESS;
    };
    let expected: Vec<String> = match std::fs::read_to_string(&expect) {
        Ok(t) => t.lines().map(str::to_owned).collect(),
        Err(e) => {
            eprintln!("cannot read {}: {e}", expect.display());
            return ExitCode::from(2);
        }
    };
    match first_difference(&replayed.output, &expected) {
        None => {
            println!("same as {} ({} lines)", expect.display(), expected.len());
            ExitCode::SUCCESS
        }
        Some(diff) => {
            println!("{diff}");
            ExitCode::FAILURE
        }
    }
}

fn header_profile(text: &str) -> Option<String> {
    let first = text.lines().next()?;
    let v: serde_json::Value = serde_json::from_str(first).ok()?;
    v["profile_id"].as_str().map(str::to_owned)
}
