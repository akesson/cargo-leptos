use camino::Utf8PathBuf;
use cargo_metadata::{Metadata, Package, Target};

use crate::{
    ext::{
        anyhow::{anyhow, bail, Error, Result},
        MetadataExt, PackageExt, PathBufExt, PathExt,
    },
    Opts,
};

use super::{project::ProjectDefinition, ProjectConfig};

pub struct BinPackage {
    pub name: String,
    pub dir: Utf8PathBuf,
    pub exe_file: Utf8PathBuf,
    pub package: Package,
    pub target: Target,
    pub features: Vec<String>,
    pub default_features: bool,
}

impl BinPackage {
    pub fn resolve(
        cli: &Opts,
        metadata: &Metadata,
        project: &ProjectDefinition,
        config: &ProjectConfig,
    ) -> Result<Self> {
        let features = if !config.bin_features.is_empty() {
            config.bin_features.clone()
        } else if !cli.bin_features.is_empty() {
            cli.bin_features.clone()
        } else {
            vec![]
        };

        let name = project.bin_package.clone();
        let packages = metadata.workspace_packages();
        let package = packages
            .iter()
            .find(|p| p.name == name && p.has_bin_target())
            .ok_or_else(|| anyhow!(r#"Could not find the project bin-package "{name}""#,))?;

        let package = (*package).clone();

        let targets = package
            .targets
            .iter()
            .filter(|t| t.is_bin())
            .collect::<Vec<&Target>>();

        let target: Target = if !&config.bin_target.is_empty() {
            targets
                .into_iter()
                .find(|t| t.name == config.bin_target)
                .ok_or_else(|| target_not_found(config.bin_target.as_str()))?
                .clone()
        } else if targets.len() == 1 {
            targets[0].clone()
        } else if targets.is_empty() {
            bail!("No bin targets found for member {name}");
        } else {
            return Err(many_targets_found(&name));
        };

        let root = metadata.workspace_root.clone();
        let dir = package.manifest_path.clone().without_last().unbase(&root)?;
        let profile = cli.profile();
        let exe_file = {
            let file_ext = if cfg!(target_os = "windows") {
                "exe"
            } else {
                ""
            };
            metadata
                .rel_target_dir()
                .join("server")
                .join(&profile)
                .join(&name)
                .with_extension(file_ext)
        };

        println!(
            "SERVER PATHDEPS: {:?}",
            metadata.src_path_dependencies(&metadata.workspace_root, &package.id)
        );

        Ok(Self {
            name,
            dir,
            exe_file,
            package,
            target,
            features,
            default_features: config.bin_default_features,
        })
    }
}

impl std::fmt::Debug for BinPackage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BinPackage")
            .field("name", &self.name)
            .field("dir", &self.dir)
            .field("exe_file", &self.exe_file.test_string())
            .field("features", &self.features)
            .finish_non_exhaustive()
    }
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