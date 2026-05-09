use crate::cache::{get_cache_key, get_cached_result, set_cached_result};
use crate::llm::{get_provider, Provider};
use crate::template::resolve_template;
use loomiq_core::{PackageManifest, PackageType};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum EngineError {
    #[error("Package manifest not found: {0}")]
    ManifestNotFound(String),
    #[error("Core error: {0}")]
    CoreError(#[from] loomiq_core::CoreError),
    #[error("LLM Provider error: {0}")]
    ProviderError(#[from] crate::llm::LLMError),
    #[error("Dependency package not found: {0}")]
    DependencyNotFound(String),
    #[error("Pipeline step '{0}' must use a 'prompt' package")]
    InvalidStepPackage(String),
    #[error("Missing required input: {0}")]
    MissingInput(String),
}

pub struct ExecuteOptions {
    pub package_path: PathBuf,
    pub inputs: HashMap<String, String>,
    pub provider_name: Option<String>,
    pub global_modules_path: Option<PathBuf>,
}

pub async fn execute_package(options: ExecuteOptions) -> Result<Value, EngineError> {
    let manifest_path = options.package_path.join("package.yaml");
    if !manifest_path.exists() {
        return Err(EngineError::ManifestNotFound(
            manifest_path.to_string_lossy().to_string(),
        ));
    }

    let manifest = PackageManifest::parse_file(&manifest_path)?;
    let provider = get_provider(options.provider_name.as_deref().unwrap_or("mock"));

    let global_modules = options.global_modules_path.unwrap_or_else(|| {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".loomiq/modules")
    });

    match manifest.package_type {
        PackageType::Prompt => {
            let mut context = Value::Object(serde_json::Map::new());
            for (k, v) in &options.inputs {
                context
                    .as_object_mut()
                    .unwrap()
                    .insert(k.clone(), Value::String(v.clone()));
            }
            run_single_prompt(&manifest, &context, provider.as_ref()).await
        }
        PackageType::Pipeline => {
            run_pipeline(
                &manifest,
                &options.inputs,
                provider.as_ref(),
                &global_modules,
            )
            .await
        }
    }
}

async fn run_single_prompt(
    pkg: &PackageManifest,
    context: &Value,
    provider: &dyn Provider,
) -> Result<Value, EngineError> {
    let resolved_prompt = resolve_template(pkg.prompt.as_deref().unwrap_or(""), context);

    let mut final_prompt = resolved_prompt;
    if let Some(ref fmt) = pkg.output_format {
        if fmt == "json" {
            let schema_str = pkg
                .schema
                .as_ref()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "{}".to_string());
            final_prompt = format!("{}\n\nYou must return your answer in valid JSON format. Follow this schema if provided: {}", final_prompt, schema_str);
        }
    }

    let cache_key = get_cache_key(&final_prompt, pkg.config.as_ref());
    let result_str = if let Some(cached) = get_cached_result(&cache_key) {
        println!("  ✓ Cache hit! ({})", cache_key);
        cached
    } else {
        println!("  ✓ Cache miss, calling LLM...");
        let generated = provider
            .generate(&final_prompt, pkg.config.as_ref())
            .await?;
        set_cached_result(&cache_key, &generated);
        generated
    };

    let parsed: Value = if let Some(captures) =
        regex::Regex::new(r"```(?:json)?\s*([\s\S]*?)\s*```")
            .unwrap()
            .captures(&result_str)
    {
        serde_json::from_str(&captures[1])
            .unwrap_or_else(|_| serde_json::json!({ "raw": result_str }))
    } else {
        serde_json::from_str(&result_str)
            .unwrap_or_else(|_| serde_json::json!({ "raw": result_str }))
    };

    Ok(parsed)
}

async fn run_pipeline(
    manifest: &PackageManifest,
    inputs: &HashMap<String, String>,
    provider: &dyn Provider,
    global_modules: &Path,
) -> Result<Value, EngineError> {
    let mut context_map = serde_json::Map::new();
    for (k, v) in inputs {
        context_map.insert(k.clone(), Value::String(v.clone()));
    }
    let mut context = Value::Object(context_map);

    if let Some(steps) = &manifest.steps {
        for step in steps {
            println!("\n▶ Executing step: {} (uses: {})", step.id, step.uses);

            let mut dep_path = global_modules.join(&step.uses);

            // Search for versioned folder if strict match fails
            if !dep_path.exists() {
                if let Ok(entries) = std::fs::read_dir(global_modules) {
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if name.starts_with(&format!("{}@", step.uses)) || name == step.uses {
                            dep_path = entry.path();
                            break;
                        }
                    }
                }
            }

            if !dep_path.exists() {
                return Err(EngineError::DependencyNotFound(
                    dep_path.to_string_lossy().to_string(),
                ));
            }

            let pkg = PackageManifest::parse_file(dep_path.join("package.yaml"))?;
            if pkg.package_type != PackageType::Prompt {
                return Err(EngineError::InvalidStepPackage(step.id.clone()));
            }

            let mut step_context_map = serde_json::Map::new();
            if let Some(with) = &step.with {
                for (k, v) in with {
                    let resolved = resolve_template(v, &context);
                    step_context_map.insert(k.clone(), Value::String(resolved));
                }
            }
            let step_context = Value::Object(step_context_map);

            let result = run_single_prompt(&pkg, &step_context, provider).await?;

            let mut step_output = serde_json::Map::new();
            step_output.insert("output".to_string(), result);
            if let Some(obj) = context.as_object_mut() {
                obj.insert(step.id.clone(), Value::Object(step_output));
            }
        }
    }

    Ok(context)
}

pub async fn compile_package(options: ExecuteOptions) -> Result<String, EngineError> {
    let manifest_path = options.package_path.join("package.yaml");
    if !manifest_path.exists() {
        return Err(EngineError::ManifestNotFound(
            manifest_path.to_string_lossy().to_string(),
        ));
    }

    let manifest = PackageManifest::parse_file(&manifest_path)?;
    let global_modules = options.global_modules_path.unwrap_or_else(|| {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".loomiq/modules")
    });

    if let Some(required_inputs) = &manifest.inputs {
        for input_name in required_inputs.keys() {
            if !options.inputs.contains_key(input_name) {
                return Err(EngineError::MissingInput(input_name.clone()));
            }
        }
    }

    let mut context_map = serde_json::Map::new();
    for (k, v) in &options.inputs {
        context_map.insert(k.clone(), Value::String(v.clone()));
    }
    let context = Value::Object(context_map);

    if manifest.package_type == PackageType::Prompt {
        let resolved = resolve_template(manifest.prompt.as_deref().unwrap_or(""), &context);
        return Ok(resolved.trim().to_string());
    }

    let mut compiled_prompt =
        String::from("I need you to perform a sequential multi-step task.\n\n");

    if let Some(steps) = &manifest.steps {
        for (i, step) in steps.iter().enumerate() {
            let mut dep_path = global_modules.join(&step.uses);
            if !dep_path.exists() {
                if let Ok(entries) = std::fs::read_dir(&global_modules) {
                    for entry in entries.flatten() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if name.starts_with(&format!("{}@", step.uses)) || name == step.uses {
                            dep_path = entry.path();
                            break;
                        }
                    }
                }
            }

            if !dep_path.exists() {
                return Err(EngineError::DependencyNotFound(
                    dep_path.to_string_lossy().to_string(),
                ));
            }

            let pkg = PackageManifest::parse_file(dep_path.join("package.yaml"))?;

            let mut step_context_map = serde_json::Map::new();
            if let Some(with) = &step.with {
                for (k, v) in with {
                    let resolved = resolve_template(v, &context);
                    step_context_map.insert(k.clone(), Value::String(resolved));
                }
            }
            let step_context = Value::Object(step_context_map);

            let resolved_prompt =
                resolve_template(pkg.prompt.as_deref().unwrap_or(""), &step_context);

            compiled_prompt.push_str(&format!("### Step {}: {}\n", i + 1, step.id));
            compiled_prompt.push_str(resolved_prompt.trim());
            compiled_prompt.push('\n');
            if pkg.output_format.as_deref() == Some("json") {
                let schema_str = pkg
                    .schema
                    .as_ref()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "{}".to_string());
                compiled_prompt.push_str(&format!(
                    "\n(Return the result for this step in JSON matching this schema: {})\n",
                    schema_str
                ));
            }
            compiled_prompt.push_str("\n---\n\n");
        }
    }

    Ok(compiled_prompt.trim().to_string())
}
