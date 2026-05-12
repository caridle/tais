// Agent Deployer — 进化策略部署器
//
// 当教学策略表现优秀时 → 自动部署到进化引擎
// 低质量策略 → 标记为待淘汰，触发优化建议

use crate::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ── Types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentDecision {
    pub rating_id: String,
    pub concept: String,
    pub strategy: String,
    pub overall_score: f64,
    pub action: DeployAction,
    pub reason: String,
    pub gene_update: Option<GeneProfile>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DeployAction {
    /// Strategy is excellent → deploy permanently
    Deploy,
    /// Good enough to keep, but not exceptional
    Retain,
    /// Underperforming → mark for review
    FlagForReview,
    /// Failing → remove and trigger retraining
    Retire,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentHistory {
    pub decisions: Vec<DeploymentDecision>,
    pub deployed_count: u32,
    pub retained_count: u32,
    pub flagged_count: u32,
    pub retired_count: u32,
}

// ── Deployer ──────────────────────────────────────────────────────────

pub struct Deployer {
    evolution: Arc<evolution::EvolutionEngine>,
    history: Arc<tokio::sync::RwLock<Vec<DeploymentDecision>>>,
    // Thresholds
    pub deploy_threshold: f64,     // score >= this → deploy
    pub retain_threshold: f64,     // score >= this → retain
    pub review_threshold: f64,     // score >= this → flag (else retire)
}

impl Deployer {
    pub fn new(evolution: Arc<evolution::EvolutionEngine>) -> Self {
        Self {
            evolution,
            history: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            deploy_threshold: 0.8,
            retain_threshold: 0.6,
            review_threshold: 0.35,
        }
    }

    /// Decide what to do with a teaching strategy based on its rating
    pub async fn decide(
        &self,
        rating: &super::rater::QualityRating,
        gene: &GeneProfile,
    ) -> DeploymentDecision {
        let action = if rating.overall_score >= self.deploy_threshold {
            DeployAction::Deploy
        } else if rating.overall_score >= self.retain_threshold {
            DeployAction::Retain
        } else if rating.overall_score >= self.review_threshold {
            DeployAction::FlagForReview
        } else {
            DeployAction::Retire
        };

        let reason = match action {
            DeployAction::Deploy => format!(
                "综合评分 {:.0}% ≥ 部署阈值 {:.0}%，策略表现优秀，自动部署",
                rating.overall_score * 100.0,
                self.deploy_threshold * 100.0
            ),
            DeployAction::Retain => format!(
                "综合评分 {:.0}% 在保留区间 [{:.0}%, {:.0}%)，维持现状",
                rating.overall_score * 100.0,
                self.retain_threshold * 100.0,
                self.deploy_threshold * 100.0
            ),
            DeployAction::FlagForReview => format!(
                "综合评分 {:.0}% 偏低，标记审查",
                rating.overall_score * 100.0
            ),
            DeployAction::Retire => format!(
                "综合评分 {:.0}% 低于 {:.0}%，建议淘汰并触发重新训练",
                rating.overall_score * 100.0,
                self.review_threshold * 100.0
            ),
        };

        let gene_update = if action == DeployAction::Deploy {
            // Boost successful strategy: shift personality toward more effective mode
            let mut updated = gene.clone();
            if rating.overall_score > 0.85 {
                updated.personality = "mentor".into(); // high performers get mentor mode
            }
            Some(updated)
        } else if action == DeployAction::Retire {
            // Penalize failing: revert to strict scholar
            let mut updated = gene.clone();
            updated.personality = "scholar".into();
            updated.risk_level = "strict".into();
            Some(updated)
        } else {
            None
        };

        let decision = DeploymentDecision {
            rating_id: rating.proposal_id.clone(),
            concept: rating.concept.clone(),
            strategy: rating.improvement_suggestions
                .first()
                .map(|s| s.area.clone())
                .unwrap_or_else(|| "general".into()),
            overall_score: rating.overall_score,
            action,
            reason,
            gene_update,
            timestamp: chrono::Utc::now().to_rfc3339(),
        };

        self.history.write().await.push(decision.clone());
        decision
    }

    /// Execute the deployment: apply gene changes to evolution engine
    pub async fn execute_deployment(
        &self,
        decision: &DeploymentDecision,
    ) -> Result<(), super::proposer::AgentError> {
        if decision.action == DeployAction::Deploy {
            // Record the successful evolution via update_prompt
            self.evolution.update_prompt(
                &decision.strategy,
                &format!("deployed strategy v{}", chrono::Utc::now().format("%Y%m%d-%H%M%S")),
                decision.overall_score,
            ).await;

            tracing::info!(
                "🚀 Deployed: {} strategy (score: {:.0}%)",
                decision.strategy, decision.overall_score * 100.0
            );
        } else if decision.action == DeployAction::Retire {
            tracing::warn!(
                "🗑️ Strategy {} retired (score: {:.0}%)",
                decision.strategy, decision.overall_score * 100.0
            );
        }

        Ok(())
    }

    /// Get deployment history summary
    pub async fn get_history(&self) -> DeploymentHistory {
        let decisions = self.history.read().await;
        let mut deployed = 0u32;
        let mut retained = 0u32;
        let mut flagged = 0u32;
        let mut retired = 0u32;

        for d in decisions.iter() {
            match d.action {
                DeployAction::Deploy => deployed += 1,
                DeployAction::Retain => retained += 1,
                DeployAction::FlagForReview => flagged += 1,
                DeployAction::Retire => retired += 1,
            }
        }

        DeploymentHistory {
            decisions: decisions.clone(),
            deployed_count: deployed,
            retained_count: retained,
            flagged_count: flagged,
            retired_count: retired,
        }
    }
}
