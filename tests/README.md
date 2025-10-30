# Tests for cargo-patch-crate

This directory contains tests for the cargo-patch-crate tool, covering both the old implementation (using the `cargo` crate) and the new implementation (using `cargo_metadata`).

## Test Structure

### Unit Tests (`unit_test.rs`)
These tests can run without network access and test core functionality:
- File and directory structure handling
- Patch filename parsing (format: `name+version.patch`)
- Package slug format (format: `name-version`)
- TOML metadata parsing
- CLI argument handling
- Git directory detection

These tests ensure that the basic logic and data structures work correctly regardless of the underlying implementation.

### Integration Tests (`integration_test.rs`)
These tests require:
- Network access to download dependencies from crates.io
- A working Rust/Cargo installation
- Git installed

They test end-to-end functionality including:
- Creating test workspaces
- Applying patches to dependencies
- Creating patch files from modified dependencies
- Force flag behavior
- Metadata configuration parsing

## Running Tests

### Running Unit Tests Only
```bash
cargo test --test unit_test
```

### Running All Tests (requires network access)
```bash
cargo test
```

### Running Specific Tests
```bash
# Run a specific test
cargo test test_patch_file_naming

# Run tests matching a pattern
cargo test test_metadata
```

## Testing Both Implementations

### Testing the New Implementation (cargo_metadata)
```bash
# On branch claude/implement-issue-6-011CUe5YyZPax6WCHQRFKhht
cargo test --test unit_test
```

### Testing the Old Implementation (cargo crate)
```bash
# Checkout the old implementation
git checkout 7cc5142

# Copy the test files
mkdir -p tests
git show claude/implement-issue-6-011CUe5YyZPax6WCHQRFKhht:tests/unit_test.rs > tests/unit_test.rs

# Add toml dev-dependency to Cargo.toml
echo '[dev-dependencies]' >> Cargo.toml
echo 'toml = "0.8"' >> Cargo.toml

# Run tests (network access required for dependencies)
cargo test --test unit_test
```

## What Changed Between Implementations

### Old Implementation (cargo crate)
- Used `cargo::core::Workspace` for workspace management
- Used `cargo::core::resolver` for dependency resolution
- Used `cargo::ops` for package operations
- Required heavy `cargo = "0.81"` dependency with hundreds of transitive dependencies
- Longer build times due to dependency size

### New Implementation (cargo_metadata)
- Uses `cargo_metadata::MetadataCommand` to execute `cargo metadata` command
- Parses JSON output from cargo metadata
- Finds package sources in `~/.cargo/registry/src/`
- Uses lightweight `cargo_metadata = "0.18"` dependency
- Much faster build times
- Fewer transitive dependencies

### Functionality Preserved
Both implementations support:
- Reading `[package.metadata.patch]` configuration
- Copying dependencies to `target/patch/`
- Creating patch files in `patches/` directory
- Applying patches to dependencies
- Force flag to clean and rebuild
- Multiple workspace members
- Git-based patch creation and application

## Expected Test Results

All unit tests should pass on both implementations as they test the common logic that doesn't depend on how we fetch package metadata.

Integration tests require network access and will skip tests if:
- Network is unavailable
- crates.io is unreachable
- Cargo build fails

## CI/CD Considerations

When setting up CI/CD:
1. Unit tests can run in any environment (no network needed)
2. Integration tests need network access and cargo/git installed
3. Consider caching `~/.cargo` to speed up test runs
4. Test both on Linux, macOS, and Windows as the tool handles Windows paths specially

## Manual Testing Checklist

Beyond automated tests, manually verify:
- [ ] Create a patch from a modified dependency
- [ ] Apply a patch to a fresh checkout
- [ ] Use `--force` flag to rebuild patches
- [ ] Multiple crates in `[package.metadata.patch]`
- [ ] Warning when patch file exists without metadata entry
- [ ] Windows path handling (if on Windows)
- [ ] Workspace with multiple members
- [ ] Path dependencies vs registry dependencies
