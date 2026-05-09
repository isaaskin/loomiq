use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::*;
use loomiq_core::{PackageManifest, ProjectManifest, RegistryIndex};
use loomiq_engine::cache::clear_cache;
use loomiq_engine::executor::{compile_package, execute_package, ExecuteOptions};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

const REGISTRY_URL: &str = "https://raw.githubusercontent.com/isaaskin/loomiq-registry/main";

#[derive(Parser)]
#[command(name = "loomiq")]
#[command(about = "Loomiq: npm-style prompt package manager and execution engine (Rust)", long_about = None)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Install prompt packages from registry or loomiq.yaml
    Install {
        /// Package name or package@version (optional, installs from loomiq.yaml if omitted)
        package: Option<String>,
    },
    /// Run a prompt package or pipeline
    Run {
        package: String,
        #[arg(short, long, value_parser = parse_key_val::<String, String>)]
        input: Vec<(String, String)>,
        #[arg(short, long, default_value = "mock")]
        provider: String,
    },
    /// Compile a prompt package into a single prompt for manual execution
    Compile {
        package: String,
        #[arg(short, long, value_parser = parse_key_val::<String, String>)]
        input: Vec<(String, String)>,
    },
    /// List installed prompt packages
    List,
    /// Clear Loomiq cache
    CacheClear,
}

/// Parse a single key-value pair
fn parse_key_val<T, U>(
    s: &str,
) -> Result<(T, U), Box<dyn std::error::Error + Send + Sync + 'static>>
where
    T: std::str::FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
    U: std::str::FromStr,
    U::Err: std::error::Error + Send + Sync + 'static,
{
    let pos = s
        .find('=')
        .ok_or_else(|| format!("invalid KEY=value: no `=` found in `{s}`"))?;
    Ok((s[..pos].parse()?, s[pos + 1..].parse()?))
}

async fn install_package_recursive(pkg_req: &str, installed: &mut HashSet<String>) -> Result<()> {
    let (pkg_name, requested_version) = if pkg_req.contains('@') {
        let parts: Vec<&str> = pkg_req.split('@').collect();
        (parts[0], Some(parts[1]))
    } else {
        (pkg_req, None)
    };

    if installed.contains(pkg_name) {
        return Ok(());
    }

    let modules_dir = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".loomiq/modules");
    if !modules_dir.exists() {
        std::fs::create_dir_all(&modules_dir)?;
    }

    // Try local registry first
    let local_index_path = std::env::current_dir()?.join("registry/index.json");
    let mut manifest = None;
    let mut local_src_path = None;

    if local_index_path.exists() {
        let index_content = std::fs::read_to_string(&local_index_path)?;
        let index: RegistryIndex = serde_json::from_str(&index_content)?;

        if let Some(pkg_info) = index.packages.get(pkg_name) {
            let version = requested_version.unwrap_or(&pkg_info.latest);
            let pkg_path = std::env::current_dir()?
                .join(format!("registry/packages/{}/{}", pkg_name, version));
            if pkg_path.exists() {
                let m = PackageManifest::parse_file(pkg_path.join("package.yaml"))?;
                manifest = Some(m);
                local_src_path = Some(pkg_path);
            }
        }
    }

    let (manifest, is_remote) = if let Some(m) = manifest {
        (m, false)
    } else {
        // Try remote registry
        println!(
            "{} Fetching index for '{}' from remote...",
            "📡".yellow(),
            pkg_name
        );
        let index_url = format!("{}/index.json", REGISTRY_URL);
        let index: RegistryIndex = reqwest::get(index_url)
            .await?
            .json()
            .await
            .with_context(|| "Failed to fetch or parse remote registry index.json")?;

        let pkg_info = index.packages.get(pkg_name).ok_or_else(|| {
            anyhow::anyhow!(
                "Package '{}' not found in local or remote registry",
                pkg_name
            )
        })?;

        let version = requested_version.unwrap_or(&pkg_info.latest);
        println!(
            "{} Downloading '{}@{}' from remote...",
            "⬇️".blue(),
            pkg_name,
            version
        );

        let pkg_url = format!(
            "{}/packages/{}/{}/package.yaml",
            REGISTRY_URL, pkg_name, version
        );
        let yaml_content = reqwest::get(pkg_url).await?.text().await.with_context(|| {
            format!(
                "Failed to download package.yaml for {}@{}",
                pkg_name, version
            )
        })?;

        let manifest: PackageManifest = serde_yaml::from_str(&yaml_content).with_context(|| {
            format!(
                "Failed to parse remote package.yaml for {}@{}",
                pkg_name, version
            )
        })?;

        (manifest, true)
    };

    let dest_name = format!("{}@{}", pkg_name, manifest.version);
    let dest = modules_dir.join(&dest_name);

    if !is_remote {
        let src = local_src_path.unwrap();
        // Use symlink for local packages to avoid duplication
        if dest.exists() {
            if dest.is_symlink() {
                std::fs::remove_file(&dest)?;
            } else {
                std::fs::remove_dir_all(&dest)?;
            }
        }

        #[cfg(unix)]
        std::os::unix::fs::symlink(&src, &dest)?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&src, &dest)?;

        println!(
            "{} Symlinked local package '{}' to ./loomiq_modules/{}",
            "🔗".magenta(),
            pkg_name,
            dest_name
        );
    } else {
        // Copy from remote (save manifest)
        if dest.exists() {
            std::fs::remove_dir_all(&dest)?;
        }
        std::fs::create_dir_all(&dest)?;
        std::fs::write(dest.join("package.yaml"), serde_yaml::to_string(&manifest)?)?;
        println!(
            "{} Installed remote package '{}' to ./loomiq_modules/{}",
            "✅".green(),
            pkg_name,
            dest_name
        );
    }

    installed.insert(pkg_name.to_string());

    if let Some(steps) = manifest.steps {
        for step in steps {
            if !installed.contains(&step.uses) {
                Box::pin(install_package_recursive(&step.uses, installed)).await?;
            }
        }
    }

    Ok(())
}

fn find_installed_package(pkg_name: &str) -> Result<PathBuf> {
    let modules_dir = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".loomiq/modules");
    if !modules_dir.exists() {
        anyhow::bail!("loomiq_modules not found. Did you run install?");
    }

    // Try exact match with version first if pkg_name contains @
    if pkg_name.contains('@') {
        let path = modules_dir.join(pkg_name);
        if path.exists() {
            return Ok(path);
        }
    }

    // Try latest installed version
    let mut found = None;
    let mut latest_version: Option<String> = None;

    for entry in std::fs::read_dir(&modules_dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(&format!("{}@", pkg_name)) || name == pkg_name {
            let version = if name.contains('@') {
                name.split('@').nth(1).unwrap_or("0.0.0").to_string()
            } else {
                "0.0.0".to_string() // fallback
            };

            if latest_version.is_none() || version > latest_version.clone().unwrap_or_default() {
                latest_version = Some(version);
                found = Some(entry.path());
            }
        }
    }

    found.ok_or_else(|| {
        anyhow::anyhow!(
            "Package '{}' not found. Run 'loomiq install {}' first.",
            pkg_name,
            pkg_name
        )
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Install { package } => {
            let mut installed = HashSet::new();
            if let Some(pkg) = package {
                install_package_recursive(&pkg, &mut installed).await?;
            } else {
                // Install from loomiq.yaml
                let project_manifest_path = std::env::current_dir()?.join("loomiq.yaml");
                if !project_manifest_path.exists() {
                    anyhow::bail!("No package specified and loomiq.yaml not found.");
                }
                let project = ProjectManifest::parse_file(&project_manifest_path)
                    .with_context(|| "Failed to parse loomiq.yaml")?;

                println!(
                    "{} Installing dependencies for {}@{}...",
                    "📦".blue(),
                    project.name,
                    project.version
                );
                for (name, version) in project.dependencies {
                    let req = format!("{}@{}", name, version);
                    install_package_recursive(&req, &mut installed).await?;
                }
            }
        }
        Commands::Run {
            package,
            input,
            provider,
        } => {
            let package_path = find_installed_package(&package)?;
            let inputs: HashMap<String, String> = input.into_iter().collect();

            let manifest = PackageManifest::parse_file(package_path.join("package.yaml"))?;
            if let Some(required_inputs) = manifest.inputs {
                for (key, _) in required_inputs {
                    if !inputs.contains_key(&key) {
                        anyhow::bail!("Missing required input: --input {}=", key);
                    }
                }
            }

            println!(
                "{} Executing '{}' using provider '{}'...",
                "🚀".cyan(),
                package,
                provider
            );
            let options = ExecuteOptions {
                package_path,
                inputs,
                provider_name: Some(provider),
                global_modules_path: None,
            };

            match execute_package(options).await {
                Ok(result) => {
                    println!("\n{} Execution completed successfully!", "🎉".green());
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                Err(e) => {
                    anyhow::bail!("\n{} Execution failed: {}", "❌".red(), e);
                }
            }
        }
        Commands::Compile { package, input } => {
            let package_path = find_installed_package(&package)?;
            let inputs: HashMap<String, String> = input.into_iter().collect();

            let manifest = PackageManifest::parse_file(package_path.join("package.yaml"))?;
            if let Some(required_inputs) = manifest.inputs {
                for (key, _) in required_inputs {
                    if !inputs.contains_key(&key) {
                        anyhow::bail!("Missing required input: --input {}=", key);
                    }
                }
            }

            let options = ExecuteOptions {
                package_path,
                inputs,
                provider_name: None,
                global_modules_path: None,
            };

            match compile_package(options).await {
                Ok(result) => {
                    println!("\n📄 Compiled Prompt for '{}':\n", package);
                    println!("{}", result);
                }
                Err(e) => {
                    anyhow::bail!("\n{} Compilation failed: {}", "❌".red(), e);
                }
            }
        }
        Commands::List => {
            let modules_dir = dirs::home_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join(".loomiq/modules");
            if !modules_dir.exists() {
                println!("No packages installed.");
                return Ok(());
            }

            let entries: Vec<_> = std::fs::read_dir(&modules_dir)?
                .filter_map(|e| e.ok())
                .collect();
            if entries.is_empty() {
                println!("No packages installed.");
                return Ok(());
            }

            println!("Installed packages:");
            for entry in entries {
                let name = entry.file_name().to_string_lossy().to_string();
                let is_symlink = entry.path().is_symlink();
                let symlink_mark = if is_symlink {
                    "(local link)".magenta()
                } else {
                    "".normal()
                };
                println!(" - {} {}", name, symlink_mark);
            }
        }
        Commands::CacheClear => {
            clear_cache();
            println!("{} Cache cleared.", "✅".green());
        }
    }

    Ok(())
}
