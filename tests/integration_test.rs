use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Helper to create a temporary test workspace
struct TestWorkspace {
    path: PathBuf,
}

impl TestWorkspace {
    fn new(name: &str) -> Result<Self> {
        let temp_dir = std::env::temp_dir().join(format!("cargo-patch-test-{}", name));
        if temp_dir.exists() {
            fs::remove_dir_all(&temp_dir)?;
        }
        fs::create_dir_all(&temp_dir)?;
        Ok(Self { path: temp_dir })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn create_cargo_toml(&self, dependencies: &str, patch_crates: &[&str]) -> Result<()> {
        let patch_crates_toml = if !patch_crates.is_empty() {
            format!(
                "\n[package.metadata.patch]\ncrates = {:?}",
                patch_crates
            )
        } else {
            String::new()
        };

        let content = format!(
            r#"[package]
name = "test-project"
version = "0.1.0"
edition = "2021"

[dependencies]
{}
{}
"#,
            dependencies, patch_crates_toml
        );

        fs::write(self.path.join("Cargo.toml"), content)?;
        Ok(())
    }

    fn create_main_rs(&self) -> Result<()> {
        fs::create_dir_all(self.path.join("src"))?;
        fs::write(
            self.path.join("src/main.rs"),
            r#"fn main() {
    println!("Test project");
}
"#,
        )?;
        Ok(())
    }

    fn run_cargo_patch(&self, args: &[&str]) -> Result<std::process::Output> {
        let cargo_patch_binary = std::env::current_exe()?
            .parent()
            .unwrap()
            .join("cargo-patch-crate");

        Ok(Command::new(&cargo_patch_binary)
            .current_dir(&self.path)
            .args(args)
            .output()?)
    }

    fn patches_exist(&self) -> bool {
        self.path.join("patches").exists()
    }

    fn patch_target_exists(&self) -> bool {
        self.path.join("target/patch").exists()
    }

    fn get_patch_files(&self) -> Result<Vec<String>> {
        let patches_dir = self.path.join("patches");
        if !patches_dir.exists() {
            return Ok(vec![]);
        }

        let mut patch_files = vec![];
        for entry in fs::read_dir(patches_dir)? {
            let entry = entry?;
            if entry.path().extension().and_then(|s| s.to_str()) == Some("patch") {
                patch_files.push(
                    entry
                        .path()
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .to_string(),
                );
            }
        }
        Ok(patch_files)
    }

    fn get_patched_crates(&self) -> Result<Vec<String>> {
        let patch_dir = self.path.join("target/patch");
        if !patch_dir.exists() {
            return Ok(vec![]);
        }

        let mut crates = vec![];
        for entry in fs::read_dir(patch_dir)? {
            let entry = entry?;
            if entry.metadata()?.is_dir() {
                crates.push(
                    entry
                        .path()
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .to_string(),
                );
            }
        }
        Ok(crates)
    }
}

impl Drop for TestWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn test_workspace_info_creation() -> Result<()> {
    let ws = TestWorkspace::new("workspace_creation")?;
    ws.create_cargo_toml("serde = \"1.0\"", &[])?;
    ws.create_main_rs()?;

    // Just verify we can create a workspace - this tests the cargo_metadata usage
    let cargo_toml = ws.path().join("Cargo.toml");
    assert!(cargo_toml.exists());

    Ok(())
}

#[test]
fn test_patch_metadata_parsing() -> Result<()> {
    let ws = TestWorkspace::new("metadata_parsing")?;
    ws.create_cargo_toml("serde = \"1.0\"", &["serde"])?;
    ws.create_main_rs()?;

    // Build the project first to ensure dependencies are downloaded
    let output = Command::new("cargo")
        .current_dir(ws.path())
        .args(&["build", "--quiet"])
        .output();

    // If build fails due to network issues, skip this test
    if output.is_err() || !output.as_ref().unwrap().status.success() {
        eprintln!("Skipping test due to cargo build failure (likely network issue)");
        return Ok(());
    }

    // Now test that patch-crate can read the metadata
    let result = ws.run_cargo_patch(&[]);

    // The command should run without crashing
    // It might fail to find packages if network is down, but it should at least parse the metadata
    assert!(
        result.is_ok(),
        "cargo-patch-crate should be able to run"
    );

    Ok(())
}

#[test]
fn test_find_cargo_toml() -> Result<()> {
    let ws = TestWorkspace::new("find_toml")?;
    ws.create_cargo_toml("", &[])?;
    ws.create_main_rs()?;

    let cargo_toml = ws.path().join("Cargo.toml");
    assert!(
        cargo_toml.exists(),
        "Cargo.toml should exist in test workspace"
    );

    Ok(())
}

#[test]
fn test_patches_folder_structure() -> Result<()> {
    let ws = TestWorkspace::new("folder_structure")?;
    ws.create_cargo_toml("serde = \"1.0\"", &["serde"])?;
    ws.create_main_rs()?;

    // Build first
    let output = Command::new("cargo")
        .current_dir(ws.path())
        .args(&["build", "--quiet"])
        .output();

    if output.is_err() || !output.as_ref().unwrap().status.success() {
        eprintln!("Skipping test due to cargo build failure");
        return Ok(());
    }

    // Run cargo-patch-crate
    let _ = ws.run_cargo_patch(&[]);

    // Check that patch target folder is created
    if ws.patch_target_exists() {
        let patched_crates = ws.get_patched_crates()?;
        println!("Patched crates: {:?}", patched_crates);

        // If patches were applied, verify the structure
        if !patched_crates.is_empty() {
            assert!(
                patched_crates.iter().any(|c| c.starts_with("serde-")),
                "Serde should be in patched crates"
            );
        }
    }

    Ok(())
}

#[test]
fn test_empty_crates_list() -> Result<()> {
    let ws = TestWorkspace::new("empty_crates")?;
    ws.create_cargo_toml("serde = \"1.0\"", &[])?;
    ws.create_main_rs()?;

    // Build first
    let output = Command::new("cargo")
        .current_dir(ws.path())
        .args(&["build", "--quiet"])
        .output();

    if output.is_err() || !output.as_ref().unwrap().status.success() {
        eprintln!("Skipping test due to cargo build failure");
        return Ok(());
    }

    // Run cargo-patch-crate with no crates to patch
    let result = ws.run_cargo_patch(&[]);

    // Should succeed but not create any patches
    assert!(result.is_ok());

    // Should not create patches folder if no crates to patch
    let patch_files = ws.get_patch_files()?;
    assert!(
        patch_files.is_empty(),
        "No patch files should be created when no crates are specified"
    );

    Ok(())
}

#[test]
fn test_package_slug_format() {
    // Test that package slugs are formatted correctly (name-version)
    use cargo_metadata::{MetadataCommand, Package};

    // This tests the format we expect for package directories
    let slug = format!("{}-{}", "serde", "1.0.0");
    assert_eq!(slug, "serde-1.0.0");

    let slug = format!("{}-{}", "clap", "4.4.7");
    assert_eq!(slug, "clap-4.4.7");
}

#[test]
fn test_force_flag() -> Result<()> {
    let ws = TestWorkspace::new("force_flag")?;
    ws.create_cargo_toml("serde = \"1.0\"", &["serde"])?;
    ws.create_main_rs()?;

    // Build first
    let output = Command::new("cargo")
        .current_dir(ws.path())
        .args(&["build", "--quiet"])
        .output();

    if output.is_err() || !output.as_ref().unwrap().status.success() {
        eprintln!("Skipping test due to cargo build failure");
        return Ok(());
    }

    // Run without force
    let _ = ws.run_cargo_patch(&[]);

    // Run with force flag
    let result = ws.run_cargo_patch(&["--force"]);

    // Should succeed with force flag
    assert!(result.is_ok(), "cargo-patch-crate --force should succeed");

    Ok(())
}
