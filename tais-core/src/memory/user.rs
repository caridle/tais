// User Memory — per-user personal data
//
// Architecture:
//   UserProfile        — user identity, preferences, grade level
//   MasteryMap         — concept → mastery level, exposures
//   MisconceptionArchive — this student's persistent errors
//   UserMemoryStore    — manages all per-user data
//
// 与 SharedMemory 的关系：
//   User Memory  = 个人（只有这个学生自己的数据）
//   Shared Memory = 集体（所有学生 + 老师共同的知识）
//   TAIS 教学时两者合并：从 Shared 取策略 + 从 User 取个人状态

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

// ── User LLM Config ──────────────────────────────────────────────────

/// Per-user LLM configuration — each student can have their own model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserLlmConfig {
    pub provider: String,    // "openai" | "anthropic" | "ollama"
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,     // student's own API key (default empty, keep old if not provided)
    pub model: String,
}

// ── Mastery Entry ───────────────────────────────────────────────────────

/// A single concept mastery record for one student
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MasteryEntry {
    pub concept: String,
    pub level: f64,              // 0.0 - 1.0: 掌握程度
    pub exposures: u32,          // 接触次数
    pub correct_responses: u32,  // 正确回答次数
    pub last_practiced: chrono::NaiveDateTime,
    pub first_seen: chrono::NaiveDateTime,
}

impl MasteryEntry {
    pub fn new(concept: &str) -> Self {
        let now = chrono::Utc::now().naive_utc();
        Self {
            concept: concept.into(),
            level: 0.0,
            exposures: 0,
            correct_responses: 0,
            last_practiced: now,
            first_seen: now,
        }
    }

    /// Update mastery after a correct response
    pub fn record_correct(&mut self) {
        self.exposures += 1;
        self.correct_responses += 1;
        // Bayesian update: more weight on recent observations when few samples
        let n = self.exposures as f64;
        self.level = (self.level * (n - 1.0) + 1.0) / n;
        self.last_practiced = chrono::Utc::now().naive_utc();
    }

    /// Update mastery after an incorrect response (mild decay)
    pub fn record_incorrect(&mut self) {
        self.exposures += 1;
        let n = self.exposures as f64;
        self.level = (self.level * (n - 1.0) + 0.2) / n; // partial credit for attempt
        self.last_practiced = chrono::Utc::now().naive_utc();
    }

    /// Mastery category
    pub fn category(&self) -> MasteryCategory {
        if self.exposures == 0 {
            MasteryCategory::Unknown
        } else if self.level >= 0.85 {
            MasteryCategory::Mastered
        } else if self.level >= 0.60 {
            MasteryCategory::Developing
        } else if self.level >= 0.30 {
            MasteryCategory::Struggling
        } else {
            MasteryCategory::Beginner
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MasteryCategory {
    Unknown,
    Beginner,
    Struggling,
    Developing,
    Mastered,
}

// ── Misconception Record ────────────────────────────────────────────────

/// A persistent error pattern for this student
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MisconceptionRecord {
    pub description: String,
    pub related_concept: String,
    pub occurrences: u32,
    pub first_seen: chrono::NaiveDateTime,
    pub last_seen: chrono::NaiveDateTime,
    pub resolved: bool,          // has the student overcome this?
}

// ── User Profile ────────────────────────────────────────────────────────

/// Complete user profile with identity, preferences, and learning state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    pub user_id: String,
    pub display_name: Option<String>,
    pub grade_level: Option<String>,           // e.g. "高一", "大三"
    pub preferred_subjects: Vec<String>,
    pub preferred_style: Option<String>,       // "visual" | "verbal" | "hands-on"
    pub mastery: HashMap<String, MasteryEntry>,
    pub misconceptions: Vec<MisconceptionRecord>,
    pub total_sessions: u32,
    pub total_turns: u32,
    pub llm_config: Option<UserLlmConfig>,   // 个人 LLM 配置
    pub password_hash: Option<String>,       // argon2id 密码哈希
    pub wechat_openid: Option<String>,       // 绑定的微信公众号 OpenID
    pub first_seen: chrono::NaiveDateTime,
    pub last_active: chrono::NaiveDateTime,
}

impl UserProfile {
    pub fn new(user_id: &str) -> Self {
        let now = chrono::Utc::now().naive_utc();
        Self {
            user_id: user_id.into(),
            display_name: None,
            grade_level: None,
            preferred_subjects: vec![],
            preferred_style: None,
            mastery: HashMap::new(),
            misconceptions: vec![],
            total_sessions: 0,
            total_turns: 0,
            llm_config: None,
            password_hash: None,
            wechat_openid: None,
            first_seen: now,
            last_active: now,
        }
    }

    /// Get mastery for a concept, creating entry if not exists
    pub fn get_or_create_mastery(&mut self, concept: &str) -> &mut MasteryEntry {
        self.mastery
            .entry(concept.into())
            .or_insert_with(|| MasteryEntry::new(concept))
    }

    /// Record a misconception
    pub fn record_misconception(&mut self, concept: &str, description: &str) {
        let now = chrono::Utc::now().naive_utc();
        if let Some(found) = self.misconceptions.iter_mut().find(|m| m.description == description) {
            found.occurrences += 1;
            found.last_seen = now;
        } else {
            self.misconceptions.push(MisconceptionRecord {
                description: description.into(),
                related_concept: concept.into(),
                occurrences: 1,
                first_seen: now,
                last_seen: now,
                resolved: false,
            });
        }
    }

    /// Get the top-N weakest concepts (lowest mastery, highest exposure)
    pub fn weakest_concepts(&self, n: usize) -> Vec<&MasteryEntry> {
        let mut entries: Vec<_> = self.mastery.values().collect();
        entries.sort_by(|a, b| {
            a.level
                .partial_cmp(&b.level)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        entries.truncate(n);
        entries
    }

    /// Get active (unresolved) misconceptions
    pub fn active_misconceptions(&self) -> Vec<&MisconceptionRecord> {
        self.misconceptions.iter().filter(|m| !m.resolved).collect()
    }

    /// Build a user-specific context string for LLM injection
    pub fn build_user_context(&self) -> String {
        let mut parts = Vec::new();

        // Profile
        if let Some(ref name) = self.display_name {
            parts.push(format!("学生: {name}"));
        }
        if let Some(ref grade) = self.grade_level {
            parts.push(format!("年级: {grade}"));
        }

        // Mastery summary
        if !self.mastery.is_empty() {
            let mastered: Vec<_> = self
                .mastery
                .values()
                .filter(|m| m.category() == MasteryCategory::Mastered)
                .map(|m| m.concept.as_str())
                .collect();
            let weak: Vec<_> = self
                .mastery
                .values()
                .filter(|m| matches!(m.category(), MasteryCategory::Struggling | MasteryCategory::Beginner))
                .map(|m| format!("{}({:.0}%)", m.concept, m.level * 100.0))
                .collect();

            if !mastered.is_empty() {
                parts.push(format!("已掌握: {}", mastered.join("、")));
            }
            if !weak.is_empty() {
                parts.push(format!("薄弱点: {}", weak.join("、")));
            }
        }

        // Active misconceptions
        let active = self.active_misconceptions();
        if !active.is_empty() {
            let descs: Vec<_> = active.iter().map(|m| m.description.as_str()).collect();
            parts.push(format!("持续误解: {}", descs.join("；")));
        }

        if parts.is_empty() {
            String::new()
        } else {
            format!("【学生档案】\n{}\n", parts.join("\n"))
        }
    }
}

// ── UserMemoryStore ─────────────────────────────────────────────────────

/// Manages all per-user memory
pub struct UserMemoryStore {
    users: RwLock<HashMap<String, UserProfile>>,
    persist_path: Option<String>,
}

impl UserMemoryStore {
    pub fn new() -> Self {
        Self {
            users: RwLock::new(HashMap::new()),
            persist_path: None,
        }
    }

    pub fn with_persistence(mut self, path: &str) -> Self {
        self.persist_path = Some(path.into());
        self
    }

    /// Get or create a user profile
    pub async fn get_or_create(&self, user_id: &str) -> UserProfile {
        let mut users = self.users.write().await;
        users
            .entry(user_id.into())
            .or_insert_with(|| UserProfile::new(user_id))
            .clone()
    }

    /// Get user profile (returns None if never seen)
    pub async fn get(&self, user_id: &str) -> Option<UserProfile> {
        let users = self.users.read().await;
        users.get(user_id).cloned()
    }

    /// Update a user profile
    pub async fn update(&self, profile: UserProfile) {
        let mut users = self.users.write().await;
        users.insert(profile.user_id.clone(), profile);
    }

    /// Record a learning event for a user
    pub async fn record_learning(
        &self,
        user_id: &str,
        concept: &str,
        correct: bool,
        misconception: Option<&str>,
    ) -> UserProfile {
        let mut users = self.users.write().await;
        let profile = users
            .entry(user_id.into())
            .or_insert_with(|| UserProfile::new(user_id));

        let mastery = profile.get_or_create_mastery(concept);
        if correct {
            mastery.record_correct();
        } else {
            mastery.record_incorrect();
        }

        if let Some(mis) = misconception {
            profile.record_misconception(concept, mis);
        }

        profile.total_turns += 1;
        profile.last_active = chrono::Utc::now().naive_utc();

        profile.clone()
    }

    /// List all users
    pub async fn list_users(&self) -> Vec<UserProfile> {
        let users = self.users.read().await;
        users.values().cloned().collect()
    }

    /// List all user IDs
    pub async fn list_user_ids(&self) -> Vec<String> {
        let users = self.users.read().await;
        users.keys().cloned().collect()
    }

    /// Find a user by username (user_id or display_name)
    pub async fn find_by_username(&self, username: &str) -> Option<UserProfile> {
        let users = self.users.read().await;
        users
            .values()
            .find(|p| p.user_id == username || p.display_name.as_deref() == Some(username))
            .cloned()
    }

    /// Find a user by WeChat OpenID
    pub async fn find_by_wechat_openid(&self, openid: &str) -> Option<UserProfile> {
        let users = self.users.read().await;
        users
            .values()
            .find(|p| p.wechat_openid.as_deref() == Some(openid))
            .cloned()
    }

    /// Register a new user with hashed password
    pub async fn register(
        &self,
        user_id: &str,
        display_name: Option<&str>,
        password_hash: &str,
    ) -> Result<UserProfile, String> {
        let mut users = self.users.write().await;
        if users.contains_key(user_id) {
            return Err(format!("用户名 '{}' 已存在", user_id));
        }
        // Also check display_name uniqueness
        if let Some(ref name) = display_name {
            if users.values().any(|p| p.display_name.as_deref() == Some(name)) {
                return Err(format!("显示名 '{}' 已被使用", name));
            }
        }

        let now = chrono::Utc::now().naive_utc();
        let profile = UserProfile {
            user_id: user_id.into(),
            display_name: display_name.map(String::from),
            password_hash: Some(password_hash.into()),
            ..UserProfile::new(user_id)
        };
        // Overwrite first_seen with consistent now
        let mut profile = profile;
        profile.first_seen = now;
        profile.last_active = now;

        users.insert(user_id.into(), profile.clone());
        Ok(profile)
    }

    /// Bind WeChat OpenID to a user
    pub async fn bind_wechat(&self, user_id: &str, openid: &str) -> Option<UserProfile> {
        let mut users = self.users.write().await;

        // Unbind this OpenID from any other user first
        for (_, profile) in users.iter_mut() {
            if profile.wechat_openid.as_deref() == Some(openid) {
                profile.wechat_openid = None;
            }
        }

        // Bind to target user
        if let Some(profile) = users.get_mut(user_id) {
            profile.wechat_openid = Some(openid.into());
            Some(profile.clone())
        } else {
            None
        }
    }

    /// Set or update LLM config for a user (keeps existing api_key if not provided)
    pub async fn set_llm_config(&self, user_id: &str, mut config: UserLlmConfig) -> Option<UserProfile> {
        let mut users = self.users.write().await;
        if let Some(profile) = users.get_mut(user_id) {
            // Keep existing API key if new one is empty
            if config.api_key.is_empty() {
                if let Some(ref old) = profile.llm_config {
                    config.api_key = old.api_key.clone();
                }
            }
            profile.llm_config = Some(config);
            Some(profile.clone())
        } else {
            None
        }
    }

    /// Get LLM config for a user
    pub async fn get_llm_config(&self, user_id: &str) -> Option<UserLlmConfig> {
        let users = self.users.read().await;
        users.get(user_id).and_then(|p| p.llm_config.clone())
    }

    /// Get sessions linked to a user (reverse lookup)
    pub async fn get_user_sessions(&self, session_users: &tokio::sync::RwLock<std::collections::HashMap<String, String>>, user_id: &str) -> Vec<String> {
        let map = session_users.read().await;
        map.iter()
            .filter(|(_, uid)| *uid == user_id)
            .map(|(sid, _)| sid.clone())
            .collect()
    }
}

impl Default for UserMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_or_create_profile() {
        let store = UserMemoryStore::new();
        let profile = store.get_or_create("student_01").await;
        assert_eq!(profile.user_id, "student_01");
        assert!(profile.mastery.is_empty());

        // Second call returns same profile
        let profile2 = store.get_or_create("student_01").await;
        assert_eq!(profile2.user_id, "student_01");
    }

    #[tokio::test]
    async fn test_mastery_update() {
        let store = UserMemoryStore::new();

        let p = store.record_learning("s1", "F=ma", true, None).await;
        let m = p.mastery.get("F=ma").unwrap();
        assert_eq!(m.exposures, 1);
        assert_eq!(m.correct_responses, 1);
        assert!(m.level > 0.9); // first correct → near 1.0

        let p = store.record_learning("s1", "F=ma", false, Some("搞混加速度和速度")).await;
        let m = p.mastery.get("F=ma").unwrap();
        assert_eq!(m.exposures, 2);
        assert_eq!(m.correct_responses, 1);
        assert!(!p.misconceptions.is_empty());
    }

    #[tokio::test]
    async fn test_mastery_categories() {
        let store = UserMemoryStore::new();

        // 5 consecutive wrongs → stays at Beginner level (0.2 after each)
        for _ in 0..5 {
            store.record_learning("s1", "微积分", false, None).await;
        }
        let p = store.get("s1").await.unwrap();
        let m = p.mastery.get("微积分").unwrap();
        assert!(m.level < 0.30);
        assert_eq!(m.category(), MasteryCategory::Beginner);

        // 5 correct on another concept → should be developing or mastered
        for _ in 0..5 {
            store.record_learning("s1", "代数", true, None).await;
        }
        let p = store.get("s1").await.unwrap();
        let m = p.mastery.get("代数").unwrap();
        assert!(m.level > 0.60);
        assert!(matches!(m.category(), MasteryCategory::Developing | MasteryCategory::Mastered));
    }

    #[tokio::test]
    async fn test_weakest_concepts() {
        let store = UserMemoryStore::new();
        store.record_learning("s1", "简单概念", true, None).await;
        store.record_learning("s1", "简单概念", true, None).await;
        store.record_learning("s1", "困难概念", false, None).await;
        store.record_learning("s1", "困难概念", false, None).await;

        let p = store.get("s1").await.unwrap();
        let weak = p.weakest_concepts(2);
        assert_eq!(weak[0].concept, "困难概念");
    }

    #[tokio::test]
    async fn test_build_user_context() {
        let store = UserMemoryStore::new();
        let mut p = store.get_or_create("s1").await;
        p.display_name = Some("小明".into());
        p.grade_level = Some("高二".into());

        let mut users = store.users.write().await;
        users.insert("s1".into(), p);
        drop(users);

        store.record_learning("s1", "牛顿力学", true, None).await;
        store.record_learning("s1", "电磁学", false, Some("混淆电场和磁场")).await;

        let p = store.get("s1").await.unwrap();
        let ctx = p.build_user_context();
        assert!(ctx.contains("学生: 小明"));
        assert!(ctx.contains("年级: 高二"));
        assert!(ctx.contains("已掌握: 牛顿力学"));
        assert!(ctx.contains("电磁学"));
        assert!(ctx.contains("混淆电场和磁场"));
    }

    #[tokio::test]
    async fn test_list_users() {
        let store = UserMemoryStore::new();
        store.get_or_create("alice").await;
        store.get_or_create("bob").await;
        store.get_or_create("charlie").await;

        let users = store.list_users().await;
        assert_eq!(users.len(), 3);
    }
}
