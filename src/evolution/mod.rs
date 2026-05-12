// Evolution Engine — real metrics computation from session data

pub mod evaluator;
pub mod optimizer;
pub mod collector;

use crate::*;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct EvolutionEngine {
    threshold: f64,
    min_sessions: u32,
    history: RwLock<Vec<EvolutionRecord>>,
    allow_auto_deploy: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EvolutionRecord {
    pub agent: String,
    pub action: String,
    pub old_prompt: String,
    pub new_prompt: String,
    pub composite_before: f64,
    pub composite_after: f64,
    pub timestamp: chrono::NaiveDateTime,
}

impl EvolutionEngine {
    pub fn new(threshold: f64, min_sessions: u32) -> Self {
        Self {
            threshold,
            min_sessions,
            history: RwLock::new(Vec::new()),
            allow_auto_deploy: false,
        }
    }

    pub async fn get_history(&self) -> Vec<EvolutionRecord> {
        self.history.read().await.clone()
    }

    /// Compute real EvolutionMetrics from session data
    pub fn compute_metrics(sessions: &[SessionRecord]) -> EvolutionMetrics {
        if sessions.is_empty() {
            return EvolutionMetrics {
                learning_effectiveness: 0.0,
                teaching_efficiency: 0.0,
                student_autonomy: 0.0,
                resource_engagement: 0.0,
                teacher_satisfaction: 0.0,
                composite: 0.0,
            };
        }

        let n = sessions.len() as f64;

        // Learning effectiveness: average normalized gain (post - pre) / (1 - pre)
        let le: f64 = sessions.iter()
            .map(|s| if s.pre_score < 1.0 { (s.post_score - s.pre_score) / (1.0 - s.pre_score) } else { 0.0 })
            .sum::<f64>() / n;

        // Teaching efficiency: breakthroughs per dialogue round
        let te: f64 = sessions.iter()
            .map(|s| if s.dialogue_rounds > 0 { s.breakthrough_count as f64 / s.dialogue_rounds as f64 } else { 0.0 })
            .sum::<f64>() / n;

        // Student autonomy: 1 - escalation rate
        let sa: f64 = sessions.iter()
            .map(|s| if s.dialogue_rounds > 0 { 1.0 - (s.hitl_escalations as f64 / s.dialogue_rounds as f64) } else { 1.0 })
            .sum::<f64>() / n;

        // Resource engagement: click-through rate
        let re: f64 = sessions.iter()
            .map(|s| if s.resources_pushed > 0 { s.resources_clicked as f64 / s.resources_pushed as f64 } else { 0.0 })
            .sum::<f64>() / n;

        // Teacher satisfaction: manual rating average
        let ts_count = sessions.iter().filter(|s| s.teacher_rating.is_some()).count() as f64;
        let ts: f64 = if ts_count > 0.0 {
            sessions.iter().filter_map(|s| s.teacher_rating).sum::<f64>() / ts_count
        } else {
            0.0
        };

        let composite = 0.3 * le + 0.2 * te + 0.2 * sa + 0.15 * re + 0.15 * ts;

        EvolutionMetrics {
            learning_effectiveness: le.clamp(0.0, 1.0),
            teaching_efficiency: te.clamp(0.0, 1.0),
            student_autonomy: sa.clamp(0.0, 1.0),
            resource_engagement: re.clamp(0.0, 1.0),
            teacher_satisfaction: ts.clamp(0.0, 1.0),
            composite: composite.clamp(0.0, 1.0),
        }
    }

    pub async fn update_prompt(&self, agent: &str, new_prompt: &str, composite: f64) {
        let mut history = self.history.write().await;
        let old_prompt = history.iter()
            .rev()
            .find(|r| r.agent == agent)
            .map(|r| r.new_prompt.clone())
            .unwrap_or_else(|| "initial".into());
        let action = if composite >= 0.8 { "approved" } else if composite >= 0.5 { "modified" } else { "rejected" };
        history.push(EvolutionRecord {
            agent: agent.into(),
            action: action.into(),
            old_prompt,
            new_prompt: new_prompt.into(),
            composite_before: composite,
            composite_after: composite,
            timestamp: chrono::Utc::now().naive_utc(),
        });
    }
}
