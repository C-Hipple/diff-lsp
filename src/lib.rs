use crate::parsers::utils::*;
use regex::Regex;
use url::Url;

use strum_macros::EnumIter;

pub mod client;
pub mod parsers;
pub mod server;
pub mod utils;

#[derive(Debug, Hash, PartialEq, std::cmp::Eq, Copy, Clone, EnumIter)]
pub enum SupportedFileType {
    Rust,
    Go,
    Python,
}

impl SupportedFileType {
    pub fn from_extension(extension: String) -> Option<SupportedFileType> {
        match extension.as_str() {
            "rs" => Some(SupportedFileType::Rust),
            "go" => Some(SupportedFileType::Go),
            "py" => Some(SupportedFileType::Python),
            _ => None,
        }
    }

    pub fn from_filename(filename: String) -> Option<SupportedFileType> {
        filename
            .rsplit_once('.')
            .map(|(_name, extension)| extension.to_string())
            .and_then(|extension| SupportedFileType::from_extension(extension))
    }
}

pub fn get_lsp_for_file_type(file_type: SupportedFileType) -> Option<String> {
    // TBH no idea why this is an Option type.  Maybe I should check that these
    // actually exist on the machine running lsp?
    match file_type {
        SupportedFileType::Rust => Some("rust-analyzer".to_string()),
        SupportedFileType::Go => Some("gopls".to_string()),
        SupportedFileType::Python => Some("pylsp".to_string()),
    }
}

pub fn uri_from_relative_filename(project_root: String, rel_filename: &str) -> Url {
    // since teh diff has a relative path like /src/lib.rs and not a full path.
    Url::from_file_path(project_root + "/" + rel_filename).unwrap()
}
