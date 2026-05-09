use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

pub fn get_cache_dir() -> PathBuf {
    let mut path = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push(".loomiq");
    path.push("cache");
    path
}

pub fn get_cache_key(prompt: &str, config: Option<&HashMap<String, serde_json::Value>>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prompt);
    if let Some(cfg) = config {
        if let Ok(json) = serde_json::to_string(cfg) {
            hasher.update(json);
        }
    }
    hex::encode(hasher.finalize())
}

pub fn get_cached_result(key: &str) -> Option<String> {
    let path = get_cache_dir().join(format!("{}.json", key));
    fs::read_to_string(path).ok()
}

pub fn set_cached_result(key: &str, result: &str) {
    let dir = get_cache_dir();
    if !dir.exists() {
        let _ = fs::create_dir_all(&dir);
    }
    let path = dir.join(format!("{}.json", key));
    let _ = fs::write(path, result);
}

pub fn clear_cache() {
    let dir = get_cache_dir();
    if dir.exists() {
        let _ = fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_get_cache_key_consistency() {
        let key1 = get_cache_key("hello world", None);
        let key2 = get_cache_key("hello world", None);
        assert_eq!(key1, key2);

        let key3 = get_cache_key("different", None);
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_get_cache_key_with_config() {
        let key1 = get_cache_key("hello", None);

        let mut config = HashMap::new();
        config.insert("model".to_string(), json!("gpt-4"));
        let key2 = get_cache_key("hello", Some(&config));

        assert_ne!(key1, key2);
    }
}
