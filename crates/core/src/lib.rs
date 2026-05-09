use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CoreError {
    #[error("Failed to read manifest at {0}: {1}")]
    ReadError(String, std::io::Error),
    #[error("Failed to parse YAML in {0}: {1}")]
    ParseError(String, serde_yaml::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStep {
    pub id: String,
    pub uses: String,
    pub with: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PackageType {
    Prompt,
    Pipeline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageManifest {
    pub name: String,
    pub version: String,
    #[serde(rename = "type")]
    pub package_type: PackageType,

    // Prompt specific
    pub inputs: Option<HashMap<String, String>>,
    pub config: Option<HashMap<String, serde_json::Value>>,
    pub output_format: Option<String>,
    pub schema: Option<serde_json::Value>,
    pub prompt: Option<String>,

    // Pipeline specific
    pub steps: Option<Vec<PipelineStep>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectManifest {
    pub name: String,
    pub version: String,
    pub dependencies: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryPackageInfo {
    pub latest: String,
    pub versions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryIndex {
    pub packages: HashMap<String, RegistryPackageInfo>,
}

impl PackageManifest {
    pub fn parse_file<P: AsRef<Path>>(path: P) -> Result<Self, CoreError> {
        let path_str = path.as_ref().to_string_lossy().to_string();
        let content = std::fs::read_to_string(path.as_ref())
            .map_err(|e| CoreError::ReadError(path_str.clone(), e))?;
        serde_yaml::from_str(&content).map_err(|e| CoreError::ParseError(path_str, e))
    }
}

impl ProjectManifest {
    pub fn parse_file<P: AsRef<Path>>(path: P) -> Result<Self, CoreError> {
        let path_str = path.as_ref().to_string_lossy().to_string();
        let content = std::fs::read_to_string(path.as_ref())
            .map_err(|e| CoreError::ReadError(path_str.clone(), e))?;
        serde_yaml::from_str(&content).map_err(|e| CoreError::ParseError(path_str, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_prompt_manifest() {
        let yaml = r#"
name: test-prompt
version: 1.0.0
type: prompt
inputs:
  topic: string
prompt: |
  Write about {{topic}}
        "#;
        let manifest: PackageManifest = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(manifest.name, "test-prompt");
        assert_eq!(manifest.package_type, PackageType::Prompt);
        assert!(manifest.inputs.unwrap().contains_key("topic"));
    }
}
