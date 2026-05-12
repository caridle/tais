// OpenAI Provider — implements LlmProvider for OpenAI-compatible APIs
//
// Works with: OpenAI, DeepSeek, Groq, vLLM, and any /v1/chat/completions endpoint

use super::*;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

pub struct OpenAiProvider {
    api_key: String,
    base_url: String,
    model: String,
    params: LlmParams,
    client: Client,
}

impl OpenAiProvider {
    pub fn new(api_key: String, base_url: String, model: String, params: LlmParams) -> Self {
        Self {
            api_key,
            base_url: base_url.trim_end_matches('/').to_string(),
            model,
            params,
            client: Client::new(),
        }
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }

    fn models_url(&self) -> String {
        format!("{}/models", self.base_url)
    }

    /// Convert internal messages to OpenAI format
    fn to_openai_messages(
        messages: &[ChatMessage],
        system_prompt: Option<&str>,
    ) -> Vec<Value> {
        let mut openai_msgs = Vec::new();

        if let Some(sys) = system_prompt {
            openai_msgs.push(serde_json::json!({
                "role": "system",
                "content": sys,
            }));
        }

        for msg in messages {
            let role = match msg.role {
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::System => "system",
            };
            openai_msgs.push(serde_json::json!({
                "role": role,
                "content": msg.content,
            }));
        }

        openai_msgs
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn chat(
        &self,
        messages: &[ChatMessage],
        system_prompt: Option<&str>,
    ) -> Result<ChatResponse, LlmError> {
        let openai_msgs = Self::to_openai_messages(messages, system_prompt);

        let body = serde_json::json!({
            "model": self.model,
            "messages": openai_msgs,
            "temperature": self.params.temperature,
            "max_tokens": self.params.max_tokens,
            "top_p": self.params.top_p,
        });

        let resp = self.client
            .post(self.chat_url())
            .header("Authorization", format!("Bearer {}", self.api_key))
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

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let finish_reason = json["choices"][0]["finish_reason"]
            .as_str()
            .map(|s| s.to_string());

        let usage = json.get("usage").map(|u| UsageInfo {
            prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0) as u32,
            completion_tokens: u["completion_tokens"].as_u64().unwrap_or(0) as u32,
            total_tokens: u["total_tokens"].as_u64().unwrap_or(0) as u32,
        });

        Ok(ChatResponse {
            content,
            model: self.model.clone(),
            usage,
            finish_reason,
        })
    }

    async fn health_check(&self) -> Result<bool, LlmError> {
        let resp = self.client
            .get(self.models_url())
            .header("Authorization", format!("Bearer {}", self.api_key))
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| LlmError::HttpError(e.to_string()))?;

        Ok(resp.status().is_success())
    }

    fn provider_type(&self) -> ProviderType {
        ProviderType::OpenAI
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}
