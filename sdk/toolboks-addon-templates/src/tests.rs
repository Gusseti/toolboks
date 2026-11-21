use super::*;
use std::collections::BTreeSet;

const REV: &str = "0123456789abcdef0123456789abcdef01234567";

fn params() -> Params {
    let mut p = Params::suggest("disk-tidy", "Ada \"the\" Author\\");
    p.host_rev = REV.into();
    p.title = "Disk \"tidy\" \\ é".into();
    p
}

fn text_of(files: &[File], path: &str) -> String {
    let file = files.iter().find(|f| f.path == path).expect(path);
    String::from_utf8(file.bytes.clone()).expect("text")
}

#[test]
fn suggest_fills_sensible_answers() {
    let p = Params::suggest("disk-manager", "Ada");
    assert_eq!(p.id, "local.disk-manager");
    assert_eq!(p.title, "Disk manager");
    assert_eq!(p.description, PLACEHOLDER_DESCRIPTION);
    assert_eq!(p.accent, DEFAULT_ACCENT);
    assert_eq!(p.author, "Ada");
    assert_eq!(p.host_rev, "");
    let mut p = p;
    p.host_rev = REV.into();
    assert_eq!(p.validate(), Ok(()));
}

#[test]
fn validate_checks_each_answer() {
    let good = params();
    assert_eq!(good.validate(), Ok(()));
    let bad = |change: fn(&mut Params), word: &str| {
        let mut p = good.clone();
        change(&mut p);
        let error = p.validate().unwrap_err();
        assert!(error.starts_with(word), "{error}");
        assert!(error.ends_with('.'), "{error}");
    };
    bad(|p| p.name = "a".into(), "The name");
    bad(|p| p.name = "x".repeat(41), "The name");
    bad(|p| p.name = "1disk".into(), "The name");
    bad(|p| p.name = "-disk".into(), "The name");
    bad(|p| p.name = "Disk".into(), "The name");
    bad(|p| p.name = "disk_tidy".into(), "The name");
    bad(|p| p.name = "disk tidy".into(), "The name");
    bad(|p| p.id = "Local.disk".into(), "The id");
    bad(|p| p.id = "local..disk".into(), "The id");
    bad(|p| p.id = "local.1disk".into(), "The id");
    bad(|p| p.id = "ab".into(), "The id");
    bad(|p| p.title = " ".into(), "The title");
    bad(|p| p.title = "x".repeat(65), "The title");
    bad(|p| p.title = "two\nlines".into(), "The title");
    bad(|p| p.description = String::new(), "The description");
    bad(|p| p.description = "x".repeat(221), "The description");
    bad(|p| p.description = "a\tb".into(), "The description");
    bad(|p| p.author = String::new(), "The author");
    bad(|p| p.author = "x".repeat(101), "The author");
    bad(|p| p.accent = "3ecf8e".into(), "The accent");
    bad(|p| p.accent = "#3ecf8g".into(), "The accent");
    bad(|p| p.accent = "#3ecf8e0".into(), "The accent");
    bad(|p| p.host_rev = String::new(), "The Toolboks commit");
    bad(|p| p.host_rev = REV.to_uppercase(), "The Toolboks commit");
    bad(|p| p.host_rev = REV[..39].into(), "The Toolboks commit");

    let ok = |change: fn(&mut Params)| {
        let mut p = good.clone();
        change(&mut p);
        assert_eq!(p.validate(), Ok(()));
    };
    ok(|p| p.name = "ab".into());
    ok(|p| p.name = "a".repeat(40));
    ok(|p| p.name = "disk-2".into());
    ok(|p| p.id = "com.ada.disk-tidy2".into());
    ok(|p| p.title = "x".repeat(64));
    ok(|p| p.description = "x".repeat(220));
    ok(|p| p.author = "x".repeat(100));
    ok(|p| p.accent = "#ABCdef".into());
}

#[test]
fn the_id_rule_is_the_sdks() {
    for id in [
        "local.disk",
        "a.b",
        "abc",
        "ab",
        "local..disk",
        "Local.x",
        "x.1a",
        "x-y.z-1",
        ".ab",
        "ab.",
        "a_b.c",
        "a.b-",
        "é.ab",
    ] {
        assert_eq!(
            valid_id(id),
            toolboks_addon_sdk::valid_id(id),
            "the two rules disagree on {id:?}"
        );
    }
    let long = format!("a.{}", "b".repeat(98));
    assert_eq!(valid_id(&long), toolboks_addon_sdk::valid_id(&long));
    let too_long = format!("a.{}", "b".repeat(99));
    assert_eq!(valid_id(&too_long), toolboks_addon_sdk::valid_id(&too_long));
}

#[test]
fn generate_refuses_invalid_answers() {
    let mut p = params();
    p.name = "Bad".into();
    assert!(generate(&p).unwrap_err().starts_with("The name"));
}

#[test]
fn generate_returns_exactly_the_project_files() {
    let paths: Vec<String> = generate(&params())
        .unwrap()
        .into_iter()
        .map(|f| f.path)
        .collect();
    assert_eq!(
        paths,
        [
            "Cargo.toml",
            ".cargo/config.toml",
            ".gitignore",
            "disk-tidy.tbaddon",
            "icon.png",
            "src/main.rs",
            "tests/protocol.rs",
            "README.md",
            "AGENTS.md",
            "CLAUDE.md",
        ]
    );
}

#[test]
fn the_manifest_parses_and_carries_the_answers() {
    let p = params();
    let files = generate(&p).unwrap();
    let bytes = &files
        .iter()
        .find(|f| f.path == "disk-tidy.tbaddon")
        .unwrap()
        .bytes;
    let package = toolboks_addon_sdk::Package::parse(bytes, MIN_TOOLBOKS_VERSION).unwrap();
    let m = package.manifest;
    assert_eq!(m.id, p.id);
    assert_eq!(m.native_provider.as_deref(), Some(p.id.as_str()));
    assert_eq!(m.title, p.title);
    assert_eq!(m.description, p.description);
    assert_eq!(m.author, p.author);
    assert_eq!(m.publisher, p.author);
    assert_eq!(m.accent, p.accent);
    assert_eq!(m.icon, "custom");
    assert_eq!(m.version, "0.1.0");
    assert_eq!(m.api_version, 2);
    assert_eq!(m.min_toolboks_version, MIN_TOOLBOKS_VERSION);
    assert!(m.capabilities.is_empty());
    assert!(package.pages.is_empty());
    // And the version is the one in the generated Cargo.toml.
    assert!(text_of(&files, "Cargo.toml").contains("\nversion = \"0.1.0\"\n"));
}

#[test]
fn every_placeholder_is_replaced() {
    let p = params();
    for file in generate(&p).unwrap() {
        if file.path == "icon.png" {
            continue;
        }
        let text = String::from_utf8(file.bytes).unwrap();
        for leftover in [
            "{{",
            "}}",
            EXAMPLE_ID,
            EXAMPLE_NAME,
            "Example add-on",
            "path = \"",
            "\r\n",
        ] {
            assert!(
                !text.contains(leftover),
                "{} still contains {leftover:?}",
                file.path
            );
        }
    }
}

#[test]
fn the_code_carries_the_id_name_and_title() {
    let p = params();
    let files = generate(&p).unwrap();
    let main = text_of(&files, "src/main.rs");
    assert!(main.contains("const ID: &str = \"local.disk-tidy\";"));
    assert!(main.contains(r#"const TITLE: &str = "Disk \"tidy\" \\ é";"#));
    assert!(main.contains("// EDIT:"));
    let test = text_of(&files, "tests/protocol.rs");
    assert!(test.contains("const ID: &str = \"local.disk-tidy\";"));
    assert!(test.contains("include_bytes!(\"../disk-tidy.tbaddon\")"));
    assert!(test.contains("env!(\"CARGO_BIN_EXE_disk-tidy\")"));
}

#[test]
fn the_cargo_toml_is_standalone_at_the_given_rev() {
    let cargo = text_of(&generate(&params()).unwrap(), "Cargo.toml");
    assert!(cargo.starts_with("[package]\nname = \"disk-tidy\"\n"));
    let pinned = format!("git = \"{HOST_REPO}\", rev = \"{REV}\"");
    // The SDK for the code and for the test, and the shared UI.
    assert_eq!(cargo.matches(&pinned).count(), 3, "{cargo}");
    assert!(cargo.contains("features = [\"runtime\"]"));
    assert!(cargo.contains("features = [\"testing\"]"));
    assert!(cargo.contains("features = [\"ui\"]"));
    assert!(cargo.contains("\n[workspace]\n"));
    assert!(cargo.contains("\n[profile.release]\n"));
    assert!(cargo.contains("panic = \"abort\""));
    assert!(!cargo.contains("description"));
}

#[test]
fn the_cargo_config_links_the_c_runtime_statically() {
    let config = text_of(&generate(&params()).unwrap(), ".cargo/config.toml");
    assert!(config.contains("+crt-static"));
    assert!(config.contains("/DEPENDENTLOADFLAG:0x800"));
    assert_eq!(
        text_of(&generate(&params()).unwrap(), ".gitignore"),
        "target/\n"
    );
}

#[test]
fn the_icon_passes_the_hosts_checks() {
    assert_eq!(toolboks_catalog::check_icon_header(ICON_PNG), Ok(64));
    let picture = image::load_from_memory_with_format(ICON_PNG, image::ImageFormat::Png)
        .unwrap()
        .into_rgba8();
    assert_eq!(picture.dimensions(), (64, 64));
    // Transparent corners, an opaque shape, and white wherever anything shows.
    assert_eq!(picture.get_pixel(0, 0).0[3], 0);
    assert_eq!(picture.get_pixel(63, 63).0[3], 0);
    assert!(picture.pixels().any(|p| p.0[3] == 255));
    assert!(picture
        .pixels()
        .filter(|p| p.0[3] > 0)
        .all(|p| p.0[..3] == [255, 255, 255]));
}

/// The names of the `NativeAddon` methods, read from the SDK's source.
fn trait_methods() -> BTreeSet<String> {
    let source = include_str!("../../toolboks-addon-sdk/src/lib.rs").replace("\r\n", "\n");
    let start = source
        .find("pub trait NativeAddon {")
        .expect("the trait in lib.rs");
    let body = &source[start..];
    let body = &body[..body.find("\n}\n").expect("the end of the trait")];
    let names: BTreeSet<String> = body
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("fn "))
        .map(|rest| {
            rest.split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .next()
                .unwrap()
                .to_string()
        })
        .collect();
    assert!(names.contains("ui") && names.len() > 10, "{names:?}");
    names
}

#[test]
fn agents_md_lists_exactly_the_real_methods() {
    let agents = text_of(&generate(&params()).unwrap(), "AGENTS.md");
    let section = agents
        .split("\n## The NativeAddon methods\n")
        .nth(1)
        .expect("the methods section");
    let section = section.split("\n## ").next().unwrap();
    let listed: BTreeSet<String> = section
        .lines()
        .filter_map(|line| line.strip_prefix("- `"))
        .map(|rest| rest.split('(').next().unwrap().to_string())
        .collect();
    assert_eq!(
        listed,
        trait_methods(),
        "update the list in templates/AGENTS.md"
    );
}

#[test]
fn agents_md_is_short() {
    let agents = text_of(&generate(&params()).unwrap(), "AGENTS.md");
    assert!(agents.lines().count() <= 160, "{}", agents.lines().count());
    assert!(text_of(&generate(&params()).unwrap(), "CLAUDE.md").contains("AGENTS.md"));
}

#[test]
fn the_readme_names_every_file_and_the_edits() {
    let files = generate(&params()).unwrap();
    let readme = text_of(&files, "README.md");
    for file in &files {
        assert!(
            readme.contains(&format!("`{}`", file.path)),
            "README.md does not name {}",
            file.path
        );
    }
    for words in [
        "cargo build",
        "cargo test",
        "cargo build --release",
        "cargo run --example addon_host",
        "EDIT:",
        "ADDON-AUTHOR-GUIDE.md",
        "local.disk-tidy",
    ] {
        assert!(readme.contains(words), "README.md does not mention {words}");
    }
}

#[test]
fn replace_never_replaces_what_it_put_in() {
    let pairs = [("ab", "b-ab"), ("b", "c")];
    assert_eq!(replace("ab b é", &pairs), "b-ab c é");
}

#[test]
fn json_strings_are_escaped() {
    assert_eq!(json_string("a\"b\\c"), r#""a\"b\\c""#);
    assert_eq!(json_string("\u{1}"), r#""\u0001""#);
}

fn tree(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(relative);
            }
        }
    }
    out.sort();
    out
}

#[test]
fn write_project_writes_every_file() {
    let parent = tempfile::tempdir().unwrap();
    let p = params();
    let folder = write_project(&p, parent.path()).unwrap();
    assert_eq!(folder, parent.path().join("disk-tidy"));
    let mut expected: Vec<String> = generate(&p).unwrap().into_iter().map(|f| f.path).collect();
    expected.sort();
    assert_eq!(tree(&folder), expected);
    assert_eq!(fs::read(folder.join("icon.png")).unwrap(), ICON_PNG);
    assert_eq!(
        fs::read_to_string(folder.join("src/main.rs")).unwrap(),
        text_of(&generate(&p).unwrap(), "src/main.rs")
    );
}

#[test]
fn write_project_uses_an_empty_existing_folder() {
    let parent = tempfile::tempdir().unwrap();
    fs::create_dir(parent.path().join("disk-tidy")).unwrap();
    let folder = write_project(&params(), parent.path()).unwrap();
    assert_eq!(tree(&folder).len(), 10);
}

#[test]
fn write_project_refuses_a_folder_that_is_not_empty() {
    let parent = tempfile::tempdir().unwrap();
    let folder = parent.path().join("disk-tidy");
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("Cargo.toml"), "mine").unwrap();
    let error = write_project(&params(), parent.path()).unwrap_err();
    assert!(error.contains("already exists and is not empty"), "{error}");
    assert_eq!(tree(&folder), ["Cargo.toml"]);
    assert_eq!(
        fs::read_to_string(folder.join("Cargo.toml")).unwrap(),
        "mine"
    );
}

#[test]
fn write_project_refuses_a_missing_parent_and_bad_answers() {
    let parent = tempfile::tempdir().unwrap();
    let missing = parent.path().join("nowhere");
    let error = write_project(&params(), &missing).unwrap_err();
    assert!(error.contains("does not exist"), "{error}");
    assert!(!missing.exists());
    let mut p = params();
    p.host_rev = String::new();
    assert!(write_project(&p, parent.path()).is_err());
    assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 0);
}

#[test]
fn a_failure_part_way_removes_only_what_was_created() {
    // The same path twice: the second write fails after other files and folders were made.
    let mut files = generate(&params()).unwrap();
    files.push(files[5].clone());

    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("disk-tidy");
    let error = write_files(&files, &root).unwrap_err();
    assert!(error.starts_with("Could not write src/main.rs"), "{error}");
    assert!(!root.exists(), "the folder this call made is removed");

    // An empty folder that was there before stays, and stays empty.
    fs::create_dir(&root).unwrap();
    assert!(write_files(&files, &root).is_err());
    assert!(root.is_dir());
    assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
}
