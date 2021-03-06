//! Provides the `Manifest` type, which represents a Node manifest file (`package.json`).

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use detect_indent;
use notion_fail::{ExitCode, Fallible, NotionFail, ResultExt};
use semver::Version;
use serde::Serialize;
use serde_json;

pub(crate) mod serial;

#[derive(Debug, Fail, NotionFail)]
#[fail(display = "Could not read package info: {}", error)]
#[notion_fail(code = "FileSystemError")]
pub(crate) struct PackageReadError {
    pub(crate) error: String,
}

impl PackageReadError {
    pub(crate) fn from_io_error(error: &io::Error) -> Self {
        PackageReadError {
            error: error.to_string(),
        }
    }
}

/// A toolchain manifest.
pub struct ToolchainManifest {
    /// The pinned version of Node, under the `toolchain.node` key.
    pub node: Version,
    /// The pinned version of Node as a string.
    pub node_str: String,
    /// The pinned version of Yarn, under the `toolchain.yarn` key.
    pub yarn: Option<Version>,
    /// The pinned version of Yarn as a string.
    pub yarn_str: Option<String>,
}

/// A Node manifest file.
pub struct Manifest {
    /// The `toolchain` section.
    pub toolchain: Option<ToolchainManifest>,
    /// The `dependencies` section.
    pub dependencies: HashMap<String, String>,
    /// The `devDependencies` section.
    pub dev_dependencies: HashMap<String, String>,
    /// The `bin` section, containing a map of binary names to locations
    pub bin: HashMap<String, String>,
}

impl Manifest {
    /// Loads and parses a Node manifest for the project rooted at the specified path.
    pub fn for_dir(project_root: &Path) -> Fallible<Manifest> {
        let file = File::open(project_root.join("package.json"))
            .with_context(PackageReadError::from_io_error)?;
        let serial: serial::Manifest = serde_json::de::from_reader(file).unknown()?;
        serial.into_manifest()
    }

    /// Returns whether this manifest contains a toolchain section (at least Node is pinned).
    pub fn has_toolchain(&self) -> bool {
        self.toolchain.is_some()
    }

    /// Returns the pinned version of Node as a Version, if any.
    pub fn node(&self) -> Option<Version> {
        self.toolchain.as_ref().map(|t| t.node.clone())
    }

    /// Returns the pinned verison of Node as a String, if any.
    pub fn node_str(&self) -> Option<String> {
        self.toolchain.as_ref().map(|t| t.node_str.clone())
    }

    /// Returns the pinned verison of Yarn as a Version, if any.
    pub fn yarn(&self) -> Option<Version> {
        self.toolchain
            .as_ref()
            .map(|t| t.yarn.clone())
            .unwrap_or(None)
    }

    /// Returns the pinned verison of Yarn as a String, if any.
    pub fn yarn_str(&self) -> Option<String> {
        self.toolchain
            .as_ref()
            .map(|t| t.yarn_str.clone())
            .unwrap_or(None)
    }

    /// Writes the input ToolchainManifest to package.json, adding the "toolchain" key if
    /// necessary.
    pub fn update_toolchain(
        toolchain: serial::ToolchainManifest,
        package_file: PathBuf,
    ) -> Fallible<()> {
        // parse the entire package.json file into a Value
        let file = File::open(&package_file).unknown()?;
        let mut v: serde_json::Value = serde_json::from_reader(file).unknown()?;

        // detect indentation in package.json
        let mut contents = String::new();
        let mut indent_file = File::open(&package_file).unknown()?;
        indent_file.read_to_string(&mut contents).unknown()?;
        let indent = detect_indent::detect_indent(&contents);

        if let Some(map) = v.as_object_mut() {
            // update the "toolchain" key
            let toolchain_value = serde_json::to_value(toolchain).unknown()?;
            map.insert("toolchain".to_string(), toolchain_value);

            // serialize the updated contents back to package.json
            let file = File::create(package_file).unknown()?;
            let formatter =
                serde_json::ser::PrettyFormatter::with_indent(indent.indent().as_bytes());
            let mut ser = serde_json::Serializer::with_formatter(file, formatter);
            map.serialize(&mut ser).unknown()?;
        }
        Ok(())
    }
}

// unit tests

#[cfg(test)]
pub mod tests;
