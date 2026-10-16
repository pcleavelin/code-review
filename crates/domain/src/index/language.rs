use std::fmt;

use strum::VariantArray;

use crate::text::RelativePath;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LanguageId(&'static str);

impl LanguageId {
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

const RUST: LanguageId = LanguageId("rust");
const ODIN: LanguageId = LanguageId("odin");
const CLANG: LanguageId = LanguageId("c");
const PYTHON: LanguageId = LanguageId("python");
const JAVASCRIPT: LanguageId = LanguageId("javascript");
const JAVASCRIPT_MARKUP: LanguageId = LanguageId("javascriptreact");
const TYPESCRIPT: LanguageId = LanguageId("typescript");
const TYPESCRIPT_MARKUP: LanguageId = LanguageId("typescriptreact");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Extension(&'static str);

impl Extension {
    const fn as_str(self) -> &'static str {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileType(Extension, LanguageId);

impl FileType {
    const fn language_id(self) -> LanguageId {
        self.1
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Program(&'static str);

impl Program {
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for Program {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Argument(&'static str);

impl Argument {
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, VariantArray)]
pub enum Language {
    Rust,
    Odin,
    Clang,
    Python,
    Javascript,
}

const STANDARD_INPUT: [Argument; 1] = [Argument("--stdio")];

impl Language {
    const fn file_types(self) -> &'static [FileType] {
        match self {
            Self::Rust => &[FileType(Extension("rs"), RUST)],
            Self::Odin => &[FileType(Extension("odin"), ODIN)],
            Self::Clang => &[
                FileType(Extension("c"), CLANG),
                FileType(Extension("h"), CLANG),
            ],
            Self::Python => &[FileType(Extension("py"), PYTHON)],
            Self::Javascript => &[
                FileType(Extension("js"), JAVASCRIPT),
                FileType(Extension("mjs"), JAVASCRIPT),
                FileType(Extension("cjs"), JAVASCRIPT),
                FileType(Extension("jsx"), JAVASCRIPT_MARKUP),
                FileType(Extension("ts"), TYPESCRIPT),
                FileType(Extension("mts"), TYPESCRIPT),
                FileType(Extension("cts"), TYPESCRIPT),
                FileType(Extension("tsx"), TYPESCRIPT_MARKUP),
            ],
        }
    }

    pub const fn program(self) -> Program {
        match self {
            Self::Rust => Program("rust-analyzer"),
            Self::Odin => Program("ols"),
            Self::Clang => Program("clangd"),
            Self::Python => Program("pyright-langserver"),
            Self::Javascript => Program("typescript-language-server"),
        }
    }

    pub const fn arguments(self) -> &'static [Argument] {
        match self {
            Self::Rust | Self::Odin | Self::Clang => &[],
            Self::Python | Self::Javascript => &STANDARD_INPUT,
        }
    }

    pub fn of(path: &RelativePath) -> Option<Self> {
        Self::VARIANTS
            .iter()
            .copied()
            .find(|language| language.file_type(path).is_some())
    }

    pub fn language_id(path: &RelativePath) -> Option<LanguageId> {
        Self::VARIANTS
            .iter()
            .find_map(|language| language.file_type(path))
            .map(FileType::language_id)
    }

    fn file_type(self, path: &RelativePath) -> Option<FileType> {
        let last = path.last_dotted();
        self.file_types()
            .iter()
            .copied()
            .find(|file_type| file_type.0.as_str() == last)
    }
}
