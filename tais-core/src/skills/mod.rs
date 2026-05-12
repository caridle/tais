// Skills Bus — full lifecycle: install → register → use → unregister → uninstall
//
// Lifecycle:
//   1. install()   — persist skill definition (YAML/JSON → memory)
//   2. register()  — activate on bus (ready for execute)
//   3. execute()   — run the skill
//   4. unregister()— deactivate from bus (keep definition)
//   5. uninstall() — remove definition entirely (must be unregistered first)
//
// Skill definitions survive across sessions via in-memory registry.
// Built-in skills are pre-installed at startup in main.rs.

use crate::*;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

// Concrete skill implementations
pub mod implementations;
/// On-demand skill loader (L1 index → L3 SOP files)
pub mod loader;
/// Skill crystallization — distill teaching sessions into reusable SOPs
pub mod crystallizer;

// ── Skill Definition (install-time metadata) ──────────────────────────

/// A skill blueprint — what gets installed before it can be registered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDefinition {
    pub name: String,
    pub display_name: String,
    pub version: String,
    pub description: String,
    pub category: SkillCategory,
    /// What agent/tool names this skill responds to
    pub binds: Vec<String>,
    /// JSON Schema for input validation
    pub input_schema: serde_json::Value,
    /// LLM system prompt for the skill
    pub system_prompt: String,
    pub installed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SkillCategory {
    Teaching,
    Analysis,
    Resource,
    Coaching,
    Feedback,
    Orchestration,
    Evolution,
    Custom,
}

// ── TaisSkill trait ───────────────────────────────────────────────────

/// A TAIS skill — the core teaching behavior unit
#[async_trait]
pub trait TaisSkill: Send + Sync {
    fn name(&self) -> &str;
    fn definition(&self) -> &SkillDefinition;

    async fn execute(
        &self,
        input: serde_json::Value,
        gene_profile: &GeneProfile,
    ) -> Result<serde_json::Value, SkillError>;

    fn should_escalate(&self, output: &serde_json::Value) -> Option<HitlTrigger>;
}

#[derive(Debug, thiserror::Error)]
pub enum SkillError {
    #[error("Skill execution failed: {0}")]
    ExecutionError(String),
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    #[error("Skill not installed: {0}")]
    NotInstalled(String),
    #[error("Skill not registered: {0}")]
    NotRegistered(String),
    #[error("Cannot uninstall while registered: {0}")]
    StillRegistered(String),
}

// ── Skills Bus ────────────────────────────────────────────────────────

pub struct SkillsBus {
    /// Active skills (registered, executable)
    active: Arc<RwLock<HashMap<String, Box<dyn TaisSkill>>>>,
    /// All installed skill definitions (including unregistered)
    definitions: Arc<RwLock<HashMap<String, SkillDefinition>>>,
}

impl SkillsBus {
    pub fn new() -> Self {
        Self {
            active: Arc::new(RwLock::new(HashMap::new())),
            definitions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    // ── Install / Uninstall (lifecycle management) ────────────────────

    /// Install a skill definition. Does NOT register.
    pub async fn install(&self, def: SkillDefinition) -> Result<(), SkillError> {
        let name = def.name.clone();
        let mut defs = self.definitions.write().await;
        if defs.contains_key(&name) {
            // Re-install: update definition (but don't touch registration)
            defs.insert(name, def);
        } else {
            defs.insert(name, def);
        }
        Ok(())
    }

    /// Uninstall a skill definition. Must be unregistered first.
    pub async fn uninstall(&self, name: &str) -> Result<(), SkillError> {
        let active = self.active.read().await;
        if active.contains_key(name) {
            return Err(SkillError::StillRegistered(name.into()));
        }
        drop(active);

        let mut defs = self.definitions.write().await;
        if defs.remove(name).is_none() {
            return Err(SkillError::NotInstalled(name.into()));
        }
        Ok(())
    }

    /// List all installed skill definitions
    pub async fn list_definitions(&self) -> Vec<SkillDefinition> {
        self.definitions.read().await.values().cloned().collect()
    }

    // ── Register / Unregister ─────────────────────────────────────────

    /// Register a skill on the bus. Must be installed first.
    pub async fn register(&self, skill: Box<dyn TaisSkill>) -> Result<(), SkillError> {
        let name = skill.name().to_string();
        // Ensure definition exists
        let defs = self.definitions.read().await;
        if !defs.contains_key(&name) {
            return Err(SkillError::NotInstalled(name));
        }
        drop(defs);

        let mut active = self.active.write().await;
        active.insert(name, skill);
        Ok(())
    }

    /// Unregister a skill from the bus. Keeps the definition.
    pub async fn unregister(&self, name: &str) -> Result<(), SkillError> {
        let mut active = self.active.write().await;
        if active.remove(name).is_none() {
            return Err(SkillError::NotRegistered(name.into()));
        }
        Ok(())
    }

    /// Quick install + register in one call (for convenience)
    pub async fn install_and_register(
        &self,
        def: SkillDefinition,
        skill: Box<dyn TaisSkill>,
    ) -> Result<(), SkillError> {
        self.install(def).await?;
        self.register(skill).await?;
        Ok(())
    }

    // ── Execute ───────────────────────────────────────────────────────

    /// Execute a skill by name
    pub async fn execute(
        &self,
        name: &str,
        input: serde_json::Value,
        gene_profile: &GeneProfile,
    ) -> Result<serde_json::Value, SkillError> {
        let active = self.active.read().await;
        let skill = active
            .get(name)
            .ok_or_else(|| SkillError::NotRegistered(name.into()))?;

        skill.execute(input, gene_profile).await
    }

    /// List all registered (active) skill names
    pub async fn list_skills(&self) -> Vec<String> {
        self.active.read().await.keys().cloned().collect()
    }

    /// Get full status: all definitions with registration status
    pub async fn status(&self) -> Vec<SkillStatus> {
        let defs = self.definitions.read().await;
        let active = self.active.read().await;

        defs.iter()
            .map(|(name, def)| SkillStatus {
                name: name.clone(),
                display_name: def.display_name.clone(),
                version: def.version.clone(),
                description: def.description.clone(),
                category: def.category.clone(),
                installed: true,
                registered: active.contains_key(name),
                binds: def.binds.clone(),
            })
            .collect()
    }
}

impl Default for SkillsBus {
    fn default() -> Self {
        Self::new()
    }
}

// ── Types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillStatus {
    pub name: String,
    pub display_name: String,
    pub version: String,
    pub description: String,
    pub category: SkillCategory,
    pub installed: bool,
    pub registered: bool,
    pub binds: Vec<String>,
}

/// Request body for installing a skill from API
#[derive(Debug, Deserialize)]
pub struct InstallRequest {
    pub name: String,
    pub display_name: String,
    pub version: Option<String>,
    pub description: String,
    pub category: Option<SkillCategory>,
    pub binds: Option<Vec<String>>,
    pub input_schema: Option<serde_json::Value>,
    pub system_prompt: String,
    /// If true, also register the skill after installing
    pub auto_register: Option<bool>,
}

/// Request body for executing a skill
#[derive(Debug, Deserialize)]
pub struct ExecuteRequest {
    pub input: serde_json::Value,
    pub gene_profile: Option<GeneProfile>,
}
