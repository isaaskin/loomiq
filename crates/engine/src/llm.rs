use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LLMError {
    #[error("API Request failed: {0}")]
    RequestError(#[from] reqwest::Error),
    #[error("Missing API Key")]
    MissingApiKey,
}

#[async_trait]
pub trait Provider {
    async fn generate(
        &self,
        prompt: &str,
        config: Option<&HashMap<String, Value>>,
    ) -> Result<String, LLMError>;
}

pub struct MockProvider;

#[async_trait]
impl Provider for MockProvider {
    async fn generate(
        &self,
        prompt: &str,
        _config: Option<&HashMap<String, Value>>,
    ) -> Result<String, LLMError> {
        println!(
            "[MockProvider] Generating for prompt length {}",
            prompt.len()
        );
        let res = serde_json::json!({
            "mocked": true,
            "result": format!("This is a mocked response for {}...", &prompt[0..std::cmp::min(20, prompt.len())]),
            "timestamp": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()
        });
        Ok(res.to_string())
    }
}

pub struct OpenAIProvider {
    api_key: String,
    client: reqwest::Client,
}

impl OpenAIProvider {
    pub fn new() -> Result<Self, LLMError> {
        let api_key = std::env::var("OPENAI_API_KEY").map_err(|_| LLMError::MissingApiKey)?;
        Ok(Self {
            api_key,
            client: reqwest::Client::new(),
        })
    }
}

#[async_trait]
impl Provider for OpenAIProvider {
    async fn generate(
        &self,
        prompt: &str,
        config: Option<&HashMap<String, Value>>,
    ) -> Result<String, LLMError> {
        let model = config
            .and_then(|c| c.get("model"))
            .and_then(|m| m.as_str())
            .unwrap_or("gpt-3.5-turbo");

        let temperature = config
            .and_then(|c| c.get("temperature"))
            .and_then(|t| t.as_f64())
            .unwrap_or(0.7);

        let body = serde_json::json!({
            "model": model,
            "messages": [{"role": "user", "content": prompt}],
            "temperature": temperature,
            "response_format": { "type": "json_object" }
        });

        let resp = self
            .client
            .post("https://api.openai.com/v1/chat/completions")
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;

        let resp_json: Value = resp.json().await?;

        let content = resp_json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("{}")
            .to_string();

        Ok(content)
    }
}

pub fn get_provider(name: &str) -> Box<dyn Provider> {
    if name.eq_ignore_ascii_case("openai") {
        if let Ok(p) = OpenAIProvider::new() {
            return Box::new(p);
        } else {
            println!("Warning: OPENAI_API_KEY not found, falling back to MockProvider");
        }
    }
    Box::new(MockProvider)
}
