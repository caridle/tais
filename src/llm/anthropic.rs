// Anthropic Provider — implements LlmProvider for Anthropic Claude API

use super::*;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

pub struct AnthropicProvider {
    api_key: String,
    base_url: String,
    model: String,
    params: LlmParams,
    client: Client,
    /// Anthropic API version header
    api_version: String,
}

impl AnthropicProvider {
    pub fn new(api_key: String, base_url: String, model: String, params: LlmParams) -> Self {
        Self {
            api_key,
            base_url: base_url.trim_end_matches('/').to_string(),
            model,
            params,
            client: Client::new(),
            api_version: "2023-06-01".into(),
        }
    }

    fn messages_url(&self) -> String {
        format!("{}/messages", self.base_url)
    }

    /// Convert internal messages to Anthropic format
    fn to_anthropic_messages(
        messages: &[ChatMessage],
        system_prompt: Option<&str>,
    ) -> (Option<String>, Vec<Value>) {
        let system = system_prompt.map(|s| s.to_string());

        let anthropic_msgs: Vec<Value> = messages
            .iter()
            .filter(|m| m.role != MessageRole::System)
            .map(|m| {
                let role = match m.role {
                    MessageRole::User => "user",
                    _ => "assistant",
                };
                serde_json::json!({
                    "role": role,
                    "content": m.content,
                })
            })
            .collect();

        (system, anthropic_msgs)
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    async fn chat(
        &self,
        messages: &[ChatMessage],
        system_prompt: Option<&str>,
    ) -> Result<ChatResponse, LlmError> {
        let (system, anthropic_msgs) = Self::to_anthropic_messages(messages, system_prompt);

        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": self.params.max_tokens,
            "messages": anthropic_msgs,
        });

        if let Some(ref sys) = system {
            body["system"] = serde_json::Value::String(sys.clone());
        }
        if self.params.temperature > 0.0 {
            body["temperature"] = serde_json::json!(self.params.temperature);
        }
        if self.params.top_p < 1.0 {
            body["top_p"] = serde_json::json!(self.params.top_p);
        }

        let resp = self.client
            .post(self.messages_url())
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", &self.api_version)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::HttpError(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            let error_text = resp.text().await.unwrap_or_default();
            return Err(LlmError::ApiError {
                code: status.as_u16(),
                message: error_text,
            });
        }

        let json: Value = resp.json().await
            .map_err(|e| LlmError::ParseError(e.to_string()))?;

        // Anthropic returns content as an array of blocks
        let content = json["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let finish_reason = json["stop_reason"]
            .as_str()
            .map(|s| s.to_string());

        let usage = json.get("usage").map(|u| UsageInfo {
            prompt_tokens: u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            completion_tokens: u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            total_tokens: 0, // Anthropic doesn't give total directly
        });

        Ok(ChatResponse {
            content,
            model: self.model.clone(),
            usage,
            finish_reason,
        })
    }

    async fn health_check(&self) -> Result<bool, LlmError> {
        // Anthropic doesn't have a simple health check endpoint.
        // Send a minimal message to verify API key works.
        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": 1,
            "messages": [{"role": "user", "content": "hi"}],
        });

        let resp = self.client
            .post(self.messages_url())
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", &self.api_version)
            .header("Content-Type", "application/json")
            .json(&body)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| LlmError::HttpError(e.to_string()))?;

        Ok(resp.status().is_success())
    }

    fn provider_type(&self) -> ProviderType {
        ProviderType::Anthropic
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}
