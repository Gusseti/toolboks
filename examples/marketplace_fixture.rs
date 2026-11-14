use ed25519_compact::{KeyPair, Seed};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use toolboks_catalog::{hex_encode, key_id, sha256_hex, SignatureFile};

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
        let bytes = serde_json::to_vec(&package)?;
        let filename = format!("{slug}.tbaddon");
        fs::write(folder.join(&filename), &bytes)?;
        entries.push(serde_json::json!({
            "id": id, "title": title.to_owned(), "short_description": description,
            "long_description": "A harmless local SDK test package. This is not the published tool. It shows live memory readings and saves its own preferences inside Toolboks.",
            "publisher": "Toolboks", "category": category, "icon": icon, "accent": "#8b5cf6",
            "kind": "declarative", "version": version, "min_toolboks_version": "0.12.5",
            "protocol_version": toolboks_addon_proto::PROTOCOL_VERSION,
            "download_url": format!("https://github.com/Gusseti/toolboks-addons/releases/download/fixture/{filename}"),
            "size": bytes.len(), "sha256": sha256_hex(&bytes), "verified": true, "channel": "stable",
            "capabilities": ["host_metrics", "local_preferences"], "pages": ["Overview", "Preferences"],
            "release_notes": "First local test version. No Windows changes."
        }));
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
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let index = serde_json::to_vec_pretty(&serde_json::json!({
        "format_version": 1, "sequence": if version == "1.0.0" { 1 } else { 2 }, "generated_at": now.saturating_sub(60),
        "expires_at": now + 7 * 86400, "entries": entries
    }))?;
    toolboks_catalog::Index::parse(&index).map_err(|error| format!("{error:?}"))?;
    let keys = KeyPair::from_seed(Seed::new([86; 32]));
    let signature = SignatureFile {
        key_id: key_id(&keys.pk),
        signature: hex_encode(&keys.sk.sign(&index, None)[..]),
        rotations: vec![],
    };
    fs::write(folder.join("index.json"), &index)?;
    fs::write(
        folder.join("index.json.sig"),
        serde_json::to_vec(&signature)?,
    )?;
    fs::write(folder.join("catalog-public.txt"), hex_encode(&keys.pk[..]))?;
    let recovery = KeyPair::from_seed(Seed::new([87; 32]));
    fs::write(
        folder.join("recovery-public.txt"),
        hex_encode(&recovery.pk[..]),
    )?;
    println!(
        "Local test catalog: {}. Preview builds only. Fixed test key; never publish this catalog.",
        folder.display()
    );
    Ok(())
}
