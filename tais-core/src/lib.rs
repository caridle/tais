// TAIS Core Engine — Teacher-AI-Student Self-Evolving Teaching System
//
// Module architecture:
//   orchestrator — DAG workflow generation and execution
//   mcp          — MCP Gateway (JSON-RPC protocol, tool registry)
//   evolution    — TextGrad-style prompt optimization engine
//   skills       — Skills Bus (T01-T07 dispatch)
//   gene         — Gene Gateway (G01-G07 injection at decision points)
//   llm          — LLM abstraction (OpenAI / Anthropic / Ollama)
//   wechat       — WeChat Official Account bot integration
//   memory       — Conversation history storage and context retrieval
//   dashboard    — System dashboard HTML page
//   data         — Database models and repositories
//   api          — HTTP/WebSocket API (axum)

pub mod orchestrator;
pub mod mcp;
pub mod evolution;
pub mod skills;
pub mod gene;
pub mod llm;
pub mod wechat;
pub mod memory;
pub mod dashboard;
pub mod data;
pub mod api;
pub mod config;
pub mod agent;  // 自主教学 Agent 闭环
pub mod auth;    // JWT authentication, QR login, WeChat binding
pub mod chat;    // WebSocket chat UI
pub mod habit;   // Habit Engine — 7 habit capsules (H01-H07)

use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Core Types ───────────────────────────────────────────────────────────

/// Unique identifier for sessions, students, workflows
pub type Id = Uuid;

/// Teacher's teaching goal input
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeachingGoal {
    pub subject: String,
    pub concept: String,
    pub mode: TeachingMode,
    pub target_level: StudentLevel,
    pub constraints: Vec<String>,
    pub teacher_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeachingMode {
    InquiryBased,
    DirectInstruction,
    ProjectBased,
    FlippedClassroom,
    SocraticDialogue,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StudentLevel {
    Beginner,
    Intermediate,
    Advanced,
}

/// A node in the teaching workflow DAG
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNode {
    pub id: String,
    pub name: String,
    pub agent: Vec<String>,     // which TAIS skills to invoke
    pub gene: Vec<String>,      // which gene capsules to inject
    pub mcp_tools: Vec<String>, // external MCP tools needed
    pub input: serde_json::Value,
    pub hitl_trigger: Option<HitlTrigger>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HitlTrigger {
    pub condition: HitlCondition,
    pub action: HitlAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HitlCondition {
    ConfidenceBelow(f64),
    NoProgressAfter(u32),
    CommonErrorAbove(f64),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HitlAction {
    EscalateToTeacher,
    PauseAndNotify,
    FlagForReview,
}

/// The complete DAG workflow
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub id: Id,
    pub goal: TeachingGoal,
    pub nodes: Vec<WorkflowNode>,
    pub edges: Vec<(String, String)>, // (from_node_id, to_node_id)
    pub entry: String,
    pub gene_profile: GeneProfile,
}

/// Gene profile applied across the entire workflow
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneProfile {
    pub personality: String,  // "scholar" | "mentor" | "hacker"
    pub thinking: String,     // "first_principles" | "analogical" | "systems"
    pub risk_level: String,   // "strict" | "moderate" | "permissive"
    pub behavior: String,     // "precise" | "concise" | "verbose"
}

impl Default for GeneProfile {
    fn default() -> Self {
        Self {
            personality: "scholar".into(),
            thinking: "first_principles".into(),
            risk_level: "strict".into(),
            behavior: "precise".into(),
        }
    }
}

/// A teaching session's complete record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub session_id: Id,
    pub student_id: String,
    pub workflow_id: Id,
    pub pre_score: f64,
    pub post_score: f64,
    pub dialogue_rounds: u32,
    pub breakthrough_count: u32,
    pub hitl_escalations: u32,
    pub resources_pushed: u32,
    pub resources_clicked: u32,
    pub stuck_points: Vec<String>,
    pub teacher_rating: Option<f64>,
    pub created_at: chrono::NaiveDateTime,
}

/// Evolution metrics computed from session data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvolutionMetrics {
    pub learning_effectiveness: f64,  // normalized gain
    pub teaching_efficiency: f64,      // breakthroughs per round
    pub student_autonomy: f64,         // 1 - escalation rate
    pub resource_engagement: f64,      // click-through rate
    pub teacher_satisfaction: f64,     // manual rating
    pub composite: f64,                // weighted sum
}

/// An MCP tool definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value, // JSON Schema
}

/// MCP JSON-RPC types
/// Re-exports from skills module
pub use skills::{
    SkillDefinition, SkillCategory, SkillStatus,
    InstallRequest, ExecuteRequest,
    TaisSkill, SkillError, SkillsBus,
};

/// Re-exports from habit module
pub use habit::HabitEngine;
pub use habit::state::{
    HabitRule, HabitState, HabitCondition, HabitAction, TriggerType, HabitLog,
    THETA_STABLE, THETA_RETRAIN,
};

/// MCP JSON-RPC types
pub mod rpc {
    use super::*;

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Request {
        pub jsonrpc: String,
        pub id: Option<u32>,
        pub method: String,
        pub params: Option<serde_json::Value>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Response {
        pub jsonrpc: String,
        pub id: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub result: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub error: Option<RpcError>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct RpcError {
        pub code: i32,
        pub message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub data: Option<serde_json::Value>,
    }

    impl Request {
        pub fn new(method: &str, params: Option<serde_json::Value>) -> Self {
            Self {
                jsonrpc: "2.0".into(),
                id: Some(1),
                method: method.into(),
                params,
            }
        }
    }

    impl Response {
        pub fn success(id: Option<u32>, result: serde_json::Value) -> Self {
            Self {
                jsonrpc: "2.0".into(),
                id,
                result: Some(result),
                error: None,
            }
        }

        pub fn error(id: Option<u32>, code: i32, message: &str) -> Self {
            Self {
                jsonrpc: "2.0".into(),
                id,
                result: None,
                error: Some(RpcError {
                    code,
                    message: message.into(),
                    data: None,
                }),
            }
        }
    }
}
