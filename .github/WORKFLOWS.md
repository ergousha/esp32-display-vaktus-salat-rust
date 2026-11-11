# GitHub Configuration

This directory contains GitHub-specific configurations for automated workflows and best practices.

## Files Overview

### Workflows (`.github/workflows/`)

#### `rust.yml`
CI pipeline for the `logic` crate (`logic/`) — the pure, hardware-free
calendar/prayer-time logic split out of the firmware so it can be built,
linted, and unit tested with a normal stable host Rust toolchain, with no
ESP32 device or `esp`/ESP-IDF toolchain required (see [issue #2](https://github.com/ergousha/4.0inch_ESP32_Rust_NamazVakti/issues/2)).
The `namaz-vakti` firmware package at the repo root can only be built with
the `esp` toolchain + ESP-IDF SDK, so it isn't checked here — see
`esp32-build.yml` instead.
- **Cargo check**: Verify the logic crate compiles
- **Rustfmt**: Enforce code formatting
- **Clippy**: Linting and best practices
- **Tests**: Run the logic crate's unit test suite
- **Documentation**: Check doc comments and build docs
- **Build**: Release build verification

Triggers on: `push` to main/develop, `pull_request` to main/develop

#### `esp32-build.yml`
ESP32 firmware build (Ubuntu, `esp-rs/xtensa-toolchain`):
- Builds the firmware and the `atecc608_probe` bench example for the ESP32
- Packs the OTA app image (`cargo espflash save-image`, `partitions.csv`) and
  **fails if it takes more than 90% of an OTA slot**: the slot size is frozen in
  the field, so an update must always fit. The size goes to the job summary.
- Builds without AWS access, so the image has the cloud link off.

Triggers on: `push` to main/develop, `pull_request` to main/develop

#### `security-audit.yml`
Security vulnerability scanning:
- Uses `rustsec/audit-check-action` to check dependencies
- Scheduled weekly on Mondays

Triggers on: `push` to main/develop, `pull_request` to main/develop, weekly schedule

#### `release.yml`
Every push to `main` calls the platform's reusable release workflow
(`ergousha/iot-device-sdk/.github/workflows/firmware-release.yml`, pinned by
commit SHA; Dependabot bumps it):
- **version**: the next semver from the conventional commits since the last
  `v*` tag. Only `feat:` (minor) and `fix:` (patch) release; a breaking change
  is a minor bump while below 1.0.
- **build**: no secrets, no AWS, no cache. `app.bin`, `factory.bin` (first
  flash), `manifest.json`, an SBOM; the slot-size check and the image scan.
- **publish** (Environment `release`): signs the manifest, tags `vX.Y.Z`,
  creates a GitHub pre-release with the images, and uploads them to
  `s3://<bucket>/releases/vaktus/X.Y.Z/` through the OIDC role.
- **notify**: on failure, an issue "release vX.Y.Z failed".

The platform's OTA controller takes it from there: canary, soak, fleet, with
an email at each step. There is no release PR and no GitHub App.

### Automation

#### `dependabot.yml`
Automated dependency updates:
- **Cargo**: Updates Rust dependencies weekly on Mondays
- **GitHub Actions**: Updates workflow action versions weekly

Auto-generated PRs with labels `dependencies` and appropriate ecosystem tags.

### Configuration Files

#### `CODEOWNERS`
Defines code ownership and review requirements:
- `eakin` is owner of all files by default
- Required for PR reviews

#### `pull_request_template.md`
Template shown when creating pull requests:
- Enforces consistent PR descriptions
- Includes checklist for code quality

## Setup Instructions

### 1. Branch Protection Rules

Configure on GitHub via **Settings → Branches → Branch protection rules**:

**For `main` branch:**

1. Require a pull request before merging:
   - ✅ Require approvals (1 minimum)
   - ✅ Require status checks to pass before merging:
     - `Cargo check (logic crate)`
     - `Rustfmt (logic crate)`
     - `Clippy (logic crate)`
     - `Tests (logic crate)`
     - `Documentation (logic crate)`
     - `Build (logic crate)`
     - `esp32-build (Build for ESP32)`
     - `security_audit (Security Audit)`
   - ✅ Require branches to be up to date before merging
   - ✅ Require code reviews before merging
   - ✅ Require approval of the most recent reviewable push
   - ✅ Require status checks to pass before merging
   - ✅ Dismiss stale pull request approvals when new commits are pushed
   - ✅ Require conversation resolution before merging

2. Include administrators: Consider whether to enforce on admins

3. Restrictions: Optional - restrict who can push to main

### 2. Enable Dependabot

1. Go to **Settings → Code security & analysis**
2. Enable **Dependabot alerts**
3. Enable **Dependabot security updates**
4. Enable **Dependabot version updates** (uses `dependabot.yml`)

### 3. Configure CODEOWNERS

1. The `.github/CODEOWNERS` file is already created
2. Go to **Settings → Branches → Branch protection rules → main**
3. ✅ Enable "Require code reviews from Code Owners"

### 4. Pull Request Settings

1. Go to **Settings → General**
2. Under "Pull Requests":
   - ✅ Allow auto-merge
   - ✅ Allow squash merging
   - ✅ Allow rebase merging

### 5. Actions Settings

1. Go to **Settings → Actions → General**
2. Under "Workflow permissions":
   - Select "Read and write permissions"
   - ✅ Allow GitHub Actions to create and approve pull requests

## Workflow Triggers

| Workflow | Push | PR | Schedule | Tag |
|----------|------|----|-----------|----|
| rust.yml | ✅ | ✅ | - | - |
| esp32-build.yml | ✅ | ✅ | - | - |
| security-audit.yml | ✅ | ✅ | Weekly | - |
| release.yml | ✅ (main, every push) | - | - | - |

## Local Development

All workflows can be run locally:

```bash
# Format check
cargo fmt --all -- --check

# Clippy
cargo clippy --all-targets --all-features -- -D warnings

# Tests
cargo test --verbose

# Documentation
cargo doc --no-deps --document-private-items

# Build
cargo build --release
```

## Dependabot Configuration

Dependabot creates weekly PRs for:
- Direct and indirect Rust dependencies
- GitHub Actions

PRs are labeled with `dependencies` tag and assigned to `eakin`.

## Security Scanning

- Runs weekly security audits via `rustsec`
- Also checks on all PRs and pushes
- Blocks merging if vulnerabilities are found

## Release Process

Merge a pull request whose title is a conventional commit: `fix: ...` releases
a patch, `feat: ...` a minor version, and anything else (`docs:`, `chore:`,
`ci:`, `refactor:`) nothing. Dependabot's cargo bumps are `fix(deps):`, so a
merged dependency update ships; its GitHub Actions bumps are `ci:` and do not.
Every release goes to the canary first; the fleet follows after the soak
without a click. `iot-platform/scripts/ota.sh status vaktus` shows where it
is; `hold`, `promote`, `rerun` and `rollback` are the overrides.

### One-time setup

1. Repository variables, from the iot-platform Terraform outputs:
   ```bash
   gh variable set AWS_ROLE_ARN --body "$(terraform -chdir=../iot-platform-infra output -json github_actions_role_arns | jq -r .vaktus)"
   gh variable set AWS_FIRMWARE_BUCKET --body "$(terraform -chdir=../iot-platform-infra output -raw firmware_bucket_name)"
   ```
2. Signing keys and Environment `release` (main only, secret
   `FIRMWARE_SIGNING_KEY`), from an iot-device-sdk checkout:
   ```bash
   scripts/signing-keys.sh ../esp32-display-vaktus-salat-rust
   ```
   Commit the `keys/` and `platform.toml` it writes with a `fix:` title.
3. Optional: `gh variable set IMAGE_SCAN_FORBIDDEN --body "<home SSID>"` makes
   the image scan fail on that string.
4. Remove what release-please needed: the `RELEASE_PLEASE_APP_CLIENT_ID`
   variable, the `RELEASE_PLEASE_APP_PRIVATE_KEY` secret, and the GitHub App
   itself once no repository uses it.

## Maintenance

- Review Dependabot PRs weekly
- Monitor workflow runs for failures
- Update workflows when GitHub Actions versions update
- Check security audit results regularly
