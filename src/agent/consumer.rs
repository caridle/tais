// Agent Consumer — 自主教学任务消费者
//
// 接收提案 → 调度到 SkillsBus → 异步执行 → 返回结果
// 全真：tokio::spawn 绿色线程，真实 SkillsBus::execute

use crate::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

// ── Types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub proposal_id: String,
    pub skill_name: String,
    pub output: serde_json::Value,
    pub duration_ms: u64,
    pub success: bool,
    pub error: Option<String>,
    pub gene_applied: GeneProfile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub proposal_id: String,
    pub concept: String,
    pub student_query: String,
    pub agent_response: String,
    pub strategy: String,
    pub confidence: f64,
    pub mastery_delta: f64,
    pub elapsed_ms: u64,
    pub timestamp: String,
}

// ── Consumer ──────────────────────────────────────────────────────────

pub struct Consumer {
    skills_bus: Arc<skills::SkillsBus>,
    /// Accumulated session records for analysis
    pub history: Arc<RwLock<Vec<SessionRecord>>>,
}

impl Consumer {
    pub fn new(skills_bus: Arc<skills::SkillsBus>) -> Self {
        Self {
            skills_bus,
            history: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Execute a teaching proposal via SkillsBus
    pub async fn consume(
        &self,
        proposal: &super::proposer::TaskProposal,
        student_query: &str,
    ) -> Result<TaskResult, super::proposer::AgentError> {
        let start = std::time::Instant::now();

        let input = serde_json::json!({
            "student_query": student_query,
            "concept": proposal.concept,
            "strategy": proposal.strategy,
            "context": format!("优先级: {:?}, 理由: {}", proposal.priority, proposal.rationale),
        });

        // Real execution via SkillsBus
        let result = self
            .skills_bus
            .execute(&proposal.skill, input, &proposal.gene_profile)
            .await;

        let elapsed = start.elapsed().as_millis() as u64;

        match result {
            Ok(output) => {
                let record = SessionRecord {
                    proposal_id: proposal.id.clone(),
                    concept: proposal.concept.clone(),
                    student_query: student_query.into(),
                    agent_response: output["content"].as_str().unwrap_or("").into(),
                    strategy: output["strategy"].as_str().unwrap_or(&proposal.strategy).into(),
                    confidence: output["confidence"].as_f64().unwrap_or(0.7),
                    mastery_delta: 0.0, // computed later by rater
                    elapsed_ms: elapsed,
                    timestamp: chrono::Utc::now().to_rfc3339(),
                };
                self.history.write().await.push(record);

                Ok(TaskResult {
                    proposal_id: proposal.id.clone(),
                    skill_name: proposal.skill.clone(),
                    output,
                    duration_ms: elapsed,
                    success: true,
                    error: None,
                    gene_applied: proposal.gene_profile.clone(),
                })
            }
            Err(e) => Ok(TaskResult {
                proposal_id: proposal.id.clone(),
                skill_name: proposal.skill.clone(),
                output: serde_json::json!({"error": e.to_string()}),
                duration_ms: elapsed,
                success: false,
                error: Some(e.to_string()),
                gene_applied: proposal.gene_profile.clone(),
            }),
        }
    }

    /// Get recent session history for analysis
    pub async fn get_history(&self, limit: usize) -> Vec<SessionRecord> {
        let h = self.history.read().await;
        h.iter().rev().take(limit).cloned().collect()
    }

    /// Clear session history (for fresh start)
    pub async fn clear_history(&self) {
        self.history.write().await.clear();
    }
}
