use crate::parsers::utils::*;
use regex::Regex;
use url::Url;

use strum_macros::EnumIter;

pub mod client;
pub mod diagnostics;
pub mod parsers;
pub mod reverse_map;
pub mod server;
pub mod symbols;
pub mod utils;

#[derive(Debug, Hash, PartialEq, std::cmp::Eq, Copy, Clone, EnumIter)]
pub enum SupportedFileType {
    Rust,
    Go,
    Python,
    TypeScript,
    JavaScript,
    C,
    Cpp,
    Java,
    Ruby,
    Haskell,
}

impl SupportedFileType {
    pub fn from_extension(extension: String) -> Option<SupportedFileType> {
        match extension.as_str() {
            "rs" => Some(SupportedFileType::Rust),
            "go" => Some(SupportedFileType::Go),
            "py" => Some(SupportedFileType::Python),
            "ts" | "tsx" => Some(SupportedFileType::TypeScript),
            "js" | "jsx" | "mjs" | "cjs" => Some(SupportedFileType::JavaScript),
            "c" | "h" => Some(SupportedFileType::C),
            "cpp" | "cc" | "cxx" | "hpp" | "hxx" | "hh" => Some(SupportedFileType::Cpp),
            "java" => Some(SupportedFileType::Java),
            "rb" => Some(SupportedFileType::Ruby),
            "hs" | "lhs" => Some(SupportedFileType::Haskell),
            _ => None,
        }
    }

    pub fn from_filename(filename: String) -> Option<SupportedFileType> {
        filename
            .rsplit_once('.')
            .map(|(_name, extension)| extension.to_string())
            .and_then(SupportedFileType::from_extension)
    }
}

pub fn get_lsp_for_file_type(file_type: SupportedFileType) -> (String, Option<String>) {
    // TBH no idea why this is an Option type.  Maybe I should check that these
    // actually exist on the machine running lsp?
    match file_type {
        SupportedFileType::Rust => ("rust-analyzer".to_string(), None),
        SupportedFileType::Go => ("gopls".to_string(), None),
        SupportedFileType::Python => ("pylsp".to_string(), None),
        SupportedFileType::TypeScript => (
            "typescript-language-server".to_string(),
            Some("--stdio".to_string()),
        ),
        SupportedFileType::JavaScript => (
            "typescript-language-server".to_string(),
            Some("--stdio".to_string()),
        ),
        SupportedFileType::C => ("clangd".to_string(), None),
        SupportedFileType::Cpp => ("clangd".to_string(), None),
        SupportedFileType::Java => ("jdtls".to_string(), None),
        SupportedFileType::Ruby => ("solargraph".to_string(), Some("stdio".to_string())),
        SupportedFileType::Haskell => (
            "haskell-language-server-wrapper".to_string(),
            Some("--lsp".to_string()),
        ),
    }
}

/// Builds a `file://` URI from the project root and a diff-relative filename.
/// Returns `None` if the resulting path isn't absolute.
pub fn uri_from_relative_filename(project_root: String, rel_filename: &str) -> Option<Url> {
    // since teh diff has a relative path like /src/lib.rs and not a full path.
    let path = project_root + "/" + rel_filename;
    let uri = Url::from_file_path(&path).ok();
    if uri.is_none() {
        log::warn!("Unable to build a file URI from path {:?}", path);
    }
    uri
}
