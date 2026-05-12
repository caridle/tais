// Agent Proposer — 自主教学提案器
//
// 分析学生状态 → 生成教学任务提案 → 优先级排序
// LLM 驱动，回退规则兜底

use crate::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ── Types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StudentState {
    pub student_id: String,
    pub concept: String,
    pub mastery_level: f64,
    pub weak_points: Vec<String>,
    pub learning_style: String,
    pub session_count: u32,
    pub last_activity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskProposal {
    pub id: String,
    pub concept: String,
    pub skill: String,         // which skill to invoke
    pub strategy: String,      // teaching strategy
    pub prompt: String,        // what to ask/task
    pub priority: Priority,
    pub rationale: String,
    pub expected_difficulty: f64,
    pub gene_profile: GeneProfile,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Critical,  // mastery < 0.3, must address now
    High,      // mastery < 0.5
    Medium,    // mastery < 0.7
    Low,       // mastery >= 0.7, enrichment
}

impl Priority {
    pub fn from_mastery(m: f64) -> Self {
        if m < 0.3 { Priority::Critical }
        else if m < 0.5 { Priority::High }
        else if m < 0.7 { Priority::Medium }
        else { Priority::Low }
    }
}

// ── Proposer ──────────────────────────────────────────────────────────

pub struct Proposer {
    llm: Arc<llm::LlmRouter>,
}

impl Proposer {
    pub fn new(llm: Arc<llm::LlmRouter>) -> Self {
        Self { llm }
    }

    /// Analyze student state and propose the next teaching task
    pub async fn propose(
        &self,
        state: &StudentState,
        history: &str,
    ) -> Result<TaskProposal, AgentError> {
        let priority = Priority::from_mastery(state.mastery_level);
        let weak_str = state.weak_points.join("、");

        let system = r#"你是教学任务提案专家。根据学生状态分析，提出最合适的下一个教学任务。
输出JSON：
{
  "concept": "要教的概念",
  "skill": "tais-socratic-tutor|tais-skill-coach|tais-resource-pusher|tais-feedback-collector",
  "strategy": "clarification|counterexample|analogy|scaffold|drill|feedback",
  "prompt": "具体的教学提问或任务描述",
  "rationale": "选择这个任务的逻辑依据",
  "expected_difficulty": 0.5
}"#;

        let user = format!(
            "学生状态：\n- 概念：{}\n- 掌握度：{:.0}%\n- 薄弱点：{}\n- 学习风格：{}\n- 历史对话：{}\n\n请提案下一个教学任务。",
            state.concept,
            state.mastery_level * 100.0,
            weak_str,
            state.learning_style,
            history
        );

        let response = self.llm.chat_simple(system, &user).await;

        match response {
            Ok(json_str) => {
                if let Ok(mut parsed) = serde_json::from_str::<serde_json::Value>(&json_str) {
                    Ok(TaskProposal {
                        id: format!("prop-{}", uuid::Uuid::new_v4().to_string()[..8].to_string()),
                        concept: parsed["concept"].as_str().unwrap_or(&state.concept).into(),
                        skill: parsed["skill"].as_str().unwrap_or("tais-socratic-tutor").into(),
                        strategy: parsed["strategy"].as_str().unwrap_or("clarification").into(),
                        prompt: parsed["prompt"].as_str().unwrap_or("请解释这个概念").into(),
                        priority,
                        rationale: parsed["rationale"].as_str().unwrap_or("基于掌握度分析").into(),
                        expected_difficulty: parsed["expected_difficulty"].as_f64().unwrap_or(0.5),
                        gene_profile: GeneProfile::default(),
                    })
                } else {
                    Ok(self.fallback_proposal(state))
                }
            }
            Err(_) => Ok(self.fallback_proposal(state)),
        }
    }

    /// Fallback: rule-based proposal when LLM is unavailable
    fn fallback_proposal(&self, state: &StudentState) -> TaskProposal {
        let (skill, strategy, prompt) = match Priority::from_mastery(state.mastery_level) {
            Priority::Critical => (
                "tais-socratic-tutor", "scaffold",
                format!("让我们从基础开始：关于「{}」，你能说出它的定义吗？", state.concept),
            ),
            Priority::High => (
                "tais-socratic-tutor", "clarification",
                format!("你提到「{}」——能用自己的话再解释一遍吗？", state.concept),
            ),
            Priority::Medium => (
                "tais-skill-coach", "drill",
                format!("来做一道关于「{}」的练习题吧", state.concept),
            ),
            Priority::Low => (
                "tais-resource-pusher", "feedback",
                format!("你已掌握「{}」的基础，来看看进阶资源", state.concept),
            ),
        };

        TaskProposal {
            id: format!("prop-{}", uuid::Uuid::new_v4().to_string()[..8].to_string()),
            concept: state.concept.clone(),
            skill: skill.into(),
            strategy: strategy.into(),
            prompt,
            priority: Priority::from_mastery(state.mastery_level),
            rationale: format!("掌握度 {:.0}%，选择 {} 策略", state.mastery_level * 100.0, strategy),
            expected_difficulty: 1.0 - state.mastery_level,
            gene_profile: GeneProfile::default(),
        }
    }
}

// ── Errors ────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("LLM error: {0}")]
    Llm(#[from] llm::LlmError),
    #[error("Skill error: {0}")]
    Skill(#[from] crate::SkillError),
    #[error("Proposal error: {0}")]
    Proposal(String),
    #[error("Execution error: {0}")]
    Execution(String),
    #[error("Rating error: {0}")]
    Rating(String),
}
