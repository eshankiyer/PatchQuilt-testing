use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let (Some(source_dir), Some(output_path)) = (args.next(), args.next()) else {
        eprintln!("usage: mc-index <decompiled-net/minecraft-dir> <output.json> [version]");
        return ExitCode::FAILURE;
    };
    let version = args.next().unwrap_or_else(|| "unknown".to_string());

    let index = match mc_index::build_index(&PathBuf::from(&source_dir), &version) {
        Ok(index) => index,
        Err(error) => {
            eprintln!("failed to walk {source_dir}: {error}");
            return ExitCode::FAILURE;
        }
    };

    println!(
        "indexed {} types across {} files",
        index.types.len(),
        index.file_count
    );

    let json = match serde_json::to_string_pretty(&index) {
        Ok(json) => json,
        Err(error) => {
            eprintln!("failed to serialize index: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = std::fs::write(&output_path, json) {
        eprintln!("failed to write {output_path}: {error}");
        return ExitCode::FAILURE;
    }
    println!("wrote {output_path}");
    ExitCode::SUCCESS
}
