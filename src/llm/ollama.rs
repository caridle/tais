// Ollama Provider — implements LlmProvider for local Ollama API

use super::*;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

pub struct OllamaProvider {
    base_url: String,
    model: String,
    params: LlmParams,
    client: Client,
}

impl OllamaProvider {
    pub fn new(base_url: String, model: String, params: LlmParams) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            model,
            params,
            client: Client::new(),
        }
    }

    fn chat_url(&self) -> String {
        format!("{}/api/chat", self.base_url)
    }

    fn tags_url(&self) -> String {
        format!("{}/api/tags", self.base_url)
    }

    /// Convert internal messages to Ollama format
    fn to_ollama_messages(
        messages: &[ChatMessage],
        system_prompt: Option<&str>,
    ) -> Vec<Value> {
        let mut ollama_msgs = Vec::new();

        if let Some(sys) = system_prompt {
            ollama_msgs.push(serde_json::json!({
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
            ollama_msgs.push(serde_json::json!({
                "role": role,
                "content": msg.content,
            }));
        }

        ollama_msgs
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    async fn chat(
        &self,
        messages: &[ChatMessage],
        system_prompt: Option<&str>,
    ) -> Result<ChatResponse, LlmError> {
        let ollama_msgs = Self::to_ollama_messages(messages, system_prompt);

        let body = serde_json::json!({
            "model": self.model,
            "messages": ollama_msgs,
            "stream": false,
            "options": {
                "temperature": self.params.temperature,
                "top_p": self.params.top_p,
                "num_predict": self.params.max_tokens,
            },
        });

        let resp = self.client
            .post(self.chat_url())
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

        let content = json["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        // Ollama reports token counts in eval_count/prompt_eval_count
        let usage = if json.get("eval_count").is_some() {
            Some(UsageInfo {
                prompt_tokens: json["prompt_eval_count"].as_u64().unwrap_or(0) as u32,
                completion_tokens: json["eval_count"].as_u64().unwrap_or(0) as u32,
                total_tokens: 0,
            })
        } else {
            None
        };

        Ok(ChatResponse {
            content,
            model: self.model.clone(),
            usage,
            finish_reason: Some("stop".into()),
        })
    }

    async fn health_check(&self) -> Result<bool, LlmError> {
        // Ollama: GET /api/tags returns available models
        let resp = self.client
            .get(self.tags_url())
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| LlmError::HttpError(e.to_string()))?;

        if !resp.status().is_success() {
            return Ok(false);
        }

        // Check if our model is in the list
        let json: Value = resp.json().await
            .map_err(|e| LlmError::ParseError(e.to_string()))?;

        let models = json["models"].as_array();
        let found = models.map_or(false, |ms| {
            ms.iter().any(|m| m["name"].as_str() == Some(&self.model))
        });

        Ok(found)
    }

    fn provider_type(&self) -> ProviderType {
        ProviderType::Ollama
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}
