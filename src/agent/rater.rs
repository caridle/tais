// Agent Rater — 自主教学质量评分器
//
// 评估教学回合效果 → 多维评分 → 改进建议
// LLM 驱动 + 统计指标计算

use crate::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ── Types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityRating {
    pub proposal_id: String,
    pub concept: String,

    // Core metrics (0.0 - 1.0)
    pub comprehension_score: f64,   // 学生理解度
    pub engagement_score: f64,      // 参与度
    pub relevance_score: f64,       // 内容相关性
    pub strategy_fit: f64,          // 策略匹配度

    // Composite
    pub overall_score: f64,         // 加权综合分
    pub confidence: f64,            // 评分置信度

    // Feedback
    pub summary: String,
    pub strengths: Vec<String>,
    pub weaknesses: Vec<String>,
    pub improvement_suggestions: Vec<ImprovementSuggestion>,

    // Mastery tracking
    pub mastery_before: f64,
    pub mastery_after: f64,
    pub mastery_delta: f64,

    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImprovementSuggestion {
    pub area: String,
    pub suggestion: String,
    pub expected_gain: f64,
    pub difficulty: String, // easy | medium | hard
}

// ── Rater ─────────────────────────────────────────────────────────────

pub struct Rater {
    llm: Arc<llm::LlmRouter>,
}

impl Rater {
    pub fn new(llm: Arc<llm::LlmRouter>) -> Self {
        Self { llm }
    }

    /// Rate the quality of a teaching round
    pub async fn rate(
        &self,
        proposal: &super::proposer::TaskProposal,
        result: &super::consumer::TaskResult,
        _history: &[super::consumer::SessionRecord],
        mastery_before: f64,
    ) -> Result<QualityRating, super::proposer::AgentError> {
        // Compute statistical metrics from output
        let output_confidence = result.output["confidence"].as_f64().unwrap_or(0.7);
        let question_count = result.output["question_count"].as_f64().unwrap_or(1.0);
        let error_rate = result.output["error_rate"].as_f64().unwrap_or(0.0);

        // Rule-based scoring
        let relevance = if result.success { 0.85 } else { 0.2 };
        let strategy_fit = match proposal.strategy.as_str() {
            "scaffold" if proposal.priority == super::proposer::Priority::Critical => 0.9,
            "clarification" => 0.8,
            "drill" => 0.75,
            _ => 0.7,
        };

        // Comprehension proxy: confidence * (1 - error_rate)
        let comprehension = (output_confidence * (1.0 - error_rate)).clamp(0.0, 1.0);

        // Engagement proxy: question_count / 3 (normalize to 0-1)
        let engagement = (question_count / 3.0).min(1.0);

        // Overall weighted score
        let overall = 0.4 * comprehension + 0.25 * engagement + 0.2 * relevance + 0.15 * strategy_fit;

        // Mastery delta estimate
        let mastery_delta = if result.success {
            (comprehension - mastery_before).max(0.0) * 0.15 // capped improvement
        } else {
            -0.02 // slight regression on failure
        };
        let mastery_after = (mastery_before + mastery_delta).clamp(0.0, 1.0);

        // LLM-based feedback (try, fallback to rule-based)
        let (summary, strengths, weaknesses, suggestions) = self
            .generate_feedback(proposal, result, overall)
            .await;

        Ok(QualityRating {
            proposal_id: proposal.id.clone(),
            concept: proposal.concept.clone(),
            comprehension_score: comprehension,
            engagement_score: engagement,
            relevance_score: relevance,
            strategy_fit,
            overall_score: overall,
            confidence: output_confidence,
            summary,
            strengths,
            weaknesses,
            improvement_suggestions: suggestions,
            mastery_before,
            mastery_after,
            mastery_delta,
            timestamp: chrono::Utc::now().to_rfc3339(),
        })
    }

    async fn generate_feedback(
        &self,
        proposal: &super::proposer::TaskProposal,
        result: &super::consumer::TaskResult,
        overall: f64,
    ) -> (String, Vec<String>, Vec<String>, Vec<ImprovementSuggestion>) {
        let system = "你是教学评估专家。用中文总结教学效果、优缺点和改进建议。输出JSON：{\"summary\":...,\"strengths\":[...],\"weaknesses\":[...],\"improvements\":[{\"area\":...,\"suggestion\":...,\"expected_gain\":...,\"difficulty\":...}]}";

        let user = format!(
            "教学任务：概念「{}」，策略「{}」，技能「{}」\n\
             执行成功：{}\n\
             综合评分：{:.0}%\n\
             Agent输出：{}\n\n\
             请评估：",
            proposal.concept, proposal.strategy, proposal.skill,
            result.success, overall * 100.0,
            result.output
        );

        match self.llm.chat_simple(system, &user).await {
            Ok(resp) => {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&resp) {
                    let strengths: Vec<String> = parsed["strengths"]
                        .as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                        .unwrap_or_default();
                    let weaknesses: Vec<String> = parsed["weaknesses"]
                        .as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                        .unwrap_or_default();
                    let improvements: Vec<ImprovementSuggestion> = parsed["improvements"]
                        .as_array().map(|a| a.iter().filter_map(|v| {
                            Some(ImprovementSuggestion {
                                area: v["area"].as_str()?.into(),
                                suggestion: v["suggestion"].as_str()?.into(),
                                expected_gain: v["expected_gain"].as_f64().unwrap_or(0.1),
                                difficulty: v["difficulty"].as_str().unwrap_or("medium").into(),
                            })
                        }).collect())
                        .unwrap_or_default();

                    return (
                        parsed["summary"].as_str().unwrap_or("教学完成").into(),
                        strengths, weaknesses, improvements,
                    );
                }
            }
            Err(_) => {
                tracing::warn!("LLM unavailable for feedback generation — using rule-based fallback");
            }
        }

        // Fallback
        let mut strengths = vec!["策略选择合适".into()];
        let mut weaknesses = vec![];
        let mut suggestions = vec![];

        if overall < 0.5 {
            weaknesses.push("教学效果偏低，需要调整策略".into());
            suggestions.push(ImprovementSuggestion {
                area: "策略调整".into(),
                suggestion: "尝试更换教学策略（如 scaffold → clarification）".into(),
                expected_gain: 0.2,
                difficulty: "medium".into(),
            });
        } else if overall < 0.75 {
            weaknesses.push("仍有提升空间".into());
            suggestions.push(ImprovementSuggestion {
                area: "追问深度".into(),
                suggestion: "增加元认知追问以加深理解".into(),
                expected_gain: 0.1,
                difficulty: "easy".into(),
            });
        } else {
            strengths.push("教学效果良好".into());
        }

        (format!("教学综合评分 {:.0}%", overall * 100.0), strengths, weaknesses, suggestions)
    }
}
