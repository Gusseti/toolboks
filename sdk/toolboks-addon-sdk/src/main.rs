use std::{env, fs, process::ExitCode};
use toolboks_addon_sdk::Package;

/// Design checklist, kept in step with docs/ADDON-DESIGN-CHECKLIST.md and DESIGN.md.
const CHECKLIST: &str = "Design checklist (see DESIGN.md and docs/ADDON-DESIGN-CHECKLIST.md):
  - Tokens only: no own palette, font, corner radius or icon style.
  - Sidebar entry, breadcrumb and page names in sentence case, one or two words.
  - Plain wording; label every estimate as an estimate.
  - One primary button per view; other buttons are outlined.
  - Keyboard focus is visible and every control is reachable by keyboard.
  - Text contrast is at least 4.5:1; colour is never the only signal.
  - Changes to the system show a preview, ask once to confirm and offer undo.
  - Nothing is written to disk except through the host's local preferences.";

fn main() -> ExitCode {
    let args: Vec<_> = env::args().collect();
    if args.get(1).map(String::as_str) == Some("checklist") {
        println!("{CHECKLIST}");
        return ExitCode::SUCCESS;
    }
    let result = (|| -> Result<(), String> {
        if args.len() < 3 || !["validate", "pack"].contains(&args[1].as_str()) {
            return Err(format!("Usage: tbaddon validate <package.json> | tbaddon pack <package.json> <output.tbaddon> | tbaddon checklist

{CHECKLIST}"));
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
        println!(
            "
{CHECKLIST}"
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
