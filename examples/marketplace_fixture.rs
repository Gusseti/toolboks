use std::{fs, path::PathBuf};
use toolboks_catalog::sha256_hex;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let folder = PathBuf::from(
        args.next()
            .ok_or("usage: marketplace_fixture <new folder> [native sample exe]")?,
    );
    fs::create_dir(&folder)?;
    let native = args.next();
    let version = args.next().unwrap_or_else(|| "1.0.0".into());
    if toolboks_addon_sdk::version(&version).is_none() {
        return Err("Expected x.y.z version".into());
    }
    let mut tools = [
        (
            "resource-monitor",
            "Resource Monitor",
            "Live graphs for CPU, GPU, memory, disk and network.",
            "System",
        ),
        (
            "memory-cleaner",
            "Memory Cleaner",
            "Free up memory without closing anything.",
            "System",
        ),
        (
            "disk-manager",
            "Disk Manager",
            "See what fills your drives and optimize them.",
            "Storage",
        ),
        (
            "event-watch",
            "Event Watch",
            "Windows events, explained in one line each.",
            "System",
        ),
        (
            "debloater",
            "Debloater",
            "Remove clutter from Windows, with undo for every change.",
            "System",
        ),
        (
            "system-profiles",
            "System Profiles",
            "Set Windows up for gaming, with a restore point first.",
            "System",
        ),
        (
            "aural",
            "Audio Gainer",
            "Boost a quiet microphone and route it to other apps.",
            "Audio",
        ),
        (
            "macro-manager",
            "Macro Manager",
            "Record, edit and replay keyboard and mouse actions.",
            "Automation",
        ),
    ];
    let order = [
        "memory-cleaner",
        "resource-monitor",
        "debloater",
        "system-profiles",
        "event-watch",
        "disk-manager",
        "macro-manager",
        "aural",
    ];
    tools.sort_by_key(|(slug, _, _, _)| order.iter().position(|item| item == slug).unwrap());
    let mut entries = vec![];
    for (slug, title, description, category) in tools {
        let id = format!("example.{slug}");
        let icon = match slug {
            "memory-cleaner" => "chip",
            "resource-monitor" => "pulse",
            "debloater" => "sliders",
            "system-profiles" => "gauge",
            "event-watch" => "shield",
            "disk-manager" => "drive",
            "macro-manager" => "cursor",
            "aural" => "mic",
            _ => "grid",
        };
        let mut package: serde_json::Value =
            serde_json::from_str(include_str!("../sdk/examples/hello-system.tbaddon"))?;
        package["manifest"]["id"] = id.clone().into();
        package["manifest"]["version"] = version.clone().into();
        package["manifest"]["title"] = title.to_owned().into();
        package["manifest"]["publisher"] = "Toolboks".into();
        package["manifest"]["description"] = "Local test package, not the real tool.".into();
        package["pages"][0]["widgets"][0]["text"] = format!("{title} is open in Toolboks").into();
        package["pages"][0]["widgets"][1]["text"] = "This harmless SDK test package proves first-run install and open. It is not the real tool.".into();
        if slug == "event-watch" {
            package["manifest"]["capabilities"] = serde_json::json!(["host_metrics"]);
            package["pages"].as_array_mut().unwrap().truncate(1);
        }
        let capabilities = package["manifest"]["capabilities"].clone();
        let bytes = serde_json::to_vec(&package)?;
        let filename = format!("{slug}.tbaddon");
        fs::write(folder.join(&filename), &bytes)?;
        let mut entry = serde_json::json!({
            "id": id, "title": title.to_owned(), "short_description": description,
            "long_description": "A harmless local SDK test package. This is not the published tool. It shows live memory readings and saves its own preferences inside Toolboks.",
            "publisher": "Toolboks", "category": category, "icon": icon, "accent": "#8b5cf6",
            "kind": "declarative", "version": version, "min_toolboks_version": "0.12.5",
            "protocol_version": toolboks_addon_proto::PROTOCOL_VERSION,
            "download_url": format!("https://github.com/Gusseti/toolboks-addons/releases/download/fixture/{filename}"),
            "size": bytes.len(), "sha256": sha256_hex(&bytes), "verified": true, "channel": "stable",
            "capabilities": capabilities, "pages": ["Overview", "Preferences"],
            "release_notes": "First local test version. No Windows changes."
        });
        if slug == "disk-manager" {
            entry["byline"] = "by stratum".into();
        }
        entries.push(entry);
    }
    if let Some(exe) = native {
        let bytes = fs::read(exe)?;
        fs::write(folder.join("hello-addon.exe"), &bytes)?;
        entries.push(serde_json::json!({
            "id": "example.hello", "title": "Native SDK sample", "short_description": "A separate program drawn inside Toolboks.",
            "publisher": "Example developer", "category": "Developer tools", "icon": "box", "accent": "#8b5cf6",
            "kind": "native", "version": "1.0.0", "min_toolboks_version": "0.12.5",
            "protocol_version": toolboks_addon_proto::PROTOCOL_VERSION,
            "download_url": "https://github.com/Gusseti/toolboks-addons/releases/download/fixture/hello-addon.exe",
            "size": bytes.len(), "sha256": sha256_hex(&bytes), "verified": false, "channel": "stable",
            "capabilities": ["local_preferences"], "pages": ["Overview", "Preferences"]
        }));
    }
    let index = serde_json::to_vec_pretty(&serde_json::json!({
        "format_version": 1, "entries": entries
    }))?;
    toolboks_catalog::Index::parse(&index).map_err(|error| format!("{error:?}"))?;
    fs::write(folder.join("index.json"), &index)?;
    println!(
        "Local test catalog: {}. Preview builds only; never publish this catalog.",
        folder.display()
    );
    Ok(())
}
