mod convert;
mod wire;

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use domain::{LayoutTree, OAuthApp, Root, Settings};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    User,
    OverrideOnly,
}

#[derive(Clone, Copy, Debug)]
struct FileName(&'static str);

#[derive(Clone, Copy, Debug)]
struct Variable(&'static str);

#[derive(Clone, Copy, Debug)]
struct UserFile {
    name: FileName,
    override_variable: Variable,
}

const LAYOUT_FILE: UserFile = UserFile {
    name: FileName("layout"),
    override_variable: Variable("REVIEW_LAYOUT"),
};

const SETTINGS_FILE: UserFile = UserFile {
    name: FileName("settings"),
    override_variable: Variable("REVIEW_SETTINGS"),
};

const PINS_FILE: UserFile = UserFile {
    name: FileName("pins"),
    override_variable: Variable("REVIEW_PINS"),
};

const FORGES_FILE: UserFile = UserFile {
    name: FileName("forges"),
    override_variable: Variable("REVIEW_FORGES"),
};

const CLONES: UserFile = UserFile {
    name: FileName("clones"),
    override_variable: Variable("REVIEW_CLONES"),
};

fn data_folder(file: UserFile, reach: Reach) -> Option<PathBuf> {
    if let Some(path) = env::var_os(file.override_variable.0) {
        return Some(PathBuf::from(path));
    }
    if reach == Reach::OverrideOnly {
        return None;
    }
    let folder = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("share"))
        })
        .or_else(|| env::var_os("LOCALAPPDATA").map(PathBuf::from))?;
    Some(folder.join("code-review").join(OsStr::new(file.name.0)))
}

fn user_file(file: UserFile, reach: Reach) -> Option<PathBuf> {
    if let Some(path) = env::var_os(file.override_variable.0) {
        return Some(PathBuf::from(path));
    }
    if reach == Reach::OverrideOnly {
        return None;
    }
    let folder = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .or_else(|| env::var_os("APPDATA").map(PathBuf::from))?;
    Some(folder.join("code-review").join(OsStr::new(file.name.0)))
}

fn save_text<Contents: AsRef<[u8]>>(path: &Path, contents: Contents) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    io_store::write(path, contents)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutStore {
    path: PathBuf,
}

impl LayoutStore {
    pub const fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn find(reach: Reach) -> Option<Self> {
        user_file(LAYOUT_FILE, reach).map(Self::at)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Option<LayoutTree> {
        let text = fs::read_to_string(&self.path).ok()?;
        convert::layout(&wire::parse(&text)?)
    }

    pub fn save(&self, layout: &LayoutTree) -> io::Result<()> {
        save_text(&self.path, wire::print(&convert::wire(layout)))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub const fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn find(reach: Reach) -> Option<Self> {
        user_file(SETTINGS_FILE, reach).map(Self::at)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Option<Settings> {
        let text = fs::read_to_string(&self.path).ok()?;
        wire::parse_settings(&text).map(|wire| convert::settings(&wire))
    }

    pub fn save(&self, settings: &Settings) -> io::Result<()> {
        save_text(
            &self.path,
            wire::print_settings(&convert::wire_settings(settings)),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinsStore {
    path: PathBuf,
    clones: Option<PathBuf>,
}

impl PinsStore {
    pub const fn at(path: PathBuf, clones: Option<PathBuf>) -> Self {
        Self { path, clones }
    }

    pub fn find(reach: Reach) -> Option<Self> {
        user_file(PINS_FILE, reach).map(|path| Self::at(path, data_folder(CLONES, reach)))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn clones(&self) -> Option<&Path> {
        self.clones.as_deref()
    }

    pub fn load(&self) -> Vec<Root> {
        fs::read_to_string(&self.path)
            .ok()
            .and_then(|text| wire::parse_pins(&text))
            .map(|pins| convert::pins(&pins))
            .unwrap_or_default()
    }

    pub fn save(&self, pins: &[Root]) -> io::Result<()> {
        save_text(&self.path, wire::print_pins(&convert::wire_pins(pins)))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForgesStore {
    path: PathBuf,
}

impl ForgesStore {
    pub const fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn find(reach: Reach) -> Option<Self> {
        user_file(FORGES_FILE, reach).map(Self::at)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Vec<OAuthApp> {
        fs::read_to_string(&self.path)
            .ok()
            .and_then(|text| wire::parse_forges(&text))
            .map(|forges| convert::apps(&forges))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
