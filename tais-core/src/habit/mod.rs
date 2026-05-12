// Habit Engine — self-evolving behavior system
//
// Manages 7 habit capsules (H01-H07) that observe agent behavior, learn from
// outcomes, and gradually automate good practices while flagging degradation.
//
// Architecture:
//   - rules:  RwLock<Vec<HabitRule>>   — registered habit definitions
//   - states: RwLock<HashMap<K,V>>     — runtime state per habit
//   - logs:   RwLock<Vec<HabitLog>>   — execution history
//
// Formula: H(t+1) = H(t) + η·success - λ·(1 - frequency)
//   where frequency = success_rate over last WINDOW_SIZE (20) triggers

pub mod state;
pub mod rules;

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::habit::state::*;

/// The core habit engine — shared across all subsystems via Arc<>
pub struct HabitEngine {
    /// All registered habit rules (7 by default, extensible)
    rules: RwLock<Vec<HabitRule>>,
    /// Runtime state for each rule, keyed by rule_id
    states: RwLock<HashMap<String, HabitState>>,
    /// Execution log (most recent first)
    logs: RwLock<Vec<HabitLog>>,
    /// Optional database pool for log persistence
    db_pool: RwLock<Option<sqlx::SqlitePool>>,
}

impl HabitEngine {
    /// Create a new HabitEngine with neutral initial states.
    pub fn new() -> Self {
        Self {
            rules: RwLock::new(Vec::new()),
            states: RwLock::new(HashMap::new()),
            logs: RwLock::new(Vec::new()),
            db_pool: RwLock::new(None),
        }
    }

    /// Attach a database pool for log persistence.
    pub async fn with_db(&self, pool: sqlx::SqlitePool) {
        *self.db_pool.write().await = Some(pool);
    }

    // ── Rule Management ─────────────────────────────────────────────────

    /// Register a habit rule and initialize its state if new.
    pub async fn register(&self, rule: HabitRule) -> Result<(), String> {
        let rule_id = rule.id.clone();

        // Initialize state entry if this is a new rule
        let mut states = self.states.write().await;
        states.entry(rule_id.clone()).or_insert_with(|| HabitState {
            rule_id: rule_id.clone(),
            weight: 0.5,
            ..Default::default()
        });

        self.rules.write().await.push(rule);
        Ok(())
    }

    /// Get all registered rules.
    pub async fn get_rules(&self) -> Vec<HabitRule> {
        self.rules.read().await.clone()
    }

    /// Look up a rule by ID.
    pub async fn get_rule(&self, rule_id: &str) -> Option<HabitRule> {
        self.rules.read().await.iter()
            .find(|r| r.id == rule_id)
            .cloned()
    }

    // ── State Queries ───────────────────────────────────────────────────

    /// Get the runtime state for a specific habit.
    pub async fn get_state(&self, rule_id: &str) -> Option<HabitState> {
        self.states.read().await.get(rule_id).cloned()
    }

    /// Get all habit states.
    pub async fn get_all_states(&self) -> Vec<HabitState> {
        self.states.read().await.values().cloned().collect()
    }

    /// Check if a habit weight exceeds the stable threshold.
    pub async fn is_stable(&self, rule_id: &str) -> bool {
        self.states.read().await.get(rule_id)
            .map(|s| s.weight > THETA_STABLE)
            .unwrap_or(false)
    }

    /// Check if a habit is in auto-execute mode.
    pub async fn is_auto(&self, rule_id: &str) -> bool {
        self.states.read().await.get(rule_id)
            .map(|s| s.is_auto)
            .unwrap_or(false)
    }

    // ── Logs ────────────────────────────────────────────────────────────

    /// Get execution logs, optionally filtered by rule_id.
    /// limit=0 means unlimited.
    pub async fn get_logs(&self, rule_id: Option<&str>, limit: usize) -> Vec<HabitLog> {
        let logs = self.logs.read().await;
        let filtered: Vec<HabitLog> = match rule_id {
            Some(rid) => logs.iter().filter(|l| l.rule_id == rid).cloned().collect(),
            None => logs.clone(),
        };
        if limit > 0 && filtered.len() > limit {
            filtered[..limit].to_vec()
        } else {
            filtered
        }
    }

    // ── Core Trigger + Weight Update ─────────────────────────────────────

    /// Trigger a habit rule and return the execution log.
    /// This is the main entry point for habit execution:
    /// 1. Evaluate whether the condition matches the context
    /// 2. Execute the action (or simulate it for now)
    /// 3. Log the result
    /// 4. Update the habit weight
    pub async fn trigger(
        &self,
        rule_id: &str,
        context: serde_json::Value,
    ) -> Result<HabitLog, String> {
        let rule = self.get_rule(rule_id).await
            .ok_or_else(|| format!("Habit rule not found: {rule_id}"))?;

        let start = std::time::Instant::now();
        let mut log = HabitLog::new(rule_id, context.clone());

        // Evaluate condition against context
        let should_fire = evaluate_condition(&rule.condition, &context);

        if should_fire {
            // Execute the action (record intent for now)
            log.action_result = execute_action(&rule.action, &context);
            log.success = true;
        } else {
            log.action_result = "condition not met — skipped".into();
            log.success = false;
        }

        log.duration_ms = start.elapsed().as_millis() as u64;

        // Update weight based on outcome
        self.update_weight(rule_id, log.success).await;

        // Persist log
        self.logs.write().await.push(log.clone());

        // Also persist to DB if available
        if let Some(ref pool) = *self.db_pool.read().await {
            let _ = persist_log(pool, &log).await;
        }

        Ok(log)
    }

    /// Update habit weight using the reinforcement formula:
    ///   H(t+1) = H(t) + η·success - λ·(1 - frequency)
    ///
    /// where frequency = recent_success_rate over WINDOW_SIZE sliding window.
    pub async fn update_weight(&self, rule_id: &str, success: bool) {
        let rules = self.rules.read().await;
        let rule = match rules.iter().find(|r| r.id == rule_id) {
            Some(r) => r.clone(),
            None => return,
        };
        drop(rules);

        let mut states = self.states.write().await;
        let state = match states.get_mut(rule_id) {
            Some(s) => s,
            None => return,
        };

        // Compute recent frequency from logs
        let frequency = {
            let logs = self.logs.read().await;
            let recent: Vec<bool> = logs.iter()
                .filter(|l| l.rule_id == rule_id)
                .take(WINDOW_SIZE)
                .map(|l| l.success)
                .collect();
            if recent.is_empty() {
                0.5 // neutral prior
            } else {
                recent.iter().filter(|&&s| s).count() as f64 / recent.len() as f64
            }
        };

        // Apply update formula: H(t+1) = H(t) + η·success - λ·(1 - frequency)
        let eta = rule.learning_rate;
        let lambda = rule.decay_rate;
        let success_term = if success { eta } else { 0.0 };
        let decay_term = lambda * (1.0 - frequency);

        state.weight = (state.weight + success_term - decay_term).clamp(0.0, 1.0);

        // Update counters
        if success {
            state.success_count = state.success_count.saturating_add(1);
            state.streak = state.streak.saturating_add(1);
        } else {
            state.failure_count = state.failure_count.saturating_add(1);
            state.streak = 0;
        }

        state.last_triggered = chrono::Utc::now().naive_utc();

        // Check stability thresholds
        if state.weight > THETA_STABLE {
            if !state.is_auto {
                tracing::info!(
                    "Habit {} ({}) reached stability (weight={:.3}) — now auto-executing",
                    rule_id, rule.name, state.weight
                );
            }
            state.is_auto = true;
        } else if state.weight < THETA_RETRAIN {
            if state.is_auto {
                tracing::warn!(
                    "Habit {} ({}) degraded (weight={:.3}) — needs retraining",
                    rule_id, rule.name, state.weight
                );
            }
            state.is_auto = false;
        }
    }

    // ── Scheduler ───────────────────────────────────────────────────────

    /// Start the background scheduler for periodic habits (H01 daily review).
    /// Spawns a tokio task that checks every 60 seconds if any periodic rule
    /// should fire based on current time matching its ScheduledTime condition.
    pub async fn start_scheduler(self: &Arc<Self>) {
        let engine = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;

                let rules = engine.get_rules().await;
                let now = chrono::Utc::now();

                for rule in &rules {
                    if rule.trigger_type != TriggerType::Periodic {
                        continue;
                    }

                    let should_fire = match &rule.condition {
                        HabitCondition::ScheduledTime { hour, minute } => {
                            now.format("%H:%M").to_string() == format!("{:02}:{:02}", hour, minute)
                        }
                        _ => false,
                    };

                    if should_fire {
                        let context = serde_json::json!({
                            "trigger": "scheduler",
                            "time": now.format("%Y-%m-%d %H:%M:%S").to_string(),
                        });
                        match engine.trigger(&rule.id, context).await {
                            Ok(log) => {
                                tracing::info!(
                                    "Scheduler: triggered {} ({}) — success={}",
                                    rule.id, rule.name, log.success,
                                );
                            }
                            Err(e) => {
                                tracing::warn!("Scheduler: failed to trigger {}: {e}", rule.id);
                            }
                        }
                    }
                }
            }
        });
    }
}

impl Default for HabitEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ── Internal Helpers ────────────────────────────────────────────────────────

/// Evaluate whether a habit condition matches the given context.
fn evaluate_condition(condition: &HabitCondition, context: &serde_json::Value) -> bool {
    match condition {
        HabitCondition::Always => true,
        HabitCondition::ErrorPattern { consecutive_errors } => {
            context.get("consecutive_errors")
                .and_then(|v| v.as_u64())
                .map(|n| n >= *consecutive_errors as u64)
                .unwrap_or(false)
        }
        HabitCondition::CompositeBelow { threshold } => {
            context.get("composite")
                .and_then(|v| v.as_f64())
                .map(|c| c < *threshold)
                .unwrap_or(false)
        }
        HabitCondition::HighRiskOperation => {
            context.get("high_risk")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        }
        HabitCondition::MultiAgentMode => {
            context.get("multi_agent")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        }
        HabitCondition::OutputChanged => {
            context.get("output_changed")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        }
        HabitCondition::DialogueEnded => {
            context.get("dialogue_ended")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        }
        HabitCondition::EvolutionTriggered => {
            context.get("evolution_triggered")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        }
        HabitCondition::ScheduledTime { .. } => {
            // ScheduledTime is evaluated by the scheduler, not context
            context.get("force")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        }
    }
}

/// Execute a habit action and return a description of what was done.
fn execute_action(action: &HabitAction, _context: &serde_json::Value) -> String {
    match action {
        HabitAction::SummarizeDecisions => {
            "Generated daily review summary of decisions and error patterns".into()
        }
        HabitAction::SwitchStrategy => {
            "Switched teaching strategy due to repeated error pattern".into()
        }
        HabitAction::ConfirmUnderstanding => {
            "Confirmed student understanding and summarized dialogue consensus".into()
        }
        HabitAction::UpdateLogs => {
            "Updated changelog and version annotations for output changes".into()
        }
        HabitAction::SelfReview => {
            "Performed self-review and compared current state against historical baselines".into()
        }
        HabitAction::SafetyChecklist => {
            "Executed mandatory safety checklist for high-risk operation".into()
        }
        HabitAction::HandshakeAndDecompose => {
            "Performed multi-agent handshake and decomposed collaborative task".into()
        }
    }
}

/// Persist a habit log to the SQLite database.
async fn persist_log(pool: &sqlx::SqlitePool, log: &HabitLog) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO habit_logs (rule_id, triggered_at, context, action_result, success, duration_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
    )
    .bind(&log.rule_id)
    .bind(log.triggered_at.format("%Y-%m-%d %H:%M:%S").to_string())
    .bind(log.context.to_string())
    .bind(&log.action_result)
    .bind(log.success as i32)
    .bind(log.duration_ms as i64)
    .execute(pool)
    .await?;
    Ok(())
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_register_habit() {
        let engine = HabitEngine::new();
        let rule = rules::h01_review();
        engine.register(rule).await.unwrap();

        let rules = engine.get_rules().await;
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].id, "H01");

        let state = engine.get_state("H01").await.unwrap();
        assert_eq!(state.weight, 0.5);
        assert!(!state.is_auto);
    }

    #[tokio::test]
    async fn test_trigger_and_weight_update() {
        let engine = HabitEngine::new();
        engine.register(rules::h01_review()).await.unwrap();

        // Trigger with Always-like context (force=true for scheduled)
        let log = engine.trigger("H01", serde_json::json!({"force": true})).await.unwrap();

        assert_eq!(log.rule_id, "H01");
        // With force=true, scheduled condition is met
        let state = engine.get_state("H01").await.unwrap();
        assert!(state.success_count > 0 || state.failure_count > 0,
            "Weight should be updated after trigger");
    }

    #[tokio::test]
    async fn test_weight_convergence_to_stable() {
        let engine = HabitEngine::new();
        engine.register(rules::h02_error_handling()).await.unwrap();

        // Simulate many successful triggers (error pattern)
        for _ in 0..30 {
            engine.trigger("H02", serde_json::json!({"consecutive_errors": 5})).await.unwrap();
        }

        let state = engine.get_state("H02").await.unwrap();
        // After 30 successful applications with eta=0.15,
        // weight should have increased significantly
        assert!(state.weight > 0.5, "Weight should increase with repeated success, got {}", state.weight);
        assert!(state.is_auto || state.weight <= THETA_STABLE,
            "is_auto should match weight > THETA_STABLE");
    }

    #[tokio::test]
    async fn test_condition_error_pattern() {
        let engine = HabitEngine::new();
        engine.register(rules::h02_error_handling()).await.unwrap();

        // Context with 2 consecutive errors — should NOT fire (needs 3+)
        let log = engine.trigger("H02", serde_json::json!({"consecutive_errors": 2})).await.unwrap();
        assert!(!log.success, "Should not fire for < 3 consecutive errors");

        // Context with 5 consecutive errors — SHOULD fire
        let log = engine.trigger("H02", serde_json::json!({"consecutive_errors": 5})).await.unwrap();
        assert!(log.success, "Should fire for >= 3 consecutive errors");
    }

    #[tokio::test]
    async fn test_condition_high_risk() {
        let engine = HabitEngine::new();
        engine.register(rules::h06_security()).await.unwrap();

        // Non-high-risk context
        let log = engine.trigger("H06", serde_json::json!({"high_risk": false})).await.unwrap();
        assert!(!log.success, "Should not fire for non-high-risk operations");

        // High-risk context
        let log = engine.trigger("H06", serde_json::json!({"high_risk": true})).await.unwrap();
        assert!(log.success, "Should fire for high-risk operations");
    }

    #[tokio::test]
    async fn test_all_7_habits_registered() {
        let engine = HabitEngine::new();
        for rule in rules::all_habit_rules() {
            engine.register(rule).await.unwrap();
        }

        let rules = engine.get_rules().await;
        assert_eq!(rules.len(), 7);

        let states = engine.get_all_states().await;
        assert_eq!(states.len(), 7);

        // All states should start at weight 0.5
        for state in &states {
            assert_eq!(state.weight, 0.5, "{} should start at 0.5", state.rule_id);
            assert!(!state.is_auto);
        }
    }

    #[tokio::test]
    async fn test_threshold_transitions() {
        let engine = HabitEngine::new();
        engine.register(HabitRule {
            id: "TEST".into(),
            name: "Test".into(),
            description: "Test habit".into(),
            trigger_type: TriggerType::EventDriven,
            condition: HabitCondition::Always,
            action: HabitAction::UpdateLogs,
            learning_rate: 0.5,  // High learning rate for fast testing
            decay_rate: 0.01,
        }).await.unwrap();

        // Push weight above THETA_STABLE (0.8) with many successes
        for _ in 0..10 {
            engine.trigger("TEST", serde_json::json!({})).await.unwrap();
        }

        let state = engine.get_state("TEST").await.unwrap();
        assert!(state.weight > THETA_STABLE, "Weight should exceed THETA_STABLE");
        assert!(state.is_auto, "Should be in auto mode");
    }

    #[tokio::test]
    async fn test_get_logs_filtered() {
        let engine = HabitEngine::new();
        engine.register(rules::h01_review()).await.unwrap();
        engine.register(rules::h02_error_handling()).await.unwrap();

        engine.trigger("H01", serde_json::json!({"force": true})).await.unwrap();
        engine.trigger("H02", serde_json::json!({"consecutive_errors": 5})).await.unwrap();

        let all_logs = engine.get_logs(None, 0).await;
        assert_eq!(all_logs.len(), 2);

        let h01_logs = engine.get_logs(Some("H01"), 0).await;
        assert_eq!(h01_logs.len(), 1);
        assert_eq!(h01_logs[0].rule_id, "H01");

        // Test limit
        let limited = engine.get_logs(None, 1).await;
        assert_eq!(limited.len(), 1);
    }
}
