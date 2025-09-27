use clap::Parser;
use sha2::{Digest, Sha256};
use std::io::Cursor;
use std::path::Path;
use std::process;
use tracing::{error, info};
use serde_json;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    repo: String,
    #[arg(long)]
    sha: String,
    #[arg(long)]
    package: String,
    #[arg(long)]
    hash: String,
    #[arg(long, default_value = ".")]
    dir: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let args = Args::parse();

    let metadata_url = format!("https://github.com/leighmcculloch/stellar-contract-wasms/raw/refs/heads/main/meta/{}.json", args.hash);
    info!("Fetching Wasm metadata for hash {} from {}", args.hash, metadata_url);
    let response = reqwest::blocking::get(&metadata_url)?;
    let metadata_bytes = response.bytes()?;
    let json: serde_json::Value = serde_json::from_slice(&metadata_bytes)?;
    let mut toolchain = String::new();
    if let Some(arr) = json.as_array() {
        for item in arr {
            if let Some(meta) = item.get("sc_meta_v0") {
                if meta.get("key").and_then(|k| k.as_str()) == Some("rsver") {
                    if let Some(val) = meta.get("val").and_then(|v| v.as_str()) {
                        toolchain = val.to_string();
                        break;
                    }
                }
            }
        }
    }
    if toolchain.is_empty() {
        error!("Could not find Rust toolchain version (rsver) in Wasm metadata for hash {}", args.hash);
        process::exit(1);
    }
    info!("Using Rust toolchain {}", toolchain);

    info!("Installing Rust toolchain {}", toolchain);
    let install_status = process::Command::new("rustup")
        .args(&["install", &toolchain])
        .status()?;
    if !install_status.success() {
        error!("Failed to install Rust toolchain {}", toolchain);
        process::exit(1);
    }
    info!("Successfully installed Rust toolchain {}", toolchain);

    let version_parts: Vec<&str> = toolchain.split('.').collect();
    if version_parts.len() >= 2 {
        let version_str = format!("{}.{}", version_parts[0], version_parts[1]);
        if let Ok(version) = version_str.parse::<f32>() {
            let target = if version > 1.84 {
                "wasm32v1-none"
            } else {
                "wasm32-unknown-unknown"
            };
            info!("Adding target {} to toolchain {}", target, toolchain);
            let add_status = process::Command::new("rustup")
                .args(&["target", "add", target, "--toolchain", &toolchain])
                .status()?;
            if !add_status.success() {
                error!("Failed to add target {} to toolchain {}", target, toolchain);
                process::exit(1);
            }
            info!("Successfully added target {} to toolchain {}", target, toolchain);
        } else {
            error!("Failed to parse Rust toolchain version '{}' as a valid semantic version", toolchain);
            process::exit(1);
        }
    } else {
        error!("Invalid Rust toolchain version format '{}' (expected format: x.y.z)", toolchain);
        process::exit(1);
    }

    let code_path = Path::new("code");
    let wasm_path = Path::new("wasm");

    let build_dir = code_path.join(&args.dir);

    let parts: Vec<&str> = args.repo.split('/').collect();
    if parts.len() != 2 {
        error!("Invalid repository format '{}' (expected 'owner/repo' format)", args.repo);
        process::exit(1);
    }
    let owner = parts[0];
    let repo = parts[1];

    let url = format!("https://github.com/{}/{}/archive/{}.tar.gz", owner, repo, args.sha);

    info!("Downloading archive from {}", url);
    // Download the archive
    let response = reqwest::blocking::get(&url)?;
    let bytes = response.bytes()?;
    info!("Successfully downloaded {} bytes", bytes.len());

    // Create code directory if it doesn't exist
    info!("Creating code directory");
    std::fs::create_dir_all(code_path)?;
    info!("Successfully created code directory");

    // Extract the archive
    info!("Extracting source code archive");
    let tar = flate2::read::GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(tar);
    archive.unpack(code_path)?;
    info!("Successfully extracted source code archive");

    // Find the extracted directory and move its contents up
    info!("Moving extracted contents to code directory");
    let mut extracted_dirs: Vec<_> = std::fs::read_dir(code_path)?
        .filter_map(|e| e.ok())
        .collect();
    if extracted_dirs.len() == 1 {
        let extracted_dir = extracted_dirs.remove(0).path();
        if extracted_dir.is_dir() {
            for entry in std::fs::read_dir(&extracted_dir)? {
                let entry = entry?;
                let target = code_path.join(entry.file_name());
                std::fs::rename(entry.path(), target)?;
            }
            std::fs::remove_dir(extracted_dir)?;
        }
    }
    info!("Successfully moved contents to code directory");

    // Run stellar contract build
    info!("Building Stellar contract '{}' in directory {}", args.package, build_dir.display());
    let output = process::Command::new("stellar")
        .args(&["contract", "build", "--package", &args.package, "--out-dir", "../wasm/"])
        .env("RUSTUP_TOOLCHAIN", &toolchain)
        .current_dir(&build_dir)
        .output()?;
    info!("Build command completed with exit code {}", output.status.code().unwrap_or(-1));
    if !output.status.success() {
        error!("Stellar contract build failed with exit code {}", output.status.code().unwrap_or(-1));
        if !output.stdout.is_empty() {
            error!("Build stdout: {}", String::from_utf8_lossy(&output.stdout));
        }
        if !output.stderr.is_empty() {
            error!("Build stderr: {}", String::from_utf8_lossy(&output.stderr));
        }
        if output.stdout.is_empty() && output.stderr.is_empty() {
            error!("No output captured from build command");
        }
        process::exit(output.status.code().unwrap_or(1));
    }

    // Compute SHA256 hash of the output Wasm
    info!("Computing hash of Wasm file");
    let package_name = args.package.replace("-", "_");
    let wasm_file = wasm_path.join(format!("{}.wasm", package_name));
    info!("Wasm file path: {}", wasm_file.display());

    // Optimize the Wasm
    info!("Optimizing Wasm file");
    let optimize_status = process::Command::new("stellar")
        .args(&["contract", "optimize", "--wasm", &wasm_file.to_string_lossy()])
        .status()?;
    info!("Wasm optimization completed with exit code {}", optimize_status.code().unwrap_or(-1));
    if !optimize_status.success() {
        error!("Wasm optimization failed with exit code {}", optimize_status.code().unwrap_or(-1));
        process::exit(optimize_status.code().unwrap_or(1));
    }

    // Read Wasm file - try unoptimized first, then optimized as fallback
    info!("Attempting to read unoptimized Wasm file: {}", wasm_file.display());
    let (wasm_bytes, used_file) = match std::fs::read(&wasm_file) {
        Ok(bytes) => {
            info!("Successfully read unoptimized Wasm file ({} bytes)", bytes.len());
            (bytes, "unoptimized")
        }
        Err(e) => {
            info!("Unoptimized Wasm file not found or unreadable ({}), trying optimized version", e);
            let optimized_file = wasm_path.join(format!("{}.optimized.wasm", package_name));
            info!("Attempting to read optimized Wasm file: {}", optimized_file.display());
            match std::fs::read(&optimized_file) {
                Ok(bytes) => {
                    info!("Successfully read optimized Wasm file ({} bytes)", bytes.len());
                    (bytes, "optimized")
                }
                Err(e2) => {
                    error!("Failed to read Wasm file - tried both unoptimized ({}) and optimized ({}) versions: unoptimized error: {}, optimized error: {}", wasm_file.display(), optimized_file.display(), e, e2);
                    process::exit(1);
                }
            }
        }
    };

    // Compute hash of the selected file
    info!("Computing SHA256 hash of {} Wasm file", used_file);
    let hash = Sha256::digest(&wasm_bytes);
    let hash_str = hex::encode(hash);
    info!("Computed SHA256 hash of {} Wasm file: {}", used_file, hash_str);

    // Verify hash against expected value
    info!("Verifying {} Wasm file hash against expected hash: {}", used_file, args.hash);
    if hash_str == args.hash {
        info!("✓ Hash verification successful using {} Wasm file", used_file);
    } else {
        info!("✗ Hash verification failed for {} Wasm file (expected: {}, got: {})", used_file, args.hash, hash_str);

        // Try the other file variant as fallback
        let other_variant = if used_file == "unoptimized" { "optimized" } else { "unoptimized" };
        info!("Attempting fallback verification with {} Wasm file variant", other_variant);

        let other_bytes = if other_variant == "optimized" {
            let optimized_file = wasm_path.join(format!("{}.optimized.wasm", package_name));
            match std::fs::read(&optimized_file) {
                Ok(bytes) => {
                    info!("Successfully read {} Wasm file for fallback verification ({} bytes)", other_variant, bytes.len());
                    Some(bytes)
                }
                Err(e) => {
                    info!("{} Wasm file not available for fallback verification: {}", other_variant, e);
                    None
                }
            }
        } else {
            match std::fs::read(&wasm_file) {
                Ok(bytes) => {
                    info!("Successfully read {} Wasm file for fallback verification ({} bytes)", other_variant, bytes.len());
                    Some(bytes)
                }
                Err(e) => {
                    info!("{} Wasm file not available for fallback verification: {}", other_variant, e);
                    None
                }
            }
        };

        if let Some(other_bytes) = other_bytes {
            info!("Computing SHA256 hash of {} Wasm file for fallback verification", other_variant);
            let other_hash = Sha256::digest(&other_bytes);
            let other_hash_str = hex::encode(other_hash);
            info!("Computed SHA256 hash of {} Wasm file: {}", other_variant, other_hash_str);

            if other_hash_str == args.hash {
                info!("✓ Fallback hash verification successful using {} Wasm file", other_variant);
                return Ok(());
            } else {
                info!("✗ Fallback hash verification also failed for {} Wasm file (expected: {}, got: {})", other_variant, args.hash, other_hash_str);
            }
        }

        error!("Hash verification failed for both Wasm file variants - expected: {}, unoptimized file hash: {}, optimized file hash: {}",
               args.hash, hash_str, other_bytes.map_or("N/A".to_string(), |b| hex::encode(Sha256::digest(&b))));
        process::exit(1);
    }

    Ok(())
}
