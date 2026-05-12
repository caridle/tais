// Agent Loop — 自主教学闭环 (Dual-Speed)
//
// Inspired by GenericAgent's minimal loop + TAIS's quality pipeline.
//
// Fast path (1 LLM call):  confidence > 0.85 && errors < 2
//   LLM directly selects skill → Consumer executes → habit check → next turn
//
// Quality path (4 LLM calls): confidence ≤ 0.85 or errors ≥ 2
//   Proposer → Consumer → Rater → Deployer → habit check → next turn
//
// Working checkpoint: auto-injected each turn, prevents re-analyzing full history.

use crate::*;
use crate::memory::checkpoint::WorkingCheckpoint;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

// ── Loop State ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LoopStatus {
    Idle,
    Running,
    Paused,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopState {
    pub status: LoopStatus,
    pub iteration: u64,
    pub total_rounds: u64,
    pub successful_rounds: u64,
    pub failed_rounds: u64,
    pub deployed_strategies: u32,
    pub current_mastery: f64,
    pub last_error: Option<String>,
    /// How many consecutive errors of the same type
    pub consecutive_same_errors: u32,
    /// Current path: "fast" or "quality"
    pub active_path: String,
    /// Fast path rounds executed in current session
    pub fast_path_rounds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopStats {
    pub state: LoopState,
    pub recent_ratings: Vec<super::rater::QualityRating>,
    pub deployment_history: super::deployer::DeploymentHistory,
}

// ── AgentLoop ─────────────────────────────────────────────────────────

pub struct AgentLoop {
    pub proposer: Arc<super::proposer::Proposer>,
    pub consumer: Arc<super::consumer::Consumer>,
    pub rater: Arc<super::rater::Rater>,
    pub deployer: Arc<super::deployer::Deployer>,
    pub habit_engine: Arc<crate::habit::HabitEngine>,

    state: Arc<RwLock<LoopState>>,
    /// Short-term working checkpoint (GenericAgent-style)
    checkpoint: RwLock<Option<WorkingCheckpoint>>,
    /// Skill loader for on-demand SOP injection
    skill_loader: RwLock<skills::loader::SkillLoader>,
}

impl AgentLoop {
    pub fn new(
        proposer: Arc<super::proposer::Proposer>,
        consumer: Arc<super::consumer::Consumer>,
        rater: Arc<super::rater::Rater>,
        deployer: Arc<super::deployer::Deployer>,
        habit_engine: Arc<crate::habit::HabitEngine>,
    ) -> Self {
        // Initialize skill loader from memory/skills directory
        let skills_dir = std::path::PathBuf::from("memory/skills");
        let skill_loader = skills::loader::SkillLoader::new(skills_dir);

        Self {
            proposer,
            consumer,
            rater,
            deployer,
            habit_engine,
            state: Arc::new(RwLock::new(LoopState {
                status: LoopStatus::Idle,
                iteration: 0,
                total_rounds: 0,
                successful_rounds: 0,
                failed_rounds: 0,
                deployed_strategies: 0,
                current_mastery: 0.0,
                last_error: None,
                consecutive_same_errors: 0,
                active_path: "quality".into(),
                fast_path_rounds: 0,
            })),
            checkpoint: RwLock::new(None),
            skill_loader: RwLock::new(skill_loader),
        }
    }

    // ── Path Decision ─────────────────────────────────────────────────

    /// Determine whether to use the fast path (1 LLM call) or quality path (4 calls).
    async fn should_use_fast_path(&self, student_state: &super::proposer::StudentState) -> bool {
        let state = self.state.read().await;
        let mastery = student_state.mastery_level;
        let errors = state.consecutive_same_errors;

        // Fast path conditions:
        // 1. Student mastery is decent (> 0.4)
        // 2. No recent error pattern (< 3 consecutive same errors)
        // 3. At least some successful rounds to establish confidence
        let eligible = mastery > 0.4 && errors < 3 && state.successful_rounds >= 2;

        if eligible {
            tracing::debug!("🚀 Using FAST path (mastery={:.2}, errors={})", mastery, errors);
            true
        } else {
            tracing::debug!("🐢 Using QUALITY path (mastery={:.2}, errors={})", mastery, errors);
            false
        }
    }

    // ── Fast Path (1 LLM call) ────────────────────────────────────────

    /// Fast path: LLM directly selects a skill and the consumer executes it.
    /// Skips the Proposer/Rater/Deployer pipeline for speed.
    async fn run_fast(
        &self,
        student_state: &super::proposer::StudentState,
        student_query: &str,
        _history: &str,
    ) -> Result<super::rater::QualityRating, super::proposer::AgentError> {
        // Build a lightweight prompt with working checkpoint + relevant SOPs
        let checkpoint_injection = self.get_checkpoint_injection().await;
        let sop_injection = {
            let loader = self.skill_loader.read().await;
            loader.resolve(Some(&student_state.concept), 1)
        };

        // Simple direct prompt: ask LLM to pick a skill and respond
        let fast_prompt = format!(
            "{} {} 学生问: {} 概念: {} 掌握度: {:.0}% 请选择合适的教学技能并直接回复（追问引导学生的思考，不直接给答案）。",
            checkpoint_injection, sop_injection,
            student_query, student_state.concept,
            student_state.mastery_level * 100.0,
        );

        // Build a minimal proposal — use keyword matching for skill selection
        let proposal = self.proposer.propose(student_state, &fast_prompt).await?;

        tracing::info!(
            "⚡ Fast path: {} (skill: {}, strategy: {})",
            proposal.prompt, proposal.skill, proposal.strategy
        );

        // Consume
        let result = self.consumer.consume(&proposal, student_query).await?;

        // Update state
        {
            let mut s = self.state.write().await;
            s.total_rounds += 1;
            s.fast_path_rounds += 1;
            s.active_path = "fast".into();
            if result.success {
                s.successful_rounds += 1;
                s.consecutive_same_errors = 0;
            } else {
                s.failed_rounds += 1;
                s.consecutive_same_errors += 1;
            }
        }

        let status_icon = if result.success { "⚡✅" } else { "⚡❌" };
        tracing::info!("{} Fast consumed: {} ({}ms)", status_icon, proposal.skill, result.duration_ms);

        // Lightweight habit check (skip full rating for speed)
        let _ = self.habit_engine.trigger("H03", serde_json::json!({
            "dialogue_ended": true,
            "skill": &proposal.skill,
            "success": result.success,
        })).await;

        // If error count rising, switch back to quality path
        if self.state.read().await.consecutive_same_errors >= 3 {
            let _ = self.habit_engine.trigger("H02", serde_json::json!({
                "consecutive_errors": 3,
                "skill": &proposal.skill,
                "concept": &proposal.concept,
                "switching_to": "quality_path",
            })).await;
            tracing::warn!("Fast path → quality path (consecutive errors >= 3)");
        }

        // Build a synthetic rating for the return type (fast path skips full rating)
        let fast_rating = super::rater::QualityRating {
            proposal_id: String::new(),
            concept: proposal.concept.clone(),
            comprehension_score: if result.success { 0.85 } else { 0.4 },
            engagement_score: 0.8,
            relevance_score: 0.9,
            strategy_fit: 0.8,
            overall_score: if result.success { 0.9 } else { 0.5 },
            confidence: 0.7,
            summary: format!("Fast path: {}", proposal.skill),
            strengths: vec![],
            weaknesses: vec![],
            improvement_suggestions: vec![],
            mastery_before: student_state.mastery_level,
            mastery_after: if result.success { student_state.mastery_level + 0.05 } else { student_state.mastery_level },
            mastery_delta: if result.success { 0.05 } else { 0.0 },
            timestamp: chrono::Utc::now().to_rfc3339(),
        };

        // Advance checkpoint
        self.advance_checkpoint().await;

        self.state.write().await.status = LoopStatus::Idle;
        Ok(fast_rating)
    }

    // ── Quality Path (4 LLM calls) ────────────────────────────────────

    /// Run one full iteration: Propose → Consume → Rate → Deploy
    pub async fn run_one(
        &self,
        student_state: &super::proposer::StudentState,
        student_query: &str,
        history: &str,
    ) -> Result<super::rater::QualityRating, super::proposer::AgentError> {
        // Update state
        {
            let mut s = self.state.write().await;
            s.iteration += 1;
            s.status = LoopStatus::Running;
            s.current_mastery = student_state.mastery_level;
        }

        // Decide path
        if self.should_use_fast_path(student_state).await {
            return self.run_fast(student_state, student_query, history).await;
        }

        // ── Quality path below ──
        self.state.write().await.active_path = "quality".into();

        // Step 1: Propose — inject checkpoint + SOPs for better context
        let checkpoint_injection = self.get_checkpoint_injection().await;
        let sop_injection = {
            let loader = self.skill_loader.read().await;
            loader.resolve(Some(&student_state.concept), 2)
        };
        let enriched_history = format!("{}{}\n{}", checkpoint_injection, sop_injection, history);

        let proposal = self.proposer.propose(student_state, &enriched_history).await?;
        tracing::info!("📋 Proposed: {} (skill: {}, strategy: {})", proposal.prompt, proposal.skill, proposal.strategy);

        // ── Habit check: pre-consume ──
        let failed_rounds = self.state.read().await.failed_rounds;
        if failed_rounds >= 3 {
            let _ = self.habit_engine.trigger("H02", serde_json::json!({
                "consecutive_errors": failed_rounds,
                "skill": &proposal.skill,
                "concept": &proposal.concept,
            })).await;
        }

        // Step 2: Consume
        let result = self.consumer.consume(&proposal, student_query).await?;

        {
            let mut s = self.state.write().await;
            s.total_rounds += 1;
            if result.success {
                s.successful_rounds += 1;
                s.consecutive_same_errors = 0;
            } else {
                s.failed_rounds += 1;
                s.consecutive_same_errors += 1;
            }
        }

        let status_icon = if result.success { "✅" } else { "❌" };
        tracing::info!("{} Consumed: {} ({}ms)", status_icon, proposal.skill, result.duration_ms);

        // ── Habit check: post-consume ──
        {
            let _ = self.habit_engine.trigger("H03", serde_json::json!({
                "dialogue_ended": true,
                "skill": &proposal.skill,
                "success": result.success,
            })).await;
        }

        // Step 3: Rate
        let rating = self.rater.rate(
            &proposal,
            &result,
            &self.consumer.get_history(5).await,
            student_state.mastery_level,
        ).await?;

        tracing::info!("⭐ Rated: overall={:.0}%, comprehension={:.0}%",
            rating.overall_score * 100.0, rating.comprehension_score * 100.0);

        // Step 4: Deploy
        let decision = self.deployer.decide(&rating, &proposal.gene_profile).await;

        if decision.action == super::deployer::DeployAction::Deploy {
            self.deployer.execute_deployment(&decision).await?;
            self.state.write().await.deployed_strategies += 1;

            let _ = self.habit_engine.trigger("H05", serde_json::json!({
                "evolution_triggered": true,
                "composite": rating.overall_score,
                "action": "deploy",
            })).await;

            let _ = self.habit_engine.trigger("H04", serde_json::json!({
                "output_changed": true,
                "skill": &proposal.skill,
            })).await;
        }

        tracing::info!("🔧 Decision: {:?} — {}", decision.action, decision.reason);

        // Advance checkpoint after successful round
        self.advance_checkpoint().await;

        self.state.write().await.status = LoopStatus::Idle;
        Ok(rating)
    }

    // ── Working Checkpoint (GenericAgent-style short-term memory) ─────

    /// Get the checkpoint injection string for the current turn.
    async fn get_checkpoint_injection(&self) -> String {
        let cp = self.checkpoint.read().await;
        match cp.as_ref() {
            Some(c) if !c.is_empty() => c.to_prompt_injection(),
            _ => String::new(),
        }
    }

    /// Set or update the working checkpoint (called by MCP tool).
    pub async fn update_checkpoint(&self, key_info: &str, related_sop: Option<&str>) {
        let mut cp = self.checkpoint.write().await;
        match cp.as_mut() {
            Some(existing) => {
                existing.update(key_info);
                if let Some(sop) = related_sop {
                    existing.related_sop = Some(sop.into());
                }
                tracing::debug!("Checkpoint updated: {}", key_info);
            }
            None => {
                let mut new_cp = WorkingCheckpoint::new(key_info);
                if let Some(sop) = related_sop {
                    new_cp = new_cp.with_sop(sop);
                }
                *cp = Some(new_cp);
                tracing::debug!("Checkpoint created: {}", key_info);
            }
        }
    }

    /// Advance the checkpoint counter (called after each turn).
    async fn advance_checkpoint(&self) {
        let mut cp = self.checkpoint.write().await;
        if let Some(ref mut c) = *cp {
            c.increment_passed();
            if c.is_stale() {
                tracing::debug!("Checkpoint stale — clearing");
                *cp = None;
            }
        }
    }

    /// Get the current checkpoint (for API display).
    pub async fn get_checkpoint(&self) -> Option<WorkingCheckpoint> {
        self.checkpoint.read().await.clone()
    }

    /// Clear the checkpoint (task switch or reset).
    pub async fn clear_checkpoint(&self) {
        *self.checkpoint.write().await = None;
    }

    /// Access the skill loader (for querying before agent runs).
    pub async fn skill_loader(&self) -> tokio::sync::RwLockReadGuard<'_, skills::loader::SkillLoader> {
        self.skill_loader.read().await
    }

    /// Reload the skill index (after new skills are crystallized).
    pub async fn reload_skills(&self) {
        self.skill_loader.write().await.reload();
    }

    // ── State Queries ──────────────────────────────────────────────────

    pub async fn get_state(&self) -> LoopState {
        self.state.read().await.clone()
    }

    pub async fn get_stats(&self) -> LoopStats {
        let state = self.state.read().await.clone();
        let deployment_history = self.deployer.get_history().await;
        LoopStats {
            state,
            recent_ratings: vec![],
            deployment_history,
        }
    }

    pub async fn reset(&self) {
        let mut s = self.state.write().await;
        *s = LoopState {
            status: LoopStatus::Idle,
            iteration: 0,
            total_rounds: 0,
            successful_rounds: 0,
            failed_rounds: 0,
            deployed_strategies: 0,
            current_mastery: 0.0,
            last_error: None,
            consecutive_same_errors: 0,
            active_path: "quality".into(),
            fast_path_rounds: 0,
        };
        self.consumer.clear_history().await;
        self.clear_checkpoint().await;
    }
}
