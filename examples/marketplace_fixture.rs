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
    if native.as_deref() == Some("--native-addon") {
        let manifest = args.next().ok_or("Expected manifest path")?;
        let exe = args.next().ok_or("Expected executable path")?;
        let services = args.next().ok_or("Expected host-services.json path")?;
        return native_fixture(&folder, &manifest, &exe, &services);
    }
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

/// An isolated, unsigned fixture for a real native executable. The updated preview
/// catalog reader accepts the same plain index as production; no keys are involved.
fn native_fixture(
    folder: &std::path::Path,
    manifest: &str,
    exe: &str,
    services: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let package =
        toolboks_addon_sdk::Package::parse(&fs::read(manifest)?, env!("CARGO_PKG_VERSION"))?;
    let m = package.manifest;
    let services: serde_json::Value = serde_json::from_slice(&fs::read(services)?)?;
    let bytes = fs::read(exe)?;
    let filename = std::path::Path::new(exe)
        .file_name()
        .ok_or("Expected executable filename")?
        .to_str()
        .ok_or("Expected UTF-8 executable filename")?;
    fs::write(folder.join(filename), &bytes)?;
    let entry = serde_json::json!({
        "id":m.id, "title":m.title, "byline":m.byline,
        "short_description":m.description, "long_description":m.long_description,
        "publisher":m.publisher, "category":m.category, "icon":m.icon, "accent":m.accent,
        "kind":"native", "version":m.version, "min_toolboks_version":m.min_toolboks_version,
        "protocol_version":toolboks_addon_proto::PROTOCOL_VERSION,
        "download_url":format!("https://github.com/fredhakon/toolboks-addons/releases/download/fixture/{filename}"),
        "size":bytes.len(), "sha256":sha256_hex(&bytes), "verified":true, "channel":"stable",
        "capabilities":m.capabilities, "pages":services.get("pages").cloned().unwrap_or(serde_json::json!([])),
        "background":services.get("background").and_then(|b| b.as_bool()).unwrap_or(false),
        "release_notes":"Local native migration check. Drive-changing optimization is gated on #132."
    });
    let index =
        serde_json::to_vec_pretty(&serde_json::json!({"format_version":1, "entries":[entry]}))?;
    toolboks_catalog::Index::parse(&index)?;
    fs::write(folder.join("index.json"), index)?;
    println!("Unsigned local native fixture: {}", folder.display());
    Ok(())
}
