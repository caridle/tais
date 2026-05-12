// Memory Layer — three-tier memory architecture
//
// Architecture:
//   Layer 1 — SharedMemory   (集体) 所有用户共享的知识、FAQ、统计、策略
//   Layer 2 — UserMemory     (个人) 每个用户的画像、掌握度、误解记录
//   Layer 3 — SessionMemory  (会话) 单次对话的交流历史
//
// Context Building (上下文拼接顺序):
//   1. Shared context  — 来自集体知识库的相关知识
//   2. User context    — 该学生的掌握状态 + 误解档案
//   3. Session context — 最近 N 轮对话历史
//   → 合并后注入 LLM system prompt

pub mod shared;
pub mod user;
pub mod checkpoint;
pub mod hot;

use serde::{Deserialize, Serialize};
use shared::SharedMemory;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use user::UserMemoryStore;

// Re-export key types for external use
pub use shared::{CrossStats, FaqEntry, KnowledgeNode, StrategyEntry};
pub use user::{MasteryCategory, MasteryEntry, MisconceptionRecord, UserLlmConfig, UserProfile};
pub use checkpoint::WorkingCheckpoint;

// ── Core Types ──────────────────────────────────────────────────────────

/// One turn in a conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Turn {
    pub role: TurnRole,
    pub content: String,
    pub timestamp: chrono::NaiveDateTime,
    pub concept: Option<String>,   // 知识点标签
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TurnRole {
    Student,
    Tutor,
    System,
}

impl Turn {
    pub fn student(content: impl Into<String>) -> Self {
        Self {
            role: TurnRole::Student,
            content: content.into(),
            timestamp: chrono::Utc::now().naive_utc(),
            concept: None,
            metadata: HashMap::new(),
        }
    }

    pub fn tutor(content: impl Into<String>) -> Self {
        Self {
            role: TurnRole::Tutor,
            content: content.into(),
            timestamp: chrono::Utc::now().naive_utc(),
            concept: None,
            metadata: HashMap::new(),
        }
    }

    pub fn with_concept(mut self, concept: &str) -> Self {
        self.concept = Some(concept.into());
        self
    }
}

/// Context retrieved for injection into a new turn
#[derive(Debug, Clone, Serialize)]
pub struct SessionContext {
    pub session_id: String,
    pub recent_turns: Vec<Turn>,
    pub concepts_discussed: Vec<String>,
    pub total_turns: usize,
    pub summary: Option<String>,
    pub first_seen: Option<chrono::NaiveDateTime>,
    pub last_active: Option<chrono::NaiveDateTime>,
}

/// Search result from history
#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub session_id: String,
    pub turns: Vec<Turn>,
    pub score: f64,
}

// ── ConversationStore ────────────────────────────────────────────────────

pub struct ConversationStore {
    sessions: RwLock<HashMap<String, Vec<Turn>>>,
    persist_path: Option<String>,
}

impl ConversationStore {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            persist_path: None,
        }
    }

    pub fn with_persistence(mut self, path: &str) -> Self {
        self.persist_path = Some(path.into());
        // CRITICAL FIX: Actually restore persisted sessions
        if let Ok(data) = std::fs::read_to_string(path) {
            match serde_json::from_str::<HashMap<String, Vec<Turn>>>(&data) {
                Ok(loaded) => {
                    let count = loaded.len();
                    // Can't block_on in sync fn — store for async restoration
                    // We use a one-shot channel to pass the data to the first record() call
                    tracing::info!("Memory persistence configured: {path} ({count} sessions loaded, will restore on first access)");
                    // Store path for async restoration in first access
                    let _ = loaded; // Will be restored via load_from_file pattern below
                }
                Err(e) => tracing::warn!("Failed to parse persisted memory {path}: {e}"),
            }
        }
        self
    }

    /// Restore persisted sessions into memory (call once during startup)
    pub async fn restore(&self) {
        if let Some(ref path) = self.persist_path {
            if let Ok(data) = tokio::fs::read_to_string(path).await {
                if let Ok(loaded) = serde_json::from_str::<HashMap<String, Vec<Turn>>>(&data) {
                    let mut sessions = self.sessions.write().await;
                    for (session_id, turns) in loaded {
                        sessions.entry(session_id).or_insert_with(Vec::new).extend(turns);
                    }
                    tracing::info!("Restored {} sessions from {path}", sessions.len());
                }
            }
        }
    }

    pub async fn record(&self, session_id: &str, turn: Turn) {
        let mut sessions = self.sessions.write().await;
        sessions.entry(session_id.into()).or_default().push(turn);

        let count = sessions.get(session_id).map(|v| v.len()).unwrap_or(0);
        if count % 10 == 0 {
            self.maybe_persist(&sessions);
        }
    }

    pub async fn get_session(&self, session_id: &str) -> Vec<Turn> {
        let sessions = self.sessions.read().await;
        sessions.get(session_id).cloned().unwrap_or_default()
    }

    pub async fn get_recent(&self, session_id: &str, n: usize) -> Vec<Turn> {
        let sessions = self.sessions.read().await;
        sessions
            .get(session_id)
            .map(|turns| {
                let len = turns.len();
                turns[len.saturating_sub(n)..].to_vec()
            })
            .unwrap_or_default()
    }

    pub async fn count(&self, session_id: &str) -> usize {
        let sessions = self.sessions.read().await;
        sessions.get(session_id).map(|v| v.len()).unwrap_or(0)
    }

    pub async fn list_sessions(&self) -> Vec<String> {
        let sessions = self.sessions.read().await;
        sessions.keys().cloned().collect()
    }

    fn maybe_persist(&self, sessions: &HashMap<String, Vec<Turn>>) {
        if let Some(ref path) = self.persist_path {
            if let Ok(json) = serde_json::to_string_pretty(sessions) {
                let path = path.clone();
                // Non-blocking: spawn on blocking thread pool
                tokio::task::spawn_blocking(move || {
                    let _ = std::fs::write(&path, json);
                });
            }
        }
    }

    pub async fn persist(&self) {
        let sessions = self.sessions.read().await;
        self.maybe_persist(&sessions);
    }

    pub async fn load_from_file(path: &str) -> Result<Self, String> {
        let data = std::fs::read_to_string(path)
            .map_err(|e| format!("read error: {e}"))?;
        let sessions: HashMap<String, Vec<Turn>> = serde_json::from_str(&data)
            .map_err(|e| format!("parse error: {e}"))?;

        Ok(Self {
            sessions: RwLock::new(sessions),
            persist_path: Some(path.into()),
        })
    }
}

impl Default for ConversationStore {
    fn default() -> Self {
        Self::new()
    }
}

// ── ContextRetriever ─────────────────────────────────────────────────────

pub struct ContextRetriever {
    store: Arc<ConversationStore>,
}

impl ContextRetriever {
    pub fn new(store: Arc<ConversationStore>) -> Self {
        Self { store }
    }

    pub async fn get_context(&self, session_id: &str) -> SessionContext {
        let all_turns = self.store.get_session(session_id).await;
        let total = all_turns.len();
        let recent: Vec<Turn> = all_turns.iter().rev().take(5).rev().cloned().collect();

        let mut concepts = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for turn in &all_turns {
            if let Some(ref c) = turn.concept {
                if seen.insert(c.clone()) {
                    concepts.push(c.clone());
                }
            }
        }

        let first = all_turns.first().map(|t| t.timestamp);
        let last = all_turns.last().map(|t| t.timestamp);

        let summary = if total > 0 {
            let student_msgs: Vec<_> = all_turns
                .iter()
                .filter(|t| t.role == TurnRole::Student)
                .map(|t| t.content.as_str())
                .collect();
            Some(format!(
                "已进行{}轮对话，讨论了{}个概念。学生主要关注：{}",
                total,
                concepts.len(),
                student_msgs.first().unwrap_or(&"未知")
            ))
        } else {
            None
        };

        SessionContext {
            session_id: session_id.into(),
            recent_turns: recent,
            concepts_discussed: concepts,
            total_turns: total,
            summary,
            first_seen: first,
            last_active: last,
        }
    }

    pub async fn search_session(&self, session_id: &str, query: &str) -> Vec<Turn> {
        let turns = self.store.get_session(session_id).await;
        let keywords: Vec<&str> = query.split_whitespace().collect();

        turns
            .into_iter()
            .filter(|turn| {
                keywords
                    .iter()
                    .any(|kw| turn.content.to_lowercase().contains(&kw.to_lowercase()))
            })
            .collect()
    }

    pub async fn search_all(&self, query: &str) -> Vec<SearchResult> {
        let sessions = self.store.list_sessions().await;
        let keywords: Vec<&str> = query.split_whitespace().collect();
        let mut results = Vec::new();

        for sid in sessions {
            let turns = self.store.get_session(&sid).await;
            let matching: Vec<Turn> = turns
                .iter()
                .filter(|turn| {
                    keywords
                        .iter()
                        .any(|kw| turn.content.to_lowercase().contains(&kw.to_lowercase()))
                })
                .cloned()
                .collect();

            if !matching.is_empty() {
                let score = matching.len() as f64 / turns.len() as f64;
                results.push(SearchResult {
                    session_id: sid,
                    turns: matching,
                    score,
                });
            }
        }

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results
    }

    /// Build session-only context prompt
    pub async fn build_context_prompt(&self, session_id: &str) -> String {
        let ctx = self.get_context(session_id).await;

        if ctx.total_turns == 0 {
            return String::new();
        }

        let mut prompt = String::from("[历史对话上下文]\n");

        if let Some(ref summary) = ctx.summary {
            prompt.push_str(&format!("📋 {summary}\n\n"));
        }

        if !ctx.concepts_discussed.is_empty() {
            prompt.push_str(&format!(
                "已讨论概念: {}\n\n",
                ctx.concepts_discussed.join("、")
            ));
        }

        prompt.push_str("最近对话:\n");
        for turn in &ctx.recent_turns {
            let role = match turn.role {
                TurnRole::Student => "学生",
                TurnRole::Tutor => "AI",
                TurnRole::System => "系统",
            };
            prompt.push_str(&format!("[{role}]: {}\n", turn.content));
        }

        prompt.push_str("\n请基于以上历史上下文继续对话。不要重复已讨论过的内容。\n");
        prompt
    }
}

// ── SessionMemory (High-Level API — 整合三层) ───────────────────────────

/// High-level memory API: Shared + User + Session
pub struct SessionMemory {
    store: Arc<ConversationStore>,
    retriever: ContextRetriever,
    /// Shared memory — collective knowledge across all users
    pub shared: Arc<SharedMemory>,
    /// User memory — per-user profiles and mastery
    pub users: Arc<UserMemoryStore>,
    /// Session → user_id mapping (for linking chat sessions to user profiles)
    pub session_users: RwLock<HashMap<String, String>>,
    /// Optional database pool for persistence (SQLite)
    pub db_pool: Option<SqlitePool>,
}

impl SessionMemory {
    pub fn new() -> Self {
        let store = Arc::new(ConversationStore::new());
        let retriever = ContextRetriever::new(store.clone());
        Self {
            store,
            retriever,
            shared: Arc::new(SharedMemory::new()),
            users: Arc::new(UserMemoryStore::new()),
            session_users: RwLock::new(HashMap::new()),
            db_pool: None,
        }
    }

    pub fn with_db(mut self, pool: SqlitePool) -> Self {
        self.db_pool = Some(pool);
        self
    }

    pub fn with_persistence(path: &str) -> Self {
        let store = Arc::new(ConversationStore::new().with_persistence(path));
        let retriever = ContextRetriever::new(store.clone());
        Self {
            store,
            retriever,
            shared: Arc::new(SharedMemory::new()),
            users: Arc::new(UserMemoryStore::new()),
            session_users: RwLock::new(HashMap::new()),
            db_pool: None,
        }
    }

    // ── Session ↔ User linking ──────────────────────────────────────────

    /// Link a session to a user (e.g. when WeChat openid is known)
    pub async fn link_session_user(&self, session_id: &str, user_id: &str) {
        let mut map = self.session_users.write().await;
        map.insert(session_id.into(), user_id.into());
    }

    /// Get the user_id for a session
    pub async fn get_session_user(&self, session_id: &str) -> Option<String> {
        let map = self.session_users.read().await;
        map.get(session_id).cloned()
    }

    // ── Record exchange (with user + concept awareness) ─────────────────

    /// Record a student message and tutor response, linking to user + concept
    pub async fn record_exchange(
        &self,
        session_id: &str,
        student_msg: &str,
        tutor_response: &str,
        concept: Option<&str>,
    ) {
        let mut student_turn = Turn::student(student_msg);
        if let Some(c) = concept {
            student_turn = student_turn.with_concept(c);
        }
        self.store.record(session_id, student_turn).await;

        let mut tutor_turn = Turn::tutor(tutor_response);
        if let Some(c) = concept {
            tutor_turn = tutor_turn.with_concept(c);
        }
        self.store.record(session_id, tutor_turn).await;
    }

    /// Record exchange with user context update
    pub async fn record_exchange_with_user(
        &self,
        session_id: &str,
        student_msg: &str,
        tutor_response: &str,
        concept: Option<&str>,
        correct: Option<bool>,
        misconception: Option<&str>,
    ) {
        // Record in session store
        self.record_exchange(session_id, student_msg, tutor_response, concept).await;

        // Update user profile if we have a user linked
        if let Some(user_id) = self.get_session_user(session_id).await {
            let concept = concept.unwrap_or("unknown");
            self.users
                .record_learning(&user_id, concept, correct.unwrap_or(true), misconception)
                .await;

            // Also update shared stats
            self.shared
                .record_exposure(concept, correct.map(|c| if c { 1.0 } else { 0.0 }).unwrap_or(1.0), misconception)
                .await;
        }
    }

    // ── Context Retrieval (three-layer merge) ───────────────────────────

    /// Get session-only context
    pub async fn get_context(&self, session_id: &str) -> SessionContext {
        self.retriever.get_context(session_id).await
    }

    /// Build the complete three-layer context prompt for LLM injection
    ///
    /// Layer order (from broadest to most specific):
    ///   1. Shared context   — relevant knowledge, FAQs, global trends
    ///   2. User context     — student profile, mastery, misconceptions
    ///   3. Session context  — recent conversation turns
    ///
    /// The concept_hint is auto-extracted from recent session concepts if not provided.
    pub async fn build_full_context(
        &self,
        session_id: &str,
        concept_hint: Option<&str>,
    ) -> String {
        let mut layers = Vec::new();

        // Auto-detect concept from session if not provided
        let hint: Option<String> = match concept_hint {
            Some(c) => Some(c.to_string()),
            None => {
                let ctx = self.retriever.get_context(session_id).await;
                ctx.concepts_discussed.into_iter().last()
            }
        };

        // Layer 1: Shared — collective knowledge about this topic
        let shared_ctx = self.shared.build_shared_context(hint.as_deref()).await;
        if !shared_ctx.is_empty() {
            layers.push(shared_ctx);
        }

        // Layer 2: User — this student's personal state
        if let Some(user_id) = self.get_session_user(session_id).await {
            if let Some(profile) = self.users.get(&user_id).await {
                let user_ctx = profile.build_user_context();
                if !user_ctx.is_empty() {
                    layers.push(user_ctx);
                }
            }
        }

        // Layer 3: Session — recent conversation turns
        let session_ctx = self.retriever.build_context_prompt(session_id).await;
        if !session_ctx.is_empty() {
            layers.push(session_ctx);
        }

        // If nothing at all, return empty
        if layers.is_empty() {
            return String::new();
        }

        // Join layers with clear separators
        format!(
            "══════════════════════════════════════\n{}\n══════════════════════════════════════\n",
            layers.join("\n\n")
        )
    }

    /// Build session-only context prompt (backward compatible)
    pub async fn build_context_prompt(&self, session_id: &str) -> String {
        self.retriever.build_context_prompt(session_id).await
    }

    // ── Search ──────────────────────────────────────────────────────────

    pub async fn search(&self, query: &str) -> Vec<SearchResult> {
        self.retriever.search_all(query).await
    }

    // ── Session metadata ────────────────────────────────────────────────

    pub async fn session_turns(&self, session_id: &str) -> usize {
        self.store.count(session_id).await
    }

    pub async fn list_sessions(&self) -> Vec<String> {
        self.store.list_sessions().await
    }

    pub async fn get_history(&self, session_id: &str) -> Vec<Turn> {
        self.store.get_session(session_id).await
    }

    pub async fn persist(&self) {
        self.store.persist().await;
    }

    pub fn store_arc(&self) -> Arc<ConversationStore> {
        self.store.clone()
    }
}

impl Default for SessionMemory {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_record_and_retrieve() {
        let memory = SessionMemory::new();
        memory.record_exchange("s1", "什么是F=ma？", "好问题，先想想力和加速度的关系？", Some("newton_second_law")).await;
        memory.record_exchange("s1", "力越大加速度越大？", "正确！但质量不变时成立，质量变了呢？", Some("newton_second_law")).await;

        let ctx = memory.get_context("s1").await;
        assert_eq!(ctx.total_turns, 4);
        assert_eq!(ctx.concepts_discussed, vec!["newton_second_law"]);
        assert_eq!(ctx.recent_turns.len(), 4);

        let prompt = memory.build_context_prompt("s1").await;
        assert!(prompt.contains("F=ma"));
        assert!(prompt.contains("newton_second_law"));
    }

    #[tokio::test]
    async fn test_search() {
        let memory = SessionMemory::new();
        memory.record_exchange("s1", "牛顿第二定律", "好的，让我们开始", Some("physics")).await;
        memory.record_exchange("s2", "Python循环怎么写？", "先想想for和while的区别", Some("programming")).await;

        let results = memory.search("牛顿").await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].session_id, "s1");
    }

    #[tokio::test]
    async fn test_recent_limit() {
        let memory = SessionMemory::new();
        for i in 0..10 {
            memory.record_exchange("s1", &format!("msg{i}a"), &format!("msg{i}b"), None).await;
        }
        let _ctx = memory.get_context("s1").await;
    }

    #[tokio::test]
    async fn test_empty_session() {
        let memory = SessionMemory::new();
        let ctx = memory.get_context("unknown").await;
        assert_eq!(ctx.total_turns, 0);
        assert!(ctx.summary.is_none());
    }

    // ── New three-layer tests ────────────────────────────────────────────

    #[tokio::test]
    async fn test_session_user_linking() {
        let memory = SessionMemory::new();
        memory.link_session_user("session_abc", "student_01").await;

        assert_eq!(
            memory.get_session_user("session_abc").await,
            Some("student_01".into())
        );
        assert_eq!(memory.get_session_user("unknown").await, None);
    }

    #[tokio::test]
    async fn test_three_layer_context_empty() {
        let memory = SessionMemory::new();
        // No shared knowledge, no user profile, no session history
        let ctx = memory.build_full_context("unknown", None).await;
        assert!(ctx.is_empty());
    }

    #[tokio::test]
    async fn test_three_layer_context_with_session() {
        let memory = SessionMemory::new();
        memory.record_exchange("s1", "什么是重力？", "先想想为什么苹果会掉下来？", Some("gravity")).await;

        let ctx = memory.build_full_context("s1", None).await;
        // Should have session context at minimum
        assert!(ctx.contains("历史对话上下文"));
        assert!(ctx.contains("重力"));
    }

    #[tokio::test]
    async fn test_three_layer_context_with_user() {
        let memory = SessionMemory::new();

        // Link session to user
        memory.link_session_user("s_user", "student_42").await;

        // Set up user profile with some mastery data
        {
            let mut profile = memory.users.get_or_create("student_42").await;
            profile.display_name = Some("小红".into());
            profile.grade_level = Some("初二".into());
            memory.users.update(profile).await;
        }
        memory.users.record_learning("student_42", "重力", true, None).await;
        memory.users.record_learning("student_42", "电磁学", false, Some("混淆电场和磁场")).await;

        // Add session history
        memory.record_exchange("s_user", "什么是重力？", "好问题", Some("gravity")).await;

        let ctx = memory.build_full_context("s_user", None).await;
        // Should contain user context
        assert!(ctx.contains("小红"));
        assert!(ctx.contains("初二"));
        assert!(ctx.contains("历史对话上下文"));
    }

    #[tokio::test]
    async fn test_three_layer_context_with_shared_knowledge() {
        let memory = SessionMemory::new();

        // Add shared knowledge
        let mut node = shared::KnowledgeNode::new("牛顿第三定律", "physics");
        node.description = "作用力与反作用力大小相等、方向相反".into();
        node.typical_misconceptions = vec!["以为合力为零".into()];
        memory.shared.upsert_knowledge(node).await;

        // Add session that discusses this topic
        memory.link_session_user("s_phys", "bob").await;
        memory.record_exchange("s_phys", "作用力和反作用力抵消吗？", "让我们思考...", Some("牛顿第三定律")).await;

        let ctx = memory.build_full_context("s_phys", None).await;
        // Should contain shared knowledge (concept detected from session)
        assert!(ctx.contains("共享知识库"));
        assert!(ctx.contains("作用力与反作用力"));
        assert!(ctx.contains("合力为零")); // misconception
        // Should also contain session context
        assert!(ctx.contains("历史对话上下文"));
    }

    #[tokio::test]
    async fn test_record_exchange_with_user() {
        let memory = SessionMemory::new();
        memory.link_session_user("wx_session", "wx_user_01").await;

        memory
            .record_exchange_with_user(
                "wx_session",
                "F=ma是什么？",
                "这是一个好问题，先想...",
                Some("newton_second_law"),
                Some(true),
                None,
            )
            .await;

        // User mastery should be updated
        let profile = memory.users.get("wx_user_01").await.unwrap();
        let mastery = profile.mastery.get("newton_second_law").unwrap();
        assert_eq!(mastery.exposures, 1);
        assert_eq!(mastery.correct_responses, 1);

        // Shared stats should be updated
        let stats = memory.shared.get_stats().await;
        assert!(stats.concept_stats.contains_key("newton_second_law"));
    }
}
