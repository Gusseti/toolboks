//! Writes a new, complete Toolboks add-on project from a few answers.
//!
//! The code of the new project is the example add-on in `sdk/examples/addon-template`, which
//! the host workspace builds and tests, so what a new author gets always compiles against this
//! SDK. This crate copies it and puts the author's name, id and title in place of the
//! example's own, and adds the manifest, the icon, the README and the notes for coding
//! assistants.
//!
//! ```no_run
//! use toolboks_addon_templates::{write_project, Params};
//! let mut params = Params::suggest("counter", "Your name");
//! params.host_rev = "0123456789abcdef0123456789abcdef01234567".into();
//! let folder = write_project(&params, std::path::Path::new(r"C:\Projects")).unwrap();
//! ```

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Where the generated project gets the SDK from (with `host_rev`).
pub const HOST_REPO: &str = "https://github.com/Gusseti/toolboks";
/// The accent [`Params::suggest`] picks.
pub const DEFAULT_ACCENT: &str = "#3ecf8e";
/// The description [`Params::suggest`] fills in. The README tells the author to replace it.
pub const PLACEHOLDER_DESCRIPTION: &str = "Describe in one sentence what this add-on does.";
/// The oldest Toolboks the new manifest names. The catalog raises it to what the SDK's
/// protocol needs when the add-on is published.
pub const MIN_TOOLBOKS_VERSION: &str = "0.12.5";

// The example add-on, compiled and tested in the host workspace.
const EXAMPLE_CARGO: &str = include_str!("../../examples/addon-template/Cargo.toml");
const EXAMPLE_MAIN: &str = include_str!("../../examples/addon-template/src/main.rs");
const EXAMPLE_TEST: &str = include_str!("../../examples/addon-template/tests/protocol.rs");
/// The placeholder icon: 64 x 64, white shape on a transparent background.
pub const ICON_PNG: &[u8] = include_bytes!("../../examples/addon-template/icon.png");
// The same static C runtime and DLL search settings as the host and the add-ons repository.
const CARGO_CONFIG: &str = include_str!("../../../.cargo/config.toml");
const README: &str = include_str!("../templates/README.md");
const AGENTS: &str = include_str!("../templates/AGENTS.md");
const CLAUDE: &str = include_str!("../templates/CLAUDE.md");

// What the example calls itself; each is replaced by the author's own.
const EXAMPLE_ID: &str = "toolboks.example";
const EXAMPLE_NAME: &str = "addon-template";
const EXAMPLE_TITLE_LITERAL: &str = "\"Example add-on\"";

/// The answers a new project is made from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Params {
    /// Folder, crate and exe name: lowercase letters, digits and hyphens, like `disk-manager`.
    pub name: String,
    /// The id Toolboks knows the add-on by, like `com.yourname.counter`.
    pub id: String,
    /// The name shown in the sidebar.
    pub title: String,
    /// One sentence about what the add-on does.
    pub description: String,
    pub author: String,
    /// `#RRGGBB`.
    pub accent: String,
    /// The 40-character commit of `Gusseti/toolboks` the project's SDK comes from.
    pub host_rev: String,
}

impl Params {
    /// Sensible answers for `name`: id `local.<name>`, the title from the name, the default
    /// accent and a placeholder description. `host_rev` is left empty for the caller to set.
    pub fn suggest(name: &str, author: &str) -> Params {
        let words = name.replace('-', " ");
        let mut chars = words.chars();
        let title = match chars.next() {
            Some(first) => first.to_uppercase().chain(chars).collect(),
            None => String::new(),
        };
        Params {
            name: name.to_string(),
            id: format!("local.{name}"),
            title,
            description: PLACEHOLDER_DESCRIPTION.to_string(),
            author: author.to_string(),
            accent: DEFAULT_ACCENT.to_string(),
            host_rev: String::new(),
        }
    }

    /// Checks every answer. The error is one sentence a person can act on.
    pub fn validate(&self) -> Result<(), String> {
        let name = self.name.as_bytes();
        if !(2..=40).contains(&name.len())
            || !name[0].is_ascii_lowercase()
            || !name
                .iter()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
        {
            return Err("The name must be 2 to 40 lowercase letters, digits and hyphens, starting with a letter, like disk-manager.".into());
        }
        if !valid_id(&self.id) {
            return Err("The id must be lowercase names joined by dots, like com.yourname.counter: letters, digits and hyphens, each name starting with a letter, 3 to 100 characters in all.".into());
        }
        if !one_line(&self.title, 64) {
            return Err("The title must be one line of 1 to 64 characters.".into());
        }
        if !one_line(&self.description, 220) {
            return Err("The description must be one line of 1 to 220 characters.".into());
        }
        if !one_line(&self.author, 100) {
            return Err("The author must be one line of 1 to 100 characters.".into());
        }
        let accent = self.accent.as_bytes();
        if accent.len() != 7 || accent[0] != b'#' || !accent[1..].iter().all(u8::is_ascii_hexdigit)
        {
            return Err("The accent must be a colour like #3ecf8e.".into());
        }
        if self.host_rev.len() != 40
            || !self
                .host_rev
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("The Toolboks commit must be 40 lowercase hexadecimal characters.".into());
        }
        Ok(())
    }
}

/// The SDK's id rule (`toolboks_addon_sdk::valid_id`), repeated here so this crate needs only
/// std. A test checks that the two agree.
fn valid_id(s: &str) -> bool {
    (3..=100).contains(&s.len())
        && s.split('.').all(|p| {
            p.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
                && p.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

/// Not blank, at most `max` bytes (the manifest's limit), no line breaks or other control
/// characters.
fn one_line(s: &str, max: usize) -> bool {
    !s.trim().is_empty() && s.len() <= max && !s.chars().any(char::is_control)
}

/// One file of the new project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    /// Relative, with forward slashes, like `src/main.rs`.
    pub path: String,
    pub bytes: Vec<u8>,
}

impl File {
    fn text(path: impl Into<String>, text: String) -> File {
        File {
            path: path.into(),
            bytes: text.into_bytes(),
        }
    }
}

/// Every file of a new project for `p`, after checking `p`.
pub fn generate(p: &Params) -> Result<Vec<File>, String> {
    p.validate()?;
    let title_literal = format!("{:?}", p.title);
    let code = [
        (EXAMPLE_ID, p.id.as_str()),
        (EXAMPLE_TITLE_LITERAL, title_literal.as_str()),
        (EXAMPLE_NAME, p.name.as_str()),
    ];
    let docs = [
        ("{{name}}", p.name.as_str()),
        ("{{id}}", p.id.as_str()),
        ("{{title}}", p.title.as_str()),
        ("{{rev}}", p.host_rev.as_str()),
    ];
    Ok(vec![
        File::text("Cargo.toml", cargo_toml(p)),
        File::text(".cargo/config.toml", lf(CARGO_CONFIG)),
        File::text(".gitignore", "target/\n".into()),
        File::text(format!("{}.tbaddon", p.name), manifest(p)),
        File {
            path: "icon.png".into(),
            bytes: ICON_PNG.to_vec(),
        },
        File::text("src/main.rs", replace(&lf(EXAMPLE_MAIN), &code)),
        File::text("tests/protocol.rs", replace(&lf(EXAMPLE_TEST), &code)),
        File::text("README.md", replace(&lf(README), &docs)),
        File::text("AGENTS.md", replace(&lf(AGENTS), &docs)),
        File::text("CLAUDE.md", replace(&lf(CLAUDE), &docs)),
    ])
}

/// Writes a new project for `p` into a new folder `<parent>/<name>` and returns that folder.
/// Refuses when the folder exists and is not empty: nothing is ever overwritten. If writing
/// fails part way, what this call created is removed again.
pub fn write_project(p: &Params, parent: &Path) -> Result<PathBuf, String> {
    let files = generate(p)?;
    if !parent.is_dir() {
        return Err(format!("The folder {} does not exist.", parent.display()));
    }
    write_files(&files, &parent.join(&p.name))
}

fn write_files(files: &[File], root: &Path) -> Result<PathBuf, String> {
    let mut made = Made::default();
    if root.exists() {
        let mut entries = fs::read_dir(root).map_err(|e| {
            format!(
                "{} exists and cannot be read as a folder: {e}",
                root.display()
            )
        })?;
        if entries.next().is_some() {
            return Err(format!(
                "{} already exists and is not empty. Choose another name or an empty folder.",
                root.display()
            ));
        }
    } else {
        fs::create_dir(root).map_err(|e| format!("Could not create {}: {e}", root.display()))?;
        made.dirs.push(root.to_path_buf());
    }
    for file in files {
        if let Err(e) = made.write(root, file) {
            made.undo();
            return Err(format!(
                "Could not write {}: {e}. Nothing was left behind.",
                file.path
            ));
        }
    }
    Ok(root.to_path_buf())
}

/// What a write created, so a failure can remove exactly that.
#[derive(Default)]
struct Made {
    dirs: Vec<PathBuf>,
    files: Vec<PathBuf>,
}

impl Made {
    fn write(&mut self, root: &Path, file: &File) -> std::io::Result<()> {
        let mut path = root.to_path_buf();
        let parts: Vec<&str> = file.path.split('/').collect();
        let (name, folders) = parts.split_last().expect("split gives one part at least");
        for folder in folders {
            path.push(folder);
            if !path.is_dir() {
                fs::create_dir(&path)?;
                self.dirs.push(path.clone());
            }
        }
        path.push(name);
        // create_new: never replace a file, even one that appeared since the check.
        let mut out = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        self.files.push(path);
        out.write_all(&file.bytes)?;
        out.sync_all()
    }

    fn undo(&mut self) {
        for file in self.files.drain(..).rev() {
            let _ = fs::remove_file(file);
        }
        // Newest first, and only when empty, so nothing that was there before is touched.
        for dir in self.dirs.drain(..).rev() {
            let _ = fs::remove_dir(dir);
        }
    }
}

/// The example's Cargo.toml with the SDK from git at `host_rev` instead of local paths, as a
/// standalone project with a small release build.
fn cargo_toml(p: &Params) -> String {
    let git = format!("git = \"{HOST_REPO}\", rev = \"{}\"", p.host_rev);
    let mut out = String::new();
    for line in lf(EXAMPLE_CARGO).lines() {
        if line.starts_with("description = ") {
            continue;
        }
        let line = line
            .replace(
                "name = \"addon-template\"",
                &format!("name = \"{}\"", p.name),
            )
            .replace("path = \"../../toolboks-addon-sdk\"", &git)
            .replace("path = \"../../../shared/toolboks-shared\"", &git);
        out.push_str(&line);
        out.push('\n');
        if line == "[dependencies]" {
            out.push_str(
                "# The SDK comes from the Toolboks repository at one fixed commit (rev), so every\n\
                 # build gets the same code. Change rev only to move to a newer SDK.\n",
            );
        }
    }
    out.push_str(
        "\n# Its own project, even inside another Cargo workspace.\n\
         [workspace]\n\
         \n\
         # Small, self-contained release exe.\n\
         [profile.release]\n\
         opt-level = \"s\"\n\
         lto = true\n\
         codegen-units = 1\n\
         strip = true\n\
         panic = \"abort\"\n",
    );
    out
}

fn manifest(p: &Params) -> String {
    let s = json_string;
    format!(
        r#"{{
  "schema_version": 1,
  "manifest": {{
    "id": {id},
    "title": {title},
    "native_provider": {id},
    "description": {description},
    "long_description": {description},
    "version": "0.1.0",
    "author": {author},
    "publisher": {author},
    "category": "Other",
    "icon": "custom",
    "accent": {accent},
    "api_version": 2,
    "min_toolboks_version": "{MIN_TOOLBOKS_VERSION}",
    "license": "MIT",
    "capabilities": []
  }},
  "pages": []
}}
"#,
        id = s(&p.id),
        title = s(&p.title),
        description = s(&p.description),
        author = s(&p.author),
        accent = s(&p.accent),
    )
}

/// `s` as a JSON string literal.
fn json_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Plain `\n` line ends, whatever the checkout uses.
fn lf(s: &str) -> String {
    s.replace("\r\n", "\n")
}

/// Replaces each `from` with its `to` in one pass from left to right, so text put in is never
/// replaced again. The first pair that matches at a position wins.
fn replace(text: &str, pairs: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    'scan: while !rest.is_empty() {
        for (from, to) in pairs {
            if let Some(after) = rest.strip_prefix(from) {
                out.push_str(to);
                rest = after;
                continue 'scan;
            }
        }
        let c = rest.chars().next().expect("rest is not empty");
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

#[cfg(test)]
mod tests;
