//!
//! patch-crate lets rust app developer instantly make and keep fixes to crate dependencies.
//! It's a vital band-aid for those of us living on the bleeding edge.
//!
//! # Installation
//!
//! Simply run:
//! ```sh
//! cargo install patch-crate
//! ```
//!
//! # Usage
//!
//! To patch dependency one has to add the following
//! to `Cargo.toml`
//!
//! ```toml
//! [package.metadata.patch]
//! crates = ["serde"]
//! ```
//!
//! It specifies which dependency to patch (in this case
//! serde). Running:
//!
//! ```sh
//! cargo patch-crate
//! ```
//!
//! will download the sede package specified in the
//! dpendency section to the `target/patch` folder.
//!
//! Then override the dependency using
//! `replace` like this
//!
//! ```toml
//! [patch.crates-io]
//! serde = { path = './target/patch/serde-1.0.110' }
//! ```
//!
//! fix a bug in './target/patch/serde-1.0.110' directly.
//!
//! run following to create a `patches/serde+1.0.110.patch` file
//! ```sh
//! cargo patch-crate serde
//! ```
//!
//! commit the patch file to share the fix with your team
//! ```sh
//! git add patches/serde+1.0.110.patch
//! git commit -m "fix broken-serde in serde"
//! ```

use anyhow::{anyhow, Context, Result};
use cargo_metadata::{MetadataCommand, Package, PackageId};
use clap::Parser;
use fs_extra::dir::{copy, CopyOptions};
use log::*;
use std::{
    collections::{HashMap, HashSet},
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

const PATCH_EXT: &str = "patch";

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    crates: Vec<String>,
    #[arg(short, long)]
    force: bool,
}

struct WorkspaceInfo {
    root: PathBuf,
    metadata: cargo_metadata::Metadata,
    packages_by_name: HashMap<String, Package>,
}

impl WorkspaceInfo {
    fn new(manifest_path: Option<PathBuf>) -> Result<Self> {
        let mut cmd = MetadataCommand::new();
        if let Some(path) = manifest_path {
            cmd.manifest_path(path);
        }
        let metadata = cmd.exec()?;

        let packages_by_name = metadata
            .packages
            .iter()
            .map(|pkg| (pkg.name.clone(), pkg.clone()))
            .collect();

        Ok(Self {
            root: metadata.workspace_root.clone().into(),
            metadata,
            packages_by_name,
        })
    }

    fn patches_folder(&self) -> PathBuf {
        self.root.join("patches/")
    }

    fn patch_target_folder(&self) -> PathBuf {
        self.root.join("target/patch/")
    }

    fn patch_target_tmp_folder(&self) -> PathBuf {
        self.root.join("target/patch-tmp/")
    }

    fn clean_patch_folder(&self) -> Result<()> {
        let path = self.patch_target_folder();
        if path.exists() {
            fs::remove_dir_all(self.patch_target_folder())?;
        }
        Ok(())
    }

    fn get_package(&self, name: &str) -> Result<&Package> {
        self.packages_by_name
            .get(name)
            .ok_or_else(|| anyhow!("Package '{}' not found in dependencies", name))
    }

    fn get_crates_to_patch(&self) -> Result<Vec<String>> {
        let mut crates_to_patch = Vec::new();

        // Read custom metadata from workspace and members
        for package_id in &self.metadata.workspace_members {
            if let Some(package) = self.metadata.packages.iter().find(|p| &p.id == package_id) {
                if let Some(patch_metadata) = package.metadata.get("patch") {
                    if let Some(crates) = patch_metadata.get("crates") {
                        if let Some(crates_array) = crates.as_array() {
                            for crate_value in crates_array {
                                if let Some(crate_name) = crate_value.as_str() {
                                    crates_to_patch.push(crate_name.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(crates_to_patch)
    }

    fn find_package_source(&self, package: &Package) -> Result<PathBuf> {
        // First try: check if it's a path dependency (already on disk)
        for node in &self.metadata.resolve.as_ref().unwrap().nodes {
            if node.id == package.id {
                if let Some(manifest_dir) = package.manifest_path.parent() {
                    let manifest_dir: PathBuf = manifest_dir.into();
                    if manifest_dir.exists() {
                        return Ok(manifest_dir);
                    }
                }
            }
        }

        // Second try: look in cargo's registry
        if let Some(home_dir) = home::home_dir() {
            let registry_src = home_dir.join(".cargo/registry/src");
            if registry_src.exists() {
                for entry in fs::read_dir(&registry_src)? {
                    let entry = entry?;
                    if entry.metadata()?.is_dir() {
                        let source_dir = entry.path();
                        let package_dir = source_dir.join(format!("{}-{}", package.name, package.version));
                        if package_dir.exists() {
                            return Ok(package_dir);
                        }
                    }
                }
            }
        }

        Err(anyhow!(
            "Could not find source for package {} version {}. Try running 'cargo build' first.",
            package.name,
            package.version
        ))
    }
}

fn get_package_slug(package: &Package) -> String {
    format!("{}-{}", package.name, package.version)
}

fn copy_package(
    workspace: &WorkspaceInfo,
    package: &Package,
    patch_target_folder: &Path,
    overwrite: bool,
) -> Result<PathBuf> {
    fs::create_dir_all(patch_target_folder)?;
    let slug = get_package_slug(package);
    let patch_target_path = patch_target_folder.join(&slug);

    if patch_target_path.exists() {
        if overwrite {
            info!("crate: {}, copy to {:?}", package.name, &patch_target_folder);
            fs::remove_dir_all(&patch_target_path)?;
        } else {
            info!(
                "crate: {}, skip, {:?} already exists.",
                package.name, &patch_target_path
            );
            return Ok(patch_target_path);
        }
    }

    let source_path = workspace.find_package_source(package)?;
    info!("crate: {}, copying from {:?}", package.name, &source_path);

    let options = CopyOptions::new();
    let _ = copy(&source_path, patch_target_folder, &options)?;

    Ok(patch_target_path)
}

fn find_cargo_toml(path: &Path) -> Result<PathBuf> {
    let mut current = fs::canonicalize(path)?;
    loop {
        let cargo_toml = current.join("Cargo.toml");
        if cargo_toml.exists() {
            return Ok(cargo_toml);
        }
        if !current.pop() {
            break;
        }
    }
    Err(anyhow!("Could not find Cargo.toml in current directory or any parent directory"))
}

pub fn run() -> anyhow::Result<()> {
    let args = {
        let mut args = Cli::parse();
        if let Some(idx) = args.crates.iter().position(|c| c == "patch-crate") {
            args.crates.remove(idx);
        }
        args
    };

    let cargo_toml_path = find_cargo_toml(&PathBuf::from("."))?;
    let workspace = WorkspaceInfo::new(Some(cargo_toml_path))?;

    let patches_folder = workspace.patches_folder();
    let patch_target_folder = workspace.patch_target_folder();
    let patch_target_tmp_folder = workspace.patch_target_tmp_folder();

    if !args.crates.is_empty() {
        info!("starting patch creation.");
        if !patches_folder.exists() {
            fs::create_dir_all(&patches_folder)?;
        }

        for crate_name in args.crates.iter() {
            // make patch
            info!("crate: {}, starting patch creation.", crate_name);
            let package = workspace.get_package(crate_name)?;
            let slug = get_package_slug(package);
            let patch_target_path = patch_target_folder.join(&slug);

            let patch_target_tmp_path = copy_package(&workspace, package, &patch_target_tmp_folder, true)?;
            git::init(&patch_target_tmp_path)?;
            git::destroy(&patch_target_path)?;
            copy(
                &patch_target_path,
                &patch_target_tmp_folder,
                &CopyOptions::new().overwrite(true).copy_inside(true),
            )?;

            let patch_file = patches_folder.join(format!(
                "{}+{}.{}",
                package.name,
                package.version,
                PATCH_EXT
            ));
            git::create_patch(&patch_target_tmp_path, &patch_file)?;
            fs::remove_dir_all(&patch_target_tmp_folder)?;
            info!("crate: {}, create patch successfully, {:?}", crate_name, &patch_file);
        }
    } else {
        // apply patch
        info!("applying patch");

        let crate_names_to_patch = workspace.get_crates_to_patch()?;
        let mut crates_to_patch: HashSet<String> = crate_names_to_patch.into_iter().collect();

        if args.force {
            info!("Cleaning up patch folder.");
            workspace.clean_patch_folder()?;
        }

        if patches_folder.exists() {
            for entry in fs::read_dir(patches_folder)? {
                let entry = entry?;
                if entry.metadata()?.is_file()
                    && entry.path().extension() == Some(OsStr::new(PATCH_EXT))
                {
                    let patch_file = entry.path();
                    let filename = patch_file
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .ok_or(anyhow!("Patch file does not have a name"))?;

                    if let Some((pkg_name, _version)) = filename.split_once('+') {
                        let package = workspace.get_package(pkg_name)?;

                        if !crates_to_patch.contains(pkg_name) {
                            warn!(
                                "crate: {}, {} is not in the [package.metadata.patch] section of Cargo.toml. Did you forget to add it?",
                                pkg_name, pkg_name
                            );
                            continue;
                        }

                        let slug = get_package_slug(package);
                        let patch_target_path = patch_target_folder.join(&slug);

                        if !patch_target_path.exists() {
                            copy_package(&workspace, package, &patch_target_folder, args.force)?;
                            info!("crate: {}, applying patch started.", pkg_name);
                            git::init(&patch_target_path)?;
                            git::apply(&patch_target_path, &patch_file)?;
                            git::destroy(&patch_target_path)?;
                            info!(
                                "crate: {}, successfully applied patch {:?}.",
                                pkg_name, patch_file
                            );
                        } else {
                            info!("crate: {}, skip applying patch, {:?} already exists. Did you forget to add `--force`?", pkg_name, patch_target_path);
                        }
                        crates_to_patch.remove(pkg_name);
                    }
                }
            }
        }

        for crate_name in crates_to_patch {
            let package = workspace.get_package(&crate_name)?;
            copy_package(&workspace, package, &patch_target_folder, args.force)?;
        }
    }

    info!("Done");
    Ok(())
}

mod log {
    pub use paris::*;
}

mod git {
    use std::{ffi::OsStr, fs, path::Path, process::Command};

    pub fn init(repo_dir: &Path) -> anyhow::Result<()> {
        Command::new("git")
            .current_dir(repo_dir)
            .args(["init"])
            .output()?;
        Command::new("git")
            .current_dir(repo_dir)
            .args(["add", "."])
            .output()?;
        Command::new("git")
            .current_dir(repo_dir)
            .args(["commit", "-m", "zero"])
            .output()?;
        Ok(())
    }

    pub fn apply(repo_dir: &Path, patch_file: &Path) -> anyhow::Result<()> {
        #[cfg(target_os = "windows")]
        let patch_file = patch_file
            .to_string_lossy()
            .to_string()
            .trim_start_matches(r#"\\?\"#)
            .to_string();
        #[cfg(not(target_os = "windows"))]
        let patch_file = patch_file.to_string_lossy().to_string();

        let out = Command::new("git")
            .current_dir(repo_dir)
            .args([
                "apply",
                "--ignore-space-change",
                "--ignore-whitespace",
                "--whitespace=nowarn",
                &patch_file,
            ])
            .output()?;

        if !out.status.success() {
            anyhow::bail!(String::from_utf8(out.stderr)?)
        }
        Ok(())
    }
    pub fn destroy(repo_dir: &Path) -> anyhow::Result<()> {
        let git_dir = repo_dir.join(".git");
        if git_dir.exists() {
            fs::remove_dir_all(git_dir)?;
        }
        Ok(())
    }
    pub fn create_patch(repo_dir: &Path, patch_file: &Path) -> anyhow::Result<()> {
        Command::new("git")
            .current_dir(repo_dir)
            .args(["add", "."])
            .output()?;

        let out = Command::new("git")
            .current_dir(repo_dir)
            .args([OsStr::new("diff"), OsStr::new("--staged")])
            .output()?;

        if out.status.success() {
            fs::write(patch_file, out.stdout)?;
        }
        Ok(())
    }
}
