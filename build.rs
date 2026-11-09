//! Build-time configuration for the cloud link, plus the bootloader copy.
//!
//! 1. Reads the platform's settings from SSM (iot-platform-infra), with the
//!    `aws` CLI and the credentials in `aws-env.sh` or the environment:
//!    the shared claim certificate and key into `certs/` (gitignored), and the
//!    IoT endpoint and this product's provisioning template into `cfg.toml`
//!    (`[namaz-vakti]`, gitignored). Without access it leaves placeholders and
//!    the build still succeeds; the firmware then keeps the cloud link off.
//! 2. Copies the bootloader this build produced to `<target-dir>/bootloader.bin`
//!    for `espflash` (see [`stage_bootloader`]).

use std::path::Path;

const SECTION: &str = "[namaz-vakti]";
const SSM_TEMPLATE: &str = "/esp32-ztp/poc/vaktus/provisioning_template_name";

fn main() {
    println!(
        "cargo:warning=Attempting to fetch the cloud configuration from AWS SSM Parameter Store..."
    );
    let cert = fetch_ssm_parameter("/esp32-ztp/poc/claim_certificate_pem", true);
    let key = fetch_ssm_parameter("/esp32-ztp/poc/claim_private_key", true);
    let endpoint = fetch_ssm_parameter("/esp32-ztp/poc/iot_endpoint", false);
    let template = fetch_ssm_parameter(SSM_TEMPLATE, false);

    if let (Some(cert), Some(key)) = (&cert, &key) {
        write_file("certs/claim.crt.pem", cert);
        write_file("certs/claim.private.key", key);
        println!("cargo:warning=Updated the claim identity from AWS SSM.");
    } else {
        println!("cargo:warning=Could not fetch the claim identity from SSM; keeping existing files or placeholders.");
        ensure_placeholder(
            "certs/claim.crt.pem",
            "-----BEGIN CERTIFICATE-----\nPLACEHOLDER-no-ssm-access-at-build-time\n-----END CERTIFICATE-----\n",
        );
        ensure_placeholder(
            "certs/claim.private.key",
            "-----BEGIN RSA PRIVATE KEY-----\nPLACEHOLDER-no-ssm-access-at-build-time\n-----END RSA PRIVATE KEY-----\n",
        );
    }

    if let (Some(endpoint), Some(template)) = (endpoint, template) {
        update_cfg_toml(&[
            ("iot_endpoint", &endpoint),
            ("provisioning_template", &template),
        ]);
    } else {
        println!("cargo:warning=Could not fetch the IoT endpoint/template from SSM; the cloud link stays off unless cfg.toml sets them.");
    }

    embuild::espidf::sysenv::output();
    stage_bootloader();
}

/// Copies the second-stage bootloader this build produced to
/// `<target-dir>/bootloader.bin`, where `espflash.toml` and the runner find it.
///
/// `espflash flash` otherwise writes its own bundled bootloader, built without
/// `CONFIG_BOOTLOADER_APP_ROLLBACK_ENABLE`: it ignores OTA image states and
/// never rolls a bad image back. OTA never replaces the bootloader, so what USB
/// flashing writes is what the unit keeps (esp32-opcua-gateway-rust#22).
///
/// esp-idf-sys builds it under its own hashed `OUT_DIR`; the right one is the
/// build directory in the C include arguments it passes on
/// (`-isystem<OUT_DIR>/build/config`).
fn stage_bootloader() {
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    // <target-dir>/<triple>/<profile>/build/<pkg>-<hash>/out
    let target_dir = out_dir
        .ancestors()
        .nth(5)
        .expect("OUT_DIR is <target-dir>/<triple>/<profile>/build/<pkg>-<hash>/out");
    let staged = target_dir.join("bootloader.bin");
    // Never leave an earlier build's bootloader there for espflash to write.
    let _ = std::fs::remove_file(&staged);

    let args = embuild::espidf::sysenv::cincl_args()
        .expect("esp-idf-svc relays esp-idf-sys's C include arguments")
        .args;
    let build_dir = args
        .split_whitespace()
        .filter_map(|arg| arg.trim_matches('"').strip_prefix("-isystem"))
        .map(Path::new)
        .find(|dir| dir.ends_with("build/config"))
        .and_then(Path::parent)
        .unwrap_or_else(|| {
            panic!("no <esp-idf build>/build/config in esp-idf-sys's include arguments: {args}")
        });
    let built = build_dir.join("bootloader").join("bootloader.bin");
    std::fs::copy(&built, &staged).unwrap_or_else(|e| {
        panic!(
            "copying the bootloader {} to {}: {e}",
            built.display(),
            staged.display()
        )
    });
}

fn manifest_path(rel: &str) -> std::path::PathBuf {
    Path::new(&std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR")).join(rel)
}

fn write_file(rel: &str, content: &str) {
    let path = manifest_path(rel);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, content);
}

fn ensure_placeholder(rel: &str, content: &str) {
    if !manifest_path(rel).exists() {
        write_file(rel, content);
        println!(
            "cargo:warning=Placeholder created: {rel}; the cloud link stays off in this build"
        );
    }
}

/// Exports `export KEY=value` lines from `aws-env.sh`, if there is one.
fn load_aws_env_file() {
    let Ok(content) = std::fs::read_to_string(manifest_path("aws-env.sh")) else {
        return;
    };
    for line in content.lines() {
        let Some(stripped) = line.trim().strip_prefix("export ") else {
            continue;
        };
        if let Some((key, val)) = stripped.split_once('=') {
            let val = val.trim().trim_matches(|c| c == '"' || c == '\'');
            std::env::set_var(key.trim(), val);
        }
    }
}

fn fetch_ssm_parameter(name: &str, decrypt: bool) -> Option<String> {
    load_aws_env_file();
    let mut args = vec![
        "ssm",
        "get-parameter",
        "--name",
        name,
        "--query",
        "Parameter.Value",
        "--output",
        "text",
    ];
    if decrypt {
        args.push("--with-decryption");
    }
    let output = std::process::Command::new("aws")
        .args(&args)
        .output()
        .ok()?;
    let val = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (output.status.success() && !val.is_empty() && !val.contains("ParameterNotFound"))
        .then_some(val)
}

/// Sets `key = "value"` lines in the `[namaz-vakti]` section of `cfg.toml`,
/// creating the file (with just that section) or the lines as needed.
fn update_cfg_toml(values: &[(&str, &str)]) {
    let path = manifest_path("cfg.toml");
    // Not from cfg.toml.example: its placeholder WiFi credentials would be
    // compiled in and seeded into NVS on a fresh device.
    let content = std::fs::read_to_string(&path).unwrap_or_else(|_| format!("{SECTION}\n"));
    let mut lines: Vec<String> = content.lines().map(str::to_string).collect();
    if !lines.iter().any(|l| l.trim() == SECTION) {
        lines.insert(0, SECTION.to_string());
    }

    let mut changed = false;
    for (key, value) in values {
        let wanted = format!("{key} = \"{value}\"");
        let existing = lines.iter().position(|l| {
            l.trim_start()
                .strip_prefix(key)
                .is_some_and(|rest| rest.trim_start().starts_with('='))
        });
        match existing {
            Some(i) if lines[i].trim() == wanted => {}
            Some(i) => {
                lines[i] = wanted;
                changed = true;
            }
            None => {
                let section = lines
                    .iter()
                    .position(|l| l.trim() == SECTION)
                    .expect("inserted above");
                lines.insert(section + 1, wanted);
                changed = true;
            }
        }
    }
    if changed {
        let _ = std::fs::write(&path, lines.join("\n") + "\n");
        println!("cargo:warning=Updated cfg.toml with values from SSM Parameter Store.");
    }
}
