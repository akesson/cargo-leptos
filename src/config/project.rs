use crate::{
    ext::{
        anyhow::{anyhow, Error, Result},
        PackageExt, PathBufExt, PathExt,
    },
    logger::GRAY,
    service::site::Site,
    Opts,
};
use anyhow::{bail, ensure};
use camino::{Utf8Path, Utf8PathBuf};
use cargo_metadata::{Metadata, MetadataCommand, Package, Target};
use serde::Deserialize;
use std::{net::SocketAddr, sync::Arc};

use super::{
    dotenvs::{find_env_file, overlay_env},
    paths::ProjectPaths,
};

#[cfg_attr(not(test), derive(Debug))]
pub struct Project {
    pub name: String,
    pub config: ProjectConfig,
    pub front_package: Package,
    pub front_profile: String,
    pub server_package: Package,
    pub server_target: Target,
    pub server_profile: String,
    pub watch: bool,
    pub site: Arc<Site>,
    pub paths: ProjectPaths,
}

#[cfg(test)]
impl std::fmt::Debug for Project {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Project")
            .field("name", &self.name)
            .field("config", &self.config)
            .field("front_package", &format!("{} ..", self.front_package.name))
            .field("front_profile", &self.front_profile)
            .field(
                "server_package",
                &format!("{} ..", self.server_package.name),
            )
            .field(
                "server_target",
                &format!(
                    "{} ({}) ..",
                    &self.server_target.name,
                    self.server_target.crate_types.join(", ")
                ),
            )
            .field("server_profile", &self.server_profile)
            .field("watch", &self.watch)
            .field("site", &self.site)
            .field("paths", &self.paths)
            .finish_non_exhaustive()
    }
}

impl Project {
    pub fn resolve(cli: &Opts, manifest_path: &Utf8Path, watch: bool) -> Result<Vec<Arc<Project>>> {
        let metadata = MetadataCommand::new().manifest_path(manifest_path).exec()?;

        let projects = ProjectDefinition::parse(&metadata)?;
        let packages = metadata.workspace_packages();

        let mut resolved = Vec::new();
        for (project, mut config) in projects {
            let bin_pkg = &project.bin_package;
            let lib_pkg = &project.lib_package;

            if !cli.bin_features.is_empty() {
                config.bin_features = cli.bin_features.clone();
            }

            if !cli.lib_features.is_empty() {
                config.lib_features = cli.lib_features.clone();
            }

            log::trace!("bin: {bin_pkg}, lib: {lib_pkg}");
            log::trace!(
                "PACKAGES {}",
                packages
                    .iter()
                    .map(|p| p.target_list())
                    .collect::<Vec<_>>()
                    .join(": ")
            );
            let server = packages
                .iter()
                .find(|p| p.name == *bin_pkg && p.has_bin_target())
                .ok_or_else(|| anyhow!(r#"Could not find the project bin-package "{bin_pkg}""#,))?;

            let front = packages
                .iter()
                .find(|p| p.name == *lib_pkg)
                .ok_or_else(|| anyhow!(r#"Could not find the project lib-package "{lib_pkg}""#,))?;

            let bin_targets = server
                .targets
                .iter()
                .enumerate()
                .filter(|(_, t)| t.is_bin())
                .collect::<Vec<(usize, &Target)>>();

            if config.output_name.is_empty() {
                config.output_name = front.name.replace('-', "_");
            }

            let server_target_idx = if !&config.bin_target.is_empty() {
                bin_targets
                    .iter()
                    .find(|(_, t)| t.name == config.bin_target)
                    .ok_or_else(|| target_not_found(config.bin_target.as_str()))?
                    .0
            } else if bin_targets.len() == 1 {
                config.bin_target = bin_targets[0].1.name.to_string();
                bin_targets[0].0
            } else if bin_targets.is_empty() {
                bail!("No bin targets found for member {bin_pkg}");
            } else {
                return Err(many_targets_found(bin_pkg));
            };
            let profile = if cli.release { "release" } else { "dev" };

            let paths = ProjectPaths::new(&metadata, front, server, &config, cli)?;

            let proj = Project {
                name: project.name.clone(),
                front_package: (*front).clone(),
                front_profile: profile.to_string(),
                config,
                server_package: (*server).clone(),
                server_target: server.targets[server_target_idx].clone(),
                server_profile: profile.to_string(),
                watch,
                site: Arc::new(Site::new()),
                paths,
            };
            resolved.push(Arc::new(proj));
        }
        Ok(resolved)
    }

    pub fn optimise_front(&self) -> bool {
        self.front_profile.contains("release")
    }

    pub fn optimise_server(&self) -> bool {
        self.server_profile.contains("release")
    }

    /// env vars to use when running external command
    pub fn to_envs(&self) -> Vec<(&'static str, String)> {
        let mut vec = vec![
            ("OUTPUT_NAME", self.config.output_name.to_string()),
            ("LEPTOS_SITE_ROOT", self.config.site_root.to_string()),
            ("LEPTOS_SITE_PKG_DIR", self.config.site_pkg_dir.to_string()),
            ("LEPTOS_SITE_ADDR", self.config.site_addr.to_string()),
            ("LEPTOS_RELOAD_PORT", self.config.reload_port.to_string()),
        ];
        if self.watch {
            vec.push(("LEPTOS_WATCH", "ON".to_string()))
        }
        vec
    }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct ProjectConfig {
    #[serde(default)]
    pub output_name: String,
    #[serde(default = "default_site_addr")]
    pub site_addr: SocketAddr,
    #[serde(default = "default_site_root")]
    pub site_root: Utf8PathBuf,
    #[serde(default = "default_pkg_dir")]
    pub site_pkg_dir: Utf8PathBuf,
    pub style_file: Option<Utf8PathBuf>,
    /// assets dir. content will be copied to the target/site dir
    pub assets_dir: Option<Utf8PathBuf>,
    #[serde(default = "default_reload_port")]
    pub reload_port: u16,
    /// command for launching end-2-end integration tests
    pub end2end_cmd: Option<String>,
    /// the dir used when launching end-2-end integration tests
    pub end2end_dir: Option<String>,
    #[serde(default = "default_browserquery")]
    pub browserquery: String,
    /// the bin target to use for building the server
    #[serde(default)]
    pub bin_target: String,
    #[serde(default)]
    pub lib_features: Vec<String>,
    #[serde(default)]
    pub lib_default_features: bool,
    #[serde(default)]
    pub bin_features: Vec<String>,
    #[serde(default)]
    pub bin_default_features: bool,
    #[serde(skip)]
    pub config_dir: Utf8PathBuf,
}

impl ProjectConfig {
    fn parse(dir: &Utf8Path, metadata: &serde_json::Value) -> Result<Self> {
        let mut conf: ProjectConfig = serde_json::from_value(metadata.clone())?;
        conf.config_dir = dir.to_path_buf();
        if let Some(file) = find_env_file(dir) {
            overlay_env(&mut conf, &file)?;
        }
        Ok(conf)
    }
}
fn default_site_addr() -> SocketAddr {
    SocketAddr::new([127, 0, 0, 1].into(), 3000)
}

fn default_pkg_dir() -> Utf8PathBuf {
    Utf8PathBuf::from("pkg")
}

fn default_site_root() -> Utf8PathBuf {
    Utf8PathBuf::from("target/site")
}

fn default_reload_port() -> u16 {
    3001
}

fn default_browserquery() -> String {
    "defaults".to_string()
}

fn many_targets_found(pkg: &str) -> Error {
    anyhow!(
        r#"Several bin targets found for member "{pkg}", please specify which one to use with: [[workspace.metadata.leptos]] bin-target = "name""#
    )
}
fn target_not_found(target: &str) -> Error {
    anyhow!(
        r#"Could not find the target specified: [[workspace.metadata.leptos]] bin-target = "{target}""#,
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ProjectDefinition {
    name: String,
    bin_package: String,
    lib_package: String,
}
impl ProjectDefinition {
    fn from_workspace(
        metadata: &serde_json::Value,
        dir: &Utf8Path,
    ) -> Result<Vec<(Self, ProjectConfig)>> {
        let mut found = Vec::new();
        if let Some(arr) = metadata.as_array() {
            for section in arr {
                let conf = ProjectConfig::parse(dir, section)?;
                let def: Self = serde_json::from_value(section.clone())?;
                found.push((def, conf))
            }
        }
        Ok(found)
    }

    fn from_project(
        package: &Package,
        metadata: &serde_json::Value,
        dir: &Utf8Path,
    ) -> Result<(Self, ProjectConfig)> {
        let conf = ProjectConfig::parse(dir, metadata)?;

        ensure!(
            package.cdylib_target().is_some(),
            "Cargo.toml has leptos metadata but is missing a cdylib library target. {}",
            GRAY.paint(package.manifest_path.as_str())
        );
        ensure!(
            package.has_bin_target(),
            "Cargo.toml has leptos metadata but is missing a bin target. {}",
            GRAY.paint(package.manifest_path.as_str())
        );

        Ok((
            ProjectDefinition {
                name: package.name.to_string(),
                bin_package: package.name.to_string(),
                lib_package: package.name.to_string(),
            },
            conf,
        ))
    }

    fn parse(metadata: &Metadata) -> Result<Vec<(Self, ProjectConfig)>> {
        let workspace_dir = &metadata.workspace_root;
        let mut found: Vec<(Self, ProjectConfig)> =
            if let Some(md) = leptos_metadata(&metadata.workspace_metadata) {
                Self::from_workspace(md, &Utf8PathBuf::default())?
            } else {
                Default::default()
            };

        for package in metadata.workspace_packages() {
            let dir = package.manifest_path.unbase(workspace_dir)?.without_last();

            if let Some(metadata) = leptos_metadata(&package.metadata) {
                found.push(Self::from_project(package, metadata, &dir)?);
            }
        }
        Ok(found)
    }
}

fn leptos_metadata(metadata: &serde_json::Value) -> Option<&serde_json::Value> {
    metadata.as_object().map(|o| o.get("leptos")).flatten()
}
