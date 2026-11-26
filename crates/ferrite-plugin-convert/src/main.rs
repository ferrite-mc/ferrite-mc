//! CLI for inspecting Paper/Spigot/Folia `.jar` compatibility with Ferrite and generating
//! Rust native plugin scaffolds.

use std::path::PathBuf;
use std::process::ExitCode;

fn print_usage() {
    eprintln!(
        "Usage:\n  ferrite-plugin-convert inspect <plugin.jar> [--json]\n  ferrite-plugin-convert scaffold <plugin.jar> --out <dir>"
    );
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        print_usage();
        return ExitCode::FAILURE;
    }

    match args[0].as_str() {
        "inspect" => {
            let jar = PathBuf::from(&args[1]);
            let as_json = args.iter().any(|a| a == "--json");
            match ferrite_plugin_convert::inspect_jar(&jar) {
                Ok((_, _, report)) => {
                    if as_json {
                        println!("{}", serde_json::to_string_pretty(&report).unwrap());
                    } else {
                        println!(
                            "Plugin: {} v{} (main={}, folia={})",
                            report.plugin_name,
                            report.plugin_version,
                            report.main_class,
                            report.folia_supported
                        );
                        println!("Classes scanned: {}", report.class_files_scanned);
                        println!("Verdict: {:?}", report.verdict);
                        println!("Commands: {:?}", report.commands);
                        println!("Events: {}", report.detected_events.len());
                        println!("Supported APIs: {}", report.supported_apis.len());
                        println!("Unverified APIs: {}", report.unverified_apis.len());
                        println!("Blockers: {}", report.blockers.len());
                        for b in &report.blockers {
                            println!("  [BLOCKER] {b}");
                        }
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("error: {error}");
                    ExitCode::FAILURE
                }
            }
        }
        "scaffold" => {
            let jar = PathBuf::from(&args[1]);
            let out_pos = args.iter().position(|a| a == "--out");
            let Some(out_dir) = out_pos.and_then(|i| args.get(i + 1)).map(PathBuf::from) else {
                print_usage();
                return ExitCode::FAILURE;
            };
            match ferrite_plugin_convert::inspect_jar(&jar).and_then(|(meta, config, report)| {
                ferrite_plugin_convert::generate_scaffold(
                    &meta,
                    config.as_deref(),
                    &report,
                    &out_dir,
                )?;
                Ok(report)
            }) {
                Ok(report) => {
                    println!(
                        "Generated Rust native scaffold for {} at {} (verdict={:?})",
                        report.plugin_name,
                        out_dir.display(),
                        report.verdict
                    );
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("error: {error}");
                    ExitCode::FAILURE
                }
            }
        }
        _ => {
            print_usage();
            ExitCode::FAILURE
        }
    }
}
