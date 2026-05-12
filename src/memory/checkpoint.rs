// Working Checkpoint — short-term task memory
//
// Inspired by GenericAgent's update_working_checkpoint tool.
// A lightweight notepad (<500 char) that persists across agent loop turns
// without re-injecting the full conversation history.
//
// Usage:
//   Agent calls update_working_checkpoint("当前步骤3，已验证F=ma理解")
//   Next turn: checkpoint is auto-injected into the system prompt
//   Task completion: checkpoint is cleared

use serde::{Deserialize, Serialize};

/// Working checkpoint — key info carried across loop iterations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkingCheckpoint {
    /// Key info: current progress, constraints, findings (< 500 chars)
    pub key_info: String,
    /// Related SOP file names for quick reference
    pub related_sop: Option<String>,
    /// How many sessions have passed since this checkpoint was set
    pub passed_sessions: u32,
    /// When this checkpoint was created
    pub created_at: chrono::NaiveDateTime,
    /// When this checkpoint was last updated
    pub updated_at: chrono::NaiveDateTime,
}

impl WorkingCheckpoint {
    /// Create a new checkpoint
    pub fn new(key_info: &str) -> Self {
        let now = chrono::Utc::now().naive_utc();
        Self {
            key_info: key_info.into(),
            related_sop: None,
            passed_sessions: 0,
            created_at: now,
            updated_at: now,
        }
    }

    /// Update the checkpoint with new key_info
    pub fn update(&mut self, key_info: &str) {
        self.key_info = key_info.into();
        self.passed_sessions = 0;
        self.updated_at = chrono::Utc::now().naive_utc();
    }

    /// Set related SOP reference
    pub fn with_sop(mut self, sop_name: &str) -> Self {
        self.related_sop = Some(sop_name.into());
        self
    }

    /// Increment passed_sessions count
    pub fn increment_passed(&mut self) {
        self.passed_sessions = self.passed_sessions.saturating_add(1);
    }

    /// Format for injection into system prompt
    pub fn to_prompt_injection(&self) -> String {
        let mut s = format!(
            "[工作记忆] {}\n[SYSTEM] 此为 {} 个对话前设置的 key_info，若已在新任务，请先更新或清除工作记忆。\n",
            self.key_info,
            self.passed_sessions,
        );
        if let Some(ref sop) = self.related_sop {
            s.push_str(&format!("有不清晰的地方请重新读取: {}\n", sop));
        }
        s
    }

    /// Check if this checkpoint is stale (> 5 sessions passed)
    pub fn is_stale(&self) -> bool {
        self.passed_sessions > 5
    }

    /// Check if key_info is empty
    pub fn is_empty(&self) -> bool {
        self.key_info.is_empty()
    }
}

impl Default for WorkingCheckpoint {
    fn default() -> Self {
        Self::new("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checkpoint_create_and_update() {
        let mut cp = WorkingCheckpoint::new("步骤1: 诊断学生基础");
        assert!(cp.to_prompt_injection().contains("步骤1"));
        assert_eq!(cp.passed_sessions, 0);

        cp.update("步骤2: 开始探究式教学");
        assert!(cp.key_info.contains("步骤2"));
        assert_eq!(cp.passed_sessions, 0);
    }

    #[test]
    fn test_stale_detection() {
        let mut cp = WorkingCheckpoint::new("test");
        for _ in 0..6 {
            cp.increment_passed();
        }
        assert!(cp.is_stale());
    }

    #[test]
    fn test_with_sop() {
        let cp = WorkingCheckpoint::new("使用苏格拉底追问").with_sop("tais_socratic_tutor.md");
        assert!(cp.to_prompt_injection().contains("tais_socratic_tutor.md"));
    }

    #[test]
    fn test_empty_checkpoint() {
        let cp = WorkingCheckpoint::default();
        assert!(cp.is_empty());
        assert!(cp.to_prompt_injection().contains("[工作记忆]"));
    }
}
