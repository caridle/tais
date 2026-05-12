// LLM Abstraction Layer — unified interface for OpenAI, Anthropic, Ollama
//
// Architecture:
//   LlmProvider trait  →  OpenAI / Anthropic / Ollama implementations
//   LlmRouter          →  config_id → Provider, fallback chain, CRUD management
//   LlmConfig           →  persisted config (base_url, api_key, model, params)

pub mod openai;
pub mod anthropic;
pub mod ollama;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

// ── Core Types ──────────────────────────────────────────────────────────

/// LLM provider type
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderType {
    OpenAI,
    Anthropic,
    Ollama,
}

impl std::fmt::Display for ProviderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OpenAI => write!(f, "openai"),
            Self::Anthropic => write!(f, "anthropic"),
            Self::Ollama => write!(f, "ollama"),
        }
    }
}

/// Chat message role
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
}

/// A single chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
}

/// LLM parameters (shared across providers, with provider-specific mapping)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmParams {
    #[serde(default = "default_temperature")]
    pub temperature: f64,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_top_p")]
    pub top_p: f64,
    /// Extra provider-specific params (e.g., stop_sequences for Anthropic)
    #[serde(default)]
    pub extra: HashMap<String, serde_json::Value>,
}

fn default_temperature() -> f64 { 0.7 }
fn default_max_tokens() -> u32 { 4096 }
fn default_top_p() -> f64 { 0.95 }

impl Default for LlmParams {
    fn default() -> Self {
        Self {
            temperature: 0.7,
            max_tokens: 4096,
            top_p: 0.95,
            extra: HashMap::new(),
        }
    }
}

/// LLM chat response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub content: String,
    pub model: String,
    pub usage: Option<UsageInfo>,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageInfo {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// LLM error
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("HTTP error: {0}")]
    HttpError(String),
    #[error("API error ({code}): {message}")]
    ApiError { code: u16, message: String },
    #[error("Parse error: {0}")]
    ParseError(String),
    #[error("Provider not found: {0}")]
    ProviderNotFound(String),
    #[error("Config error: {0}")]
    ConfigError(String),
}

// ── LlmProvider Trait ───────────────────────────────────────────────────

/// Unified LLM provider interface
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Send a chat completion request
    async fn chat(
        &self,
        messages: &[ChatMessage],
        system_prompt: Option<&str>,
    ) -> Result<ChatResponse, LlmError>;

    /// Health check — test connectivity
    async fn health_check(&self) -> Result<bool, LlmError>;

    /// Provider type
    fn provider_type(&self) -> ProviderType;

    /// Current model name
    fn model_name(&self) -> &str;
}

// ── LLM Configuration ───────────────────────────────────────────────────

/// Persisted LLM configuration (CRUD via API)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub id: String,
    pub name: String,
    pub provider: ProviderType,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub params: LlmParams,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default = "default_true")]
    pub is_active: bool,
}

fn default_true() -> bool { true }

/// Request body for creating/updating LLM config
#[derive(Debug, Deserialize)]
pub struct LlmConfigRequest {
    pub name: String,
    pub provider: ProviderType,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    #[serde(default)]
    pub params: LlmParams,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default = "default_true")]
    pub is_active: bool,
}

/// Connection test result
#[derive(Debug, Serialize)]
pub struct TestResult {
    pub success: bool,
    pub model: String,
    pub latency_ms: u64,
    pub message: String,
}

// ── LlmRouter ───────────────────────────────────────────────────────────

/// Routes requests to the correct provider based on config.
/// Manages provider lifecycle and fallback chain.
pub struct LlmRouter {
    /// All LLM configs (CRUD state)
    configs: Arc<RwLock<HashMap<String, LlmConfig>>>,
    /// Active provider instances (lazy-initialized)
    providers: Arc<RwLock<HashMap<String, Box<dyn LlmProvider>>>>,
    /// Fallback chain: [primary_id, secondary_id, ...]
    fallback_chain: Arc<RwLock<Vec<String>>>,
}

impl LlmRouter {
    pub fn new() -> Self {
        Self {
            configs: Arc::new(RwLock::new(HashMap::new())),
            providers: Arc::new(RwLock::new(HashMap::new())),
            fallback_chain: Arc::new(RwLock::new(Vec::new())),
        }
    }

    // ── CRUD ────────────────────────────────────────────────────────

    /// List all configs (api_key masked)
    pub async fn list_configs(&self) -> Vec<LlmConfig> {
        let configs = self.configs.read().await;
        configs.values().map(|c| {
            let mut masked = c.clone();
            masked.api_key = mask_key(&c.api_key);
            masked
        }).collect()
    }

    /// Get a single config
    pub async fn get_config(&self, id: &str) -> Option<LlmConfig> {
        self.configs.read().await.get(id).cloned()
    }

    /// Create a new config and initialize the provider
    pub async fn create_config(&self, req: LlmConfigRequest) -> Result<LlmConfig, LlmError> {
        let id = format!("{}-{}", req.provider, uuid::Uuid::new_v4().to_string()[..8].to_string());

        // If this is the first default or explicitly set as default
        let is_default = if req.is_default {
            // Unset other defaults
            let mut configs = self.configs.write().await;
            for (_, c) in configs.iter_mut() {
                c.is_default = false;
            }
            true
        } else {
            let configs = self.configs.read().await;
            configs.is_empty() // first config is default
        };

        let config = LlmConfig {
            id: id.clone(),
            name: req.name,
            provider: req.provider,
            base_url: req.base_url,
            api_key: req.api_key.clone(),
            model: req.model,
            params: req.params,
            is_default,
            is_active: req.is_active,
        };

        // Build provider
        let provider = build_provider(&config)?;
        self.providers.write().await.insert(id.clone(), provider);

        // Store config
        self.configs.write().await.insert(id.clone(), config.clone());

        // Rebuild fallback chain
        self.rebuild_fallback_chain().await;

        Ok(config)
    }

    /// Update an existing config
    pub async fn update_config(&self, id: &str, req: LlmConfigRequest) -> Result<LlmConfig, LlmError> {
        // First, unset defaults on all configs if requested
        if req.is_default {
            let mut configs = self.configs.write().await;
            for (_, c) in configs.iter_mut() {
                c.is_default = false;
            }
        }

        // Then update the specific config
        let mut configs = self.configs.write().await;
        let config = configs.get_mut(id)
            .ok_or_else(|| LlmError::ConfigError(format!("config not found: {id}")))?;

        config.name = req.name;
        config.provider = req.provider;
        config.base_url = req.base_url;
        config.api_key = req.api_key.clone();
        config.model = req.model;
        config.params = req.params;
        config.is_active = req.is_active;
        config.is_default = req.is_default;

        // Rebuild provider
        let provider = build_provider(config)?;
        self.providers.write().await.insert(id.to_string(), provider);

        // Rebuild fallback chain
        self.rebuild_fallback_chain().await;

        Ok(config.clone())
    }

    /// Delete a config
    pub async fn delete_config(&self, id: &str) -> Result<(), LlmError> {
        let mut configs = self.configs.write().await;
        if configs.remove(id).is_none() {
            return Err(LlmError::ConfigError(format!("config not found: {id}")));
        }
        self.providers.write().await.remove(id);
        self.rebuild_fallback_chain().await;
        Ok(())
    }

    /// Test connection for a config
    pub async fn test_config(&self, id: &str) -> Result<TestResult, LlmError> {
        let providers = self.providers.read().await;
        let provider = providers.get(id)
            .ok_or_else(|| LlmError::ProviderNotFound(id.into()))?;

        let start = std::time::Instant::now();
        match provider.health_check().await {
            Ok(true) => Ok(TestResult {
                success: true,
                model: provider.model_name().into(),
                latency_ms: start.elapsed().as_millis() as u64,
                message: format!("{} 连接成功", provider.model_name()),
            }),
            Ok(false) => Ok(TestResult {
                success: false,
                model: provider.model_name().into(),
                latency_ms: start.elapsed().as_millis() as u64,
                message: "连接失败：健康检查返回 false".into(),
            }),
            Err(e) => Ok(TestResult {
                success: false,
                model: provider.model_name().into(),
                latency_ms: start.elapsed().as_millis() as u64,
                message: format!("连接失败: {e}"),
            }),
        }
    }

    // ── Routing ─────────────────────────────────────────────────────

    /// Send a chat request through the default provider (with fallback)
    pub async fn chat(
        &self,
        messages: &[ChatMessage],
        system_prompt: Option<&str>,
        config_id: Option<&str>,
    ) -> Result<ChatResponse, LlmError> {
        // If specific config requested, use that
        if let Some(id) = config_id {
            let providers = self.providers.read().await;
            if let Some(provider) = providers.get(id) {
                return provider.chat(messages, system_prompt).await;
            }
            return Err(LlmError::ProviderNotFound(id.into()));
        }

        // Use fallback chain
        let chain = self.fallback_chain.read().await;
        let providers = self.providers.read().await;

        let mut last_error = None;
        for id in chain.iter() {
            if let Some(provider) = providers.get(id) {
                match provider.chat(messages, system_prompt).await {
                    Ok(resp) => return Ok(resp),
                    Err(e) => {
                        tracing::warn!("LLM fallback: {id} failed ({e}), trying next");
                        last_error = Some(e);
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| LlmError::ProviderNotFound("no providers available".into())))
    }

    /// Convenience: one-shot chat with just system + user prompts, returns content string
    pub async fn chat_simple(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, LlmError> {
        let messages = vec![
            ChatMessage::user(user_prompt),
        ];
        let response = self.chat(&messages, Some(system_prompt), None).await?;
        Ok(response.content)
    }

    /// Get system status for dashboard
    pub async fn status(&self) -> LlmStatus {
        let configs = self.configs.read().await;
        let providers = self.providers.read().await;

        let mut model_statuses = Vec::new();
        for config in configs.values() {
            let provider = providers.get(&config.id);
            model_statuses.push(ModelStatus {
                id: config.id.clone(),
                name: config.name.clone(),
                provider_type: config.provider.clone(),
                model: config.model.clone(),
                is_default: config.is_default,
                is_active: config.is_active,
                is_connected: provider.is_some(),
            });
        }

        LlmStatus {
            total_configs: configs.len() as u32,
            active_configs: configs.values().filter(|c| c.is_active).count() as u32,
            connected_count: providers.len() as u32,
            models: model_statuses,
        }
    }

    /// Rebuild fallback chain: default first, then active, then rest
    async fn rebuild_fallback_chain(&self) {
        let configs = self.configs.read().await;
        let mut ids: Vec<_> = configs.values()
            .filter(|c| c.is_active)
            .collect();
        // Sort: default first
        ids.sort_by_key(|c| !c.is_default);

        let chain: Vec<_> = ids.into_iter().map(|c| c.id.clone()).collect();
        *self.fallback_chain.write().await = chain;
    }
}

impl Default for LlmRouter {
    fn default() -> Self {
        Self::new()
    }
}

/// Dashboard status response
#[derive(Debug, Serialize)]
pub struct LlmStatus {
    pub total_configs: u32,
    pub active_configs: u32,
    pub connected_count: u32,
    pub models: Vec<ModelStatus>,
}

#[derive(Debug, Serialize)]
pub struct ModelStatus {
    pub id: String,
    pub name: String,
    pub provider_type: ProviderType,
    pub model: String,
    pub is_default: bool,
    pub is_active: bool,
    pub is_connected: bool,
}

// ── Internal Helpers ────────────────────────────────────────────────────

/// Build a provider from config
fn build_provider(config: &LlmConfig) -> Result<Box<dyn LlmProvider>, LlmError> {
    match config.provider {
        ProviderType::OpenAI => Ok(Box::new(openai::OpenAiProvider::new(
            config.api_key.clone(),
            config.base_url.clone(),
            config.model.clone(),
            config.params.clone(),
        ))),
        ProviderType::Anthropic => Ok(Box::new(anthropic::AnthropicProvider::new(
            config.api_key.clone(),
            config.base_url.clone(),
            config.model.clone(),
            config.params.clone(),
        ))),
        ProviderType::Ollama => Ok(Box::new(ollama::OllamaProvider::new(
            config.base_url.clone(),
            config.model.clone(),
            config.params.clone(),
        ))),
    }
}

/// Mask API key for safe display
fn mask_key(key: &str) -> String {
    if key.len() <= 8 {
        return "****".into();
    }
    format!("{}...{}", &key[..4], &key[key.len()-4..])
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: MessageRole::User, content: content.into() }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: MessageRole::Assistant, content: content.into() }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self { role: MessageRole::System, content: content.into() }
    }
}
