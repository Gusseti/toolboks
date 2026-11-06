use std::{env, fs, process::ExitCode};
use toolboks_addon_sdk::Package;
fn main() -> ExitCode {
    let args: Vec<_> = env::args().collect();
    let result = (|| -> Result<(), String> {
        if args.len() < 3 || !["validate", "pack"].contains(&args[1].as_str()) {
            return Err("Usage: tbaddon validate <package.json> | tbaddon pack <package.json> <output.tbaddon>".into());
        }
        let bytes = fs::read(&args[2]).map_err(|e| e.to_string())?;
        let package = Package::parse(&bytes, "0.8.2")?;
        if args[1] == "pack" {
            let out = args.get(3).ok_or("Provide the output .tbaddon path.")?;
            fs::write(
                out,
                serde_json::to_vec_pretty(&package).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        }
        println!(
            "Valid: {} {} · API {}",
            package.manifest.title, package.manifest.version, package.manifest.api_version
        );
        Ok(())
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
