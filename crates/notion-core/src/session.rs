//! Provides the `Session` type, which represents the user's state during an
//! execution of a Notion tool, including their configuration, their current
//! directory, and the state of the local tool catalog.

use std::env::{self, VarError};

use catalog::{Catalog, LazyCatalog};
use config::{Config, LazyConfig};
use distro::Fetched;
use plugin::Publish;
use project::Project;
use version::VersionSpec;

use std::fmt::{self, Display, Formatter};
use std::process::exit;

use event::EventLog;
use notion_fail::{ExitCode, Fallible, NotionError, NotionFail, ResultExt};
use semver::Version;

#[derive(Eq, PartialEq, Ord, PartialOrd, Clone, Copy)]
pub enum ActivityKind {
    Fetch,
    Install,
    Uninstall,
    Current,
    Deactivate,
    Default,
    Use,
    Node,
    Yarn,
    Notion,
    Tool,
    Help,
    Version,
    Binary,
    Shim,
}

impl Display for ActivityKind {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        let s = match self {
            &ActivityKind::Fetch => "fetch",
            &ActivityKind::Install => "install",
            &ActivityKind::Uninstall => "uninstall",
            &ActivityKind::Current => "current",
            &ActivityKind::Deactivate => "deactivate",
            &ActivityKind::Default => "default",
            &ActivityKind::Use => "use",
            &ActivityKind::Node => "node",
            &ActivityKind::Yarn => "yarn",
            &ActivityKind::Notion => "notion",
            &ActivityKind::Tool => "tool",
            &ActivityKind::Help => "help",
            &ActivityKind::Version => "version",
            &ActivityKind::Binary => "binary",
            &ActivityKind::Shim => "shim",
        };
        f.write_str(s)
    }
}

/// Thrown when the user tries to pin Node or Yarn versions outside of a package.
#[derive(Debug, Fail, NotionFail)]
#[fail(display = "Not in a node package")]
#[notion_fail(code = "ConfigurationError")]
pub(crate) struct NotInPackageError;

impl NotInPackageError {
    pub(crate) fn new() -> Self {
        NotInPackageError
    }
}

/// Represents the user's state during an execution of a Notion tool. The session
/// encapsulates a number of aspects of the environment in which the tool was
/// invoked, including:
///     - the current directory
///     - the Node project tree that contains the current directory (if any)
///     - the Notion configuration settings
///     - the catalog of locally-installed Notion tools
pub struct Session {
    config: LazyConfig,
    catalog: LazyCatalog,
    project: Option<Project>,
    event_log: EventLog,
}

impl Session {
    /// Constructs a new `Session`.
    pub fn new() -> Fallible<Session> {
        Ok(Session {
            config: LazyConfig::new(),
            catalog: LazyCatalog::new(),
            project: Project::for_current_dir()?,
            event_log: EventLog::new()?,
        })
    }

    /// Produces a reference to the current Node project, if any.
    pub fn project(&self) -> Option<&Project> {
        self.project.as_ref()
    }

    /// Returns if the current project has a pinned toolchain (at least Node is pinned).
    pub fn in_pinned_project(&self) -> bool {
        if let Some(ref project) = self.project {
            return project.is_pinned();
        }
        false
    }

    /// Produces a reference to the current tool catalog.
    pub fn catalog(&self) -> Fallible<&Catalog> {
        self.catalog.get()
    }

    /// Produces a mutable reference to the current tool catalog.
    pub fn catalog_mut(&mut self) -> Fallible<&mut Catalog> {
        self.catalog.get_mut()
    }

    /// Produces a reference to the configuration.
    pub fn config(&self) -> Fallible<&Config> {
        self.config.get()
    }

    /// Produces the version of Node for the current session. If there is an
    /// active pinned project, this will ensure that project's Node version is
    /// installed before returning. If there is no active pinned project, this
    /// produces the user version, which may be `None`.
    pub fn current_node(&mut self) -> Fallible<Option<Version>> {
        if self.in_pinned_project() {
            let project = self.project.as_ref().unwrap();
            let version = &project.manifest().node().unwrap();
            let catalog = self.catalog.get_mut()?;
            let spec = VersionSpec::exact(&version);

            if catalog.node.contains(version) {
                return Ok(Some(version.clone()));
            }

            let config = self.config.get()?;
            let fetched = catalog.fetch_node(&spec, config)?;

            return Ok(Some(fetched.into_version()));
        }

        self.user_node()
    }

    pub fn user_node(&self) -> Fallible<Option<Version>> {
        match env::var("NOTION_NODE_VERSION") {
            Ok(s) => Ok(Some(Version::parse(&s[..]).unknown()?)),
            Err(VarError::NotPresent) => Ok(self.catalog()?.node.default.clone()),
            Err(VarError::NotUnicode(_)) => unimplemented!(),
        }
    }

    /// Fetches a version of Node matching the specified semantic verisoning
    /// requirements.
    pub fn fetch_node(&mut self, matching: &VersionSpec) -> Fallible<Fetched> {
        let catalog = self.catalog.get_mut()?;
        let config = self.config.get()?;
        catalog.fetch_node(matching, config)
    }

    /// Sets the user toolchain's Node version to one matching the specified semantic versioning
    /// requirements.
    pub fn set_user_node(&mut self, matching: &VersionSpec) -> Fallible<()> {
        let catalog = self.catalog.get_mut()?;
        let config = self.config.get()?;
        catalog.set_user_node(matching, config)
    }

    /// Returns the version of Node matching the specified semantic versioning requirements.
    pub fn get_matching_node(&self, matching: &VersionSpec) -> Fallible<Version> {
        let catalog = self.catalog.get()?;
        let config = self.config.get()?;
        catalog.resolve_node(matching, config)
    }

    /// Updates toolchain in package.json with the Node version matching the specified semantic
    /// versioning requirements.
    pub fn pin_node_version(&self, matching: &VersionSpec) -> Fallible<()> {
        if let Some(ref project) = self.project() {
            let node_version = self.get_matching_node(matching)?;
            project.pin_node_in_toolchain(node_version)?;
        } else {
            throw!(NotInPackageError::new());
        }
        Ok(())
    }

    /// Produces the version of Yarn for the current session. If there is an
    /// active pinned project, this will ensure that project's Yarn version is
    /// installed before returning. If there is no active pinned project, this
    /// produces the user version, which may be `None`.
    pub fn current_yarn(&mut self) -> Fallible<Option<Version>> {
        if self.in_pinned_project() {
            let project = self.project.as_ref().unwrap();
            // pinning yarn is optional
            if let Some(version) = &project.manifest().yarn().clone() {
                let catalog = self.catalog.get_mut()?;
                let spec = VersionSpec::exact(&version);

                if catalog.yarn.contains(&version) {
                    return Ok(Some(version.clone()));
                }

                let config = self.config.get()?;
                let fetched = catalog.fetch_yarn(&spec, config)?;

                return Ok(Some(fetched.into_version()));
            }
        }

        Ok(self.catalog()?.yarn.default.clone())
    }

    /// Fetches a version of Node matching the specified semantic verisoning
    /// requirements.
    pub fn fetch_yarn(&mut self, matching: &VersionSpec) -> Fallible<Fetched> {
        let catalog = self.catalog.get_mut()?;
        let config = self.config.get()?;
        catalog.fetch_yarn(matching, config)
    }

    /// Sets the Yarn version in the user toolchain to one matching the specified semantic versioning
    /// requirements.
    pub fn set_user_yarn(&mut self, matching: &VersionSpec) -> Fallible<()> {
        let catalog = self.catalog.get_mut()?;
        let config = self.config.get()?;
        catalog.set_user_yarn(matching, config)
    }

    /// Returns the version of Yarn matching the specified semantic versioning requirements
    pub fn get_matching_yarn(&self, matching: &VersionSpec) -> Fallible<Version> {
        let catalog = self.catalog.get()?;
        let config = self.config.get()?;
        catalog.resolve_yarn(matching, config)
    }

    /// Updates toolchain in package.json with the Yarn version matching the specified semantic
    /// versioning requirements.
    pub fn pin_yarn_version(&self, matching: &VersionSpec) -> Fallible<()> {
        if let Some(ref project) = self.project() {
            let yarn_version = self.get_matching_yarn(matching)?;
            project.pin_yarn_in_toolchain(yarn_version)?;
        } else {
            throw!(NotInPackageError::new());
        }
        Ok(())
    }

    pub fn add_event_start(&mut self, activity_kind: ActivityKind) {
        self.event_log.add_event_start(activity_kind)
    }
    pub fn add_event_end(&mut self, activity_kind: ActivityKind, exit_code: ExitCode) {
        self.event_log.add_event_end(activity_kind, exit_code)
    }
    pub fn add_event_tool_end(&mut self, activity_kind: ActivityKind, exit_code: i32) {
        self.event_log.add_event_tool_end(activity_kind, exit_code)
    }
    pub fn add_event_error(&mut self, activity_kind: ActivityKind, error: &NotionError) {
        self.event_log.add_event_error(activity_kind, error)
    }

    fn publish_to_event_log(mut self) {
        match publish_plugin(&self.config) {
            Ok(plugin) => {
                self.event_log.publish(plugin);
            }
            Err(e) => {
                eprintln!("Warning: invalid config file ({})", e);
            }
        }
    }

    pub fn exit(self, code: ExitCode) -> ! {
        self.publish_to_event_log();
        code.exit();
    }

    pub fn exit_tool(self, code: i32) -> ! {
        self.publish_to_event_log();
        exit(code);
    }
}

fn publish_plugin(config: &LazyConfig) -> Fallible<Option<&Publish>> {
    let config = config.get()?;
    Ok(config
        .events
        .as_ref()
        .and_then(|events| events.publish.as_ref()))
}

#[cfg(test)]
pub mod tests {

    use session::Session;
    use std::env;
    use std::path::PathBuf;

    fn fixture_path(fixture_dir: &str) -> PathBuf {
        let mut cargo_manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        cargo_manifest_dir.push("fixtures");
        cargo_manifest_dir.push(fixture_dir);
        cargo_manifest_dir
    }

    #[test]
    fn test_in_pinned_project() {
        let project_pinned = fixture_path("basic");
        env::set_current_dir(&project_pinned).expect("Could not set current directory");
        let pinned_session = Session::new().expect("Couldn't create new Session");
        assert_eq!(pinned_session.in_pinned_project(), true);

        let project_unpinned = fixture_path("no_toolchain");
        env::set_current_dir(&project_unpinned).expect("Could not set current directory");
        let unpinned_session = Session::new().expect("Couldn't create new Session");
        assert_eq!(unpinned_session.in_pinned_project(), false);
    }
}
