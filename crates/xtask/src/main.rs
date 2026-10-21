mod bridge_task;
mod content_task;
mod generation;

use std::{io::Read, process::ExitCode};

const USAGE: &str = "Usage: cargo run -p xtask -- <generate|check-generated|verify-wire|generate-content|check-content|bake-content|character-trace|generate-bridge|check-bridge>";

fn run(args: &[String]) -> Result<(), String> {
    if args.len() != 1 {
        return Err(USAGE.into());
    }
    match args[0].as_str() {
        "generate-bridge" => bridge_task::run(false),
        "check-bridge" => bridge_task::run(true),
        "generate" => generation::generate(),
        "check-generated" => generation::check(),
        "verify-wire" => {
            let mut input = String::new();
            std::io::stdin()
                .take(8 * 1024 * 1024 + 1)
                .read_to_string(&mut input)
                .map_err(|e| e.to_string())?;
            if input.len() > 8 * 1024 * 1024 {
                return Err("fixture input limit".into());
            }
            let fixtures: Vec<protocol::fixtures::Fixture> =
                serde_json::from_str(&input).map_err(|e| e.to_string())?;
            let results: Vec<_> = fixtures.iter().map(|fixture| {
                match protocol::fixtures::decoded_fixture(fixture) {
                    Ok(value) => serde_json::json!({"name":fixture.name,"ok":true,"value":value}),
                    Err(error) => serde_json::json!({"name":fixture.name,"ok":false,"error":error.to_string()}),
                }
            }).collect();
            println!(
                "{}",
                serde_json::to_string(&results).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        "character-trace" => {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/simulation/character-trace.json");
            let trace: sim_core::trace::CharacterTrace =
                serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            let report = sim_core::trace::verify(&trace, 0.0001)?;
            println!(
                "Native character trace PASS: {} ticks (60s), {} checkpoints; max position error {:.6}m",
                report.ticks, report.checkpoints, report.max_position_error
            );
            Ok(())
        }
        "bake-content" => content_task::bake(),
        "generate-content" => content_task::generate(),
        "check-content" => content_task::check(),
        "help" | "--help" | "-h" => {
            println!("{USAGE}");
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}

fn main() -> ExitCode {
    match run(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(if error == USAGE { 2 } else { 1 })
        }
    }
}
