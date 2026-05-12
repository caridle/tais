// Habit Engine core types — state, rules, conditions, actions
//
// Defines the type system for the 7 habit capsules (H01-H07).
// Each habit has a rule definition (immutable) and a runtime state (evolving).

use serde::{Deserialize, Serialize};

// ── Trigger Type ────────────────────────────────────────────────────────────

/// How a habit is triggered
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TriggerType {
    /// Time-based (H01: daily review at 23:00)
    Periodic,
    /// Reacts to system events (H02, H03, H04, H05)
    EventDriven,
    /// Gated on a condition (H06, H07)
    Conditional,
}

// ── Habit Condition ─────────────────────────────────────────────────────────

/// When to fire the habit
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HabitCondition {
    /// Fire every time (used primarily for testing)
    Always,
    /// N consecutive same-type errors detected (H02)
    ErrorPattern { consecutive_errors: u32 },
    /// Composite evolution score below threshold (H05)
    CompositeBelow { threshold: f64 },
    /// High-risk operation about to execute (H06)
    HighRiskOperation,
    /// Multiple agents collaborating (H07)
    MultiAgentMode,
    /// Output or gene profile changed (H04)
    OutputChanged,
    /// Dialogue just ended (H03)
    DialogueEnded,
    /// Evolution engine triggered (H05)
    EvolutionTriggered,
    /// Scheduled time (H01 daily review)
    ScheduledTime { hour: u32, minute: u32 },
}

// ── Habit Action ────────────────────────────────────────────────────────────

/// What the habit does when triggered
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HabitAction {
    /// H01: Summarize today's decisions and error patterns
    SummarizeDecisions,
    /// H02: Switch to a different teaching strategy
    SwitchStrategy,
    /// H03: Confirm student understanding, summarize consensus
    ConfirmUnderstanding,
    /// H04: Update logs, changelog, version annotations
    UpdateLogs,
    /// H05: Self-review current state, compare with history
    SelfReview,
    /// H06: Run mandatory safety checklist
    SafetyChecklist,
    /// H07: Handshake protocol + decompose task
    HandshakeAndDecompose,
}

// ── Habit Rule ──────────────────────────────────────────────────────────────

/// A habit rule definition (immutable after registration)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HabitRule {
    /// Unique identifier: "H01", "H02", ...
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// What this habit does
    pub description: String,
    /// How it's triggered
    pub trigger_type: TriggerType,
    /// When to fire
    pub condition: HabitCondition,
    /// What to do when fired
    pub action: HabitAction,
    /// Reinforcement learning rate (eta)
    pub learning_rate: f64,
    /// Decay coefficient (lambda)
    pub decay_rate: f64,
}

// ── Habit State ─────────────────────────────────────────────────────────────

/// Runtime state of a habit (mutable, evolves over time)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HabitState {
    /// Which rule this state belongs to
    pub rule_id: String,
    /// Current weight H(t) in [0, 1]
    pub weight: f64,
    /// Total successful applications
    pub success_count: u32,
    /// Total failed applications
    pub failure_count: u32,
    /// Consecutive successes
    pub streak: u32,
    /// Last time this habit was triggered
    pub last_triggered: chrono::NaiveDateTime,
    /// Whether the habit is in auto-execute mode
    pub is_auto: bool,
}

impl Default for HabitState {
    fn default() -> Self {
        Self {
            rule_id: String::new(),
            weight: 0.5,
            success_count: 0,
            failure_count: 0,
            streak: 0,
            last_triggered: chrono::Utc::now().naive_utc(),
            is_auto: false,
        }
    }
}

// ── Constants ───────────────────────────────────────────────────────────────

/// Above this threshold, habit is stable and auto-executes
pub const THETA_STABLE: f64 = 0.8;
/// Below this threshold, habit is degraded and needs retraining
pub const THETA_RETRAIN: f64 = 0.3;
/// Sliding window size for frequency calculation
pub const WINDOW_SIZE: usize = 20;

// ── Habit Log ───────────────────────────────────────────────────────────────

/// A record of one habit execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HabitLog {
    pub id: String,
    pub rule_id: String,
    pub triggered_at: chrono::NaiveDateTime,
    pub context: serde_json::Value,
    pub action_result: String,
    pub success: bool,
    pub duration_ms: u64,
}

impl HabitLog {
    pub fn new(rule_id: &str, context: serde_json::Value) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            rule_id: rule_id.into(),
            triggered_at: chrono::Utc::now().naive_utc(),
            context,
            action_result: String::new(),
            success: false,
            duration_ms: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_habit_state_default() {
        let state = HabitState::default();
        assert_eq!(state.weight, 0.5);
        assert!(!state.is_auto);
        assert_eq!(state.success_count, 0);
        assert_eq!(state.failure_count, 0);
        assert_eq!(state.streak, 0);
    }

    #[test]
    fn test_habit_state_serde() {
        let state = HabitState {
            rule_id: "H01".into(),
            weight: 0.75,
            success_count: 10,
            failure_count: 2,
            streak: 5,
            last_triggered: chrono::NaiveDateTime::parse_from_str("2026-05-10 23:00:00", "%Y-%m-%d %H:%M:%S").unwrap(),
            is_auto: false,
        };
        let json = serde_json::to_string(&state).unwrap();
        let decoded: HabitState = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.rule_id, "H01");
        assert_eq!(decoded.weight, 0.75);
    }

    #[test]
    fn test_habit_rule_serde() {
        let rule = HabitRule {
            id: "H01".into(),
            name: "复盘".into(),
            description: "每日定时总结".into(),
            trigger_type: TriggerType::Periodic,
            condition: HabitCondition::ScheduledTime { hour: 23, minute: 0 },
            action: HabitAction::SummarizeDecisions,
            learning_rate: 0.10,
            decay_rate: 0.05,
        };
        let json = serde_json::to_string(&rule).unwrap();
        let decoded: HabitRule = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.id, "H01");
        assert_eq!(decoded.learning_rate, 0.10);
    }

    #[test]
    fn test_threshold_constants() {
        assert!(THETA_STABLE > THETA_RETRAIN);
        assert!(THETA_STABLE <= 1.0);
        assert!(THETA_RETRAIN >= 0.0);
    }
}
