// Shared Memory — collective knowledge accessible to all users
//
// Architecture:
//   KnowledgeGraph  — concept → dependencies, misconceptions, explanations
//   FaqBank          — question → answer templates, ranked by effectiveness
//   CrossStats       — aggregate performance across all students
//   StrategyPool     — proven teaching strategies from evolution
//
// Teacher ↔ Student 共享逻辑：
//   - Teacher 添加知识点、FAQ、策略到共享记忆
//   - Student 对话时自动检索相关共享知识，注入上下文
//   - 跨学生统计数据帮助 TAIS 识别全局难点

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

// ── Knowledge Node (知识点) ─────────────────────────────────────────────

/// A knowledge node in the shared concept graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeNode {
    pub concept: String,
    pub domain: String,                      // 学科领域: physics, math, cs...
    pub description: String,
    pub prerequisites: Vec<String>,          // 前置知识点
    pub typical_misconceptions: Vec<String>, // 典型误解
    pub best_explanation: String,            // 最佳解释模板
    pub probing_questions: Vec<String>,      // 苏格拉底式追问列表
    pub difficulty: f64,                     // 0-1 难度系数（从统计数据更新）
    pub created_by: Option<String>,          // 谁添加的（teacher id）
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

impl KnowledgeNode {
    pub fn new(concept: &str, domain: &str) -> Self {
        let now = chrono::Utc::now().naive_utc();
        Self {
            concept: concept.into(),
            domain: domain.into(),
            description: String::new(),
            prerequisites: vec![],
            typical_misconceptions: vec![],
            best_explanation: String::new(),
            probing_questions: vec![],
            difficulty: 0.5,
            created_by: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Find the best probing question given a student's query keywords
    pub fn best_probe(&self, _keywords: &[&str]) -> Option<&str> {
        self.probing_questions.first().map(|s| s.as_str())
    }
}

// ── FAQ Entry ───────────────────────────────────────────────────────────

/// A frequently-asked question with proven answer template
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaqEntry {
    pub question: String,
    pub answer_template: String,
    pub domain: String,
    pub usage_count: u32,
    pub effectiveness: f64,          // 0-1: how often students understand after this answer
    pub related_concepts: Vec<String>,
    pub created_at: chrono::NaiveDateTime,
}

// ── Cross-Student Stats ─────────────────────────────────────────────────

/// Aggregated statistics across all students for a concept
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConceptStats {
    pub concept: String,
    pub total_exposures: u32,        // 总曝光次数
    pub avg_mastery: f64,            // 平均掌握度 0-1
    pub common_errors: Vec<(String, u32)>, // (错误描述, 出现次数)
    pub confusion_rate: f64,         // 混淆率 — 高 = 需要更多教学资源
}

/// Global cross-student statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossStats {
    pub total_sessions: u32,
    pub total_students: u32,
    pub concept_stats: HashMap<String, ConceptStats>,
    pub most_difficult: Vec<String>,   // 最难掌握的概念 top N
    pub updated_at: chrono::NaiveDateTime,
}

// ── Strategy Entry ──────────────────────────────────────────────────────

/// A teaching strategy proven effective across students
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub applicable_concepts: Vec<String>,
    pub success_rate: f64,           // 0-1
    pub times_used: u32,
    pub created_at: chrono::NaiveDateTime,
}

// ── SharedMemory Store ──────────────────────────────────────────────────

/// Shared memory store — thread-safe, all users can read, teachers can write
pub struct SharedMemory {
    knowledge: RwLock<HashMap<String, KnowledgeNode>>,
    faqs: RwLock<Vec<FaqEntry>>,
    stats: RwLock<CrossStats>,
    strategies: RwLock<HashMap<String, StrategyEntry>>,
    persist_path: Option<String>,
}

impl SharedMemory {
    pub fn new() -> Self {
        Self {
            knowledge: RwLock::new(HashMap::new()),
            faqs: RwLock::new(Vec::new()),
            stats: RwLock::new(CrossStats {
                total_sessions: 0,
                total_students: 0,
                concept_stats: HashMap::new(),
                most_difficult: vec![],
                updated_at: chrono::Utc::now().naive_utc(),
            }),
            strategies: RwLock::new(HashMap::new()),
            persist_path: None,
        }
    }

    pub fn with_persistence(mut self, path: &str) -> Self {
        self.persist_path = Some(path.into());
        self
    }

    // ── Knowledge Operations ────────────────────────────────────────────

    /// Add or update a knowledge node
    pub async fn upsert_knowledge(&self, node: KnowledgeNode) {
        let mut k = self.knowledge.write().await;
        k.insert(node.concept.clone(), node);
    }

    /// Get a knowledge node by concept name
    pub async fn get_knowledge(&self, concept: &str) -> Option<KnowledgeNode> {
        let k = self.knowledge.read().await;
        k.get(concept).cloned()
    }

    /// Search knowledge by keyword (fuzzy match on concept + description)
    pub async fn search_knowledge(&self, query: &str) -> Vec<KnowledgeNode> {
        let k = self.knowledge.read().await;
        let q = query.to_lowercase();
        let mut results: Vec<_> = k
            .values()
            .filter(|n| {
                n.concept.to_lowercase().contains(&q)
                    || n.description.to_lowercase().contains(&q)
                    || n.domain.to_lowercase().contains(&q)
            })
            .cloned()
            .collect();
        results.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        results
    }

    /// List all knowledge nodes
    pub async fn list_knowledge(&self) -> Vec<KnowledgeNode> {
        let k = self.knowledge.read().await;
        k.values().cloned().collect()
    }

    // ── FAQ Operations ──────────────────────────────────────────────────

    /// Add a new FAQ entry
    pub async fn add_faq(&self, entry: FaqEntry) {
        let mut f = self.faqs.write().await;
        f.push(entry);
    }

    /// Search FAQs by keyword
    pub async fn search_faqs(&self, query: &str) -> Vec<FaqEntry> {
        let f = self.faqs.read().await;
        let q = query.to_lowercase();
        let mut results: Vec<_> = f
            .iter()
            .filter(|e| {
                e.question.to_lowercase().contains(&q)
                    || e.answer_template.to_lowercase().contains(&q)
            })
            .cloned()
            .collect();
        // Sort by effectiveness (most effective first)
        results.sort_by(|a, b| b.effectiveness.partial_cmp(&a.effectiveness).unwrap_or(std::cmp::Ordering::Equal));
        results
    }

    /// Get top FAQs
    pub async fn top_faqs(&self, n: usize) -> Vec<FaqEntry> {
        let f = self.faqs.read().await;
        let mut sorted = f.clone();
        sorted.sort_by(|a, b| b.usage_count.cmp(&a.usage_count));
        sorted.truncate(n);
        sorted
    }

    // ── Stats Operations ────────────────────────────────────────────────

    /// Update concept stats incrementally
    pub async fn record_exposure(&self, concept: &str, mastery: f64, error_desc: Option<&str>) {
        let mut s = self.stats.write().await;
        let entry = s.concept_stats.entry(concept.into()).or_insert_with(|| ConceptStats {
            concept: concept.into(),
            total_exposures: 0,
            avg_mastery: 0.0,
            common_errors: vec![],
            confusion_rate: 0.0,
        });

        // Update running average
        let n = entry.total_exposures as f64;
        entry.avg_mastery = (entry.avg_mastery * n + mastery) / (n + 1.0);
        entry.total_exposures += 1;

        // Record error
        if let Some(err) = error_desc {
            if let Some(found) = entry.common_errors.iter_mut().find(|(e, _)| e == err) {
                found.1 += 1;
            } else {
                entry.common_errors.push((err.into(), 1));
            }
            entry.confusion_rate = entry.common_errors.iter().map(|(_, c)| *c).sum::<u32>() as f64
                / entry.total_exposures as f64;
        }

        s.total_sessions += 1;
        s.updated_at = chrono::Utc::now().naive_utc();

        // Update most_difficult
        let mut concepts: Vec<_> = s.concept_stats.iter().collect();
        concepts.sort_by(|a, b| b.1.confusion_rate.partial_cmp(&a.1.confusion_rate).unwrap_or(std::cmp::Ordering::Equal));
        s.most_difficult = concepts.iter().take(10).map(|(name, _)| name.to_string()).collect::<Vec<_>>();
    }

    /// Get current cross-stats
    pub async fn get_stats(&self) -> CrossStats {
        self.stats.read().await.clone()
    }

    // ── Strategy Operations ─────────────────────────────────────────────

    /// Add a proven strategy
    pub async fn add_strategy(&self, entry: StrategyEntry) {
        let mut s = self.strategies.write().await;
        s.insert(entry.id.clone(), entry);
    }

    /// Find strategies applicable to a concept
    pub async fn find_strategies(&self, concept: &str) -> Vec<StrategyEntry> {
        let s = self.strategies.read().await;
        s.values()
            .filter(|e| e.applicable_concepts.iter().any(|c| c == concept))
            .cloned()
            .collect()
    }

    /// Get top strategies by success rate
    pub async fn top_strategies(&self, n: usize) -> Vec<StrategyEntry> {
        let s = self.strategies.read().await;
        let mut sorted: Vec<_> = s.values().cloned().collect();
        sorted.sort_by(|a, b| b.success_rate.partial_cmp(&a.success_rate).unwrap_or(std::cmp::Ordering::Equal));
        sorted.truncate(n);
        sorted
    }

    // ── Shared Context Builder ──────────────────────────────────────────

    /// Build a shared-knowledge context prompt for injection into LLM
    /// This is the "common memory" that all students get
    pub async fn build_shared_context(&self, concept_hint: Option<&str>) -> String {
        let mut parts = Vec::new();

        // 1. Relevant knowledge nodes
        if let Some(hint) = concept_hint {
            let knowledge = self.search_knowledge(hint).await;
            if !knowledge.is_empty() {
                parts.push("【共享知识库】".to_string());
                for k in &knowledge {
                    parts.push(format!(
                        "• {}: {}",
                        k.concept, k.description
                    ));
                    if !k.typical_misconceptions.is_empty() {
                        parts.push(format!(
                            "  常见误解: {}",
                            k.typical_misconceptions.join("、")
                        ));
                    }
                }
                parts.push(String::new()); // blank line
            }
        }

        // 2. Top FAQs related to this topic
        if let Some(hint) = concept_hint {
            let faqs = self.search_faqs(hint).await;
            if !faqs.is_empty() {
                parts.push("【高频问答】".to_string());
                for f in faqs.iter().take(3) {
                    parts.push(format!("  Q: {}", f.question));
                }
                parts.push(String::new());
            }
        }

        // 3. Cross-student trends
        let stats = self.stats.read().await;
        if !stats.most_difficult.is_empty() {
            parts.push(format!(
                "【全局难点】最近学生普遍在以下概念遇到困难: {}",
                stats.most_difficult.iter().take(5).cloned().collect::<Vec<_>>().join("、")
            ));
            parts.push(String::new());
        }

        if parts.is_empty() {
            String::new()
        } else {
            parts.join("\n")
        }
    }
}

impl Default for SharedMemory {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_upsert_and_search_knowledge() {
        let sm = SharedMemory::new();

        let mut node = KnowledgeNode::new("牛顿第二定律", "physics");
        node.description = "F=ma，力等于质量乘以加速度".into();
        node.typical_misconceptions = vec!["认为力是维持运动的原因".into()];
        sm.upsert_knowledge(node).await;

        let results = sm.search_knowledge("牛顿").await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].concept, "牛顿第二定律");
    }

    #[tokio::test]
    async fn test_faq_search_and_rank() {
        let sm = SharedMemory::new();

        sm.add_faq(FaqEntry {
            question: "什么是惯性?".into(),
            answer_template: "惯性是...".into(),
            domain: "physics".into(),
            usage_count: 100,
            effectiveness: 0.9,
            related_concepts: vec!["牛顿第一定律".into()],
            created_at: chrono::Utc::now().naive_utc(),
        }).await;

        sm.add_faq(FaqEntry {
            question: "Python 循环怎么写?".into(),
            answer_template: "for 和 while...".into(),
            domain: "cs".into(),
            usage_count: 50,
            effectiveness: 0.7,
            related_concepts: vec![],
            created_at: chrono::Utc::now().naive_utc(),
        }).await;

        let results = sm.search_faqs("惯性").await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].question, "什么是惯性?");

        let top = sm.top_faqs(1).await;
        assert_eq!(top[0].usage_count, 100);
    }

    #[tokio::test]
    async fn test_record_and_get_stats() {
        let sm = SharedMemory::new();

        sm.record_exposure("F=ma", 0.8, Some("搞混加速度和速度")).await;
        sm.record_exposure("F=ma", 0.3, Some("搞混加速度和速度")).await;
        sm.record_exposure("F=ma", 0.9, None).await;

        let stats = sm.get_stats().await;
        assert_eq!(stats.total_sessions, 3);
        assert!(stats.concept_stats.contains_key("F=ma"));
        assert_eq!(stats.concept_stats["F=ma"].common_errors.len(), 1);
        assert_eq!(stats.concept_stats["F=ma"].common_errors[0].1, 2); // appeared twice
    }

    #[tokio::test]
    async fn test_strategies() {
        let sm = SharedMemory::new();

        sm.add_strategy(StrategyEntry {
            id: "s1".into(),
            name: "类比法".into(),
            description: "用水流类比电流".into(),
            applicable_concepts: vec!["电流".into()],
            success_rate: 0.85,
            times_used: 12,
            created_at: chrono::Utc::now().naive_utc(),
        }).await;

        let found = sm.find_strategies("电流").await;
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].success_rate, 0.85);

        // No strategies for unrelated concept
        let none = sm.find_strategies("微积分").await;
        assert!(none.is_empty());
    }

    #[tokio::test]
    async fn test_build_shared_context() {
        let sm = SharedMemory::new();

        let mut node = KnowledgeNode::new("牛顿第三定律", "physics");
        node.description = "作用力与反作用力".into();
        node.typical_misconceptions = vec!["以为合力为零".into()];
        sm.upsert_knowledge(node).await;

        sm.record_exposure("牛顿第三定律", 0.4, Some("混淆作用力和合力")).await;

        let ctx = sm.build_shared_context(Some("牛顿")).await;
        assert!(ctx.contains("共享知识库"));
        assert!(ctx.contains("作用力与反作用力"));
        assert!(ctx.contains("全局难点"));
    }
}
