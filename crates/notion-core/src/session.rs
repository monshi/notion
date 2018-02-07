use config::{self, Config, NodeConfig};
use plugin::{self, ResolveResponse};
use catalog::Catalog;
use project::Project;
use failure;

use lazycell::LazyCell;
use semver::{Version, VersionReq};
use cmdline_words_parser::StrExt;
use readext::ReadExt;
use serde_json;

use std::string::ToString;
use std::process::{Command, Stdio};
use std::ffi::OsString;

pub struct Session {
    config: LazyCell<Config>,
    catalog: LazyCell<Catalog>,
    project: Option<Project>
}

impl Session {

    pub fn new() -> Result<Session, failure::Error> {
        Ok(Session {
            config: LazyCell::new(),
            catalog: LazyCell::new(),
            project: Project::for_current_dir()?
        })
    }

    pub fn catalog(&self) -> Result<&Catalog, failure::Error> {
        self.catalog.try_borrow_with(|| Catalog::current())
    }

    pub fn catalog_mut(&mut self) -> Result<&mut Catalog, failure::Error> {
        self.catalog.try_borrow_mut_with(|| Catalog::current())
    }

    pub fn config(&self) -> Result<&Config, failure::Error> {
        self.config.try_borrow_with(|| config::config())
    }

    // FIXME: should return Version once we kill lockfile
    pub fn node_version(&self) -> Result<Option<String>, failure::Error> {
        if let Some(ref project) = self.project {
            return Ok(Some(project.lockfile()?.node.version.clone()));
        }

        Ok(self.catalog()?.node.current.clone().map(|v| v.to_string()))
    }

    pub fn node(&mut self) -> Result<Option<Version>, failure::Error> {
        let catalog = self.catalog()?;

        if let Some(ref project) = self.project {
            let req: VersionReq = project.manifest().node_req();
            let available = catalog.node.resolve_local(&req);

            return if available.is_some() {
                Ok(available)
            } else {
                self.resolve_remote_node(&req).map(Some)
            }
        }

        Ok(catalog.node.current.clone())
    }

    fn resolve_remote_node(&self, req: &VersionReq) -> Result<Version, failure::Error> {
        let config = self.config()?;

        match config.node {
            Some(NodeConfig { resolve: Some(plugin::Resolve::Url(_)), .. }) => {
                unimplemented!()
            }
            Some(NodeConfig { resolve: Some(plugin::Resolve::Bin(ref bin)), .. }) => {
                let mut bin = bin.trim().to_string();
                let mut words = bin.parse_cmdline_words();
                // FIXME: error for not having any commands
                let cmd = words.next().unwrap();
                let args: Vec<OsString> = words.map(|s| {
                    let mut os = OsString::new();
                    os.push(s);
                    os
                }).collect();
                let child = Command::new(cmd)
                    .args(&args)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap(); // FIXME: error for failed spawn
                let response = ResolveResponse::from_reader(child.stdout.unwrap())?;
                eprintln!("response: {:?}", response);
                panic!("there's a bin plugin")
            }
            _ => {
                panic!("there's no plugin")
            }
        }
    }

}