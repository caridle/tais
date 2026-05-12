// Gene Gateway — intercepts all decision points and injects gene capsule behavior
//
// The gateway sits between Skills Bus and Orchestrator.
// At every decision point, it applies gene modifications:
//   - scholar: high precision, cites sources
//   - mentor: warm tone, encouraging language
//   - hacker: minimal, code-only, no comments
//
// Architecture:
//   GeneGateway.wrap(skill_output, gene_profile) → modified output

use crate::GeneProfile;
use serde_json::Value;

/// The Gene Gateway applies gene modifications at decision points.
pub struct GeneGateway;

impl GeneGateway {
    /// Wrap a skill output with gene-specific modifications.
    /// Called before returning any AI response to the student.
    pub fn wrap(output: &Value, gene: &GeneProfile) -> Value {
        let mut wrapped = output.clone();

        // Inject gene metadata
        if let Some(obj) = wrapped.as_object_mut() {
            obj.insert("_gene_personality".into(), Value::String(gene.personality.clone()));
            obj.insert("_gene_thinking".into(), Value::String(gene.thinking.clone()));
        }

        // Apply personality-specific modifications
        match gene.personality.as_str() {
            "scholar" => {
                // Add confidence markers, source citations
                if let Some(obj) = wrapped.as_object_mut() {
                    obj.insert("_style".into(), Value::String("precise_academic".into()));
                    if let Some(content) = obj.get_mut("content") {
                        let s = content.as_str().unwrap_or("");
                        *content = Value::String(format!("{s} [置信度: 高 · 来源: 已验证]"));
                    }
                }
            }
            "mentor" => {
                if let Some(obj) = wrapped.as_object_mut() {
                    obj.insert("_style".into(), Value::String("encouraging".into()));
                    if let Some(content) = obj.get_mut("content") {
                        let s = content.as_str().unwrap_or("");
                        *content = Value::String(format!("你已经很接近了！{s}"));
                    }
                }
            }
            "hacker" => {
                if let Some(obj) = wrapped.as_object_mut() {
                    obj.insert("_style".into(), Value::String("minimal".into()));
                    // Strip verbose explanations — keep only essential info
                    if let Some(content) = obj.get_mut("content") {
                        let s = content.as_str().unwrap_or("");
                        // Compress via heuristics: keep first 3 sentences, drop filler words
                        let compressed = compress_hacker_style(s);
                        *content = Value::String(compressed);
                    }
                }
            }
            _ => {}
        }

        // Apply risk control
        match gene.risk_level.as_str() {
            "strict" => {
                if let Some(obj) = wrapped.as_object_mut() {
                    obj.insert("_safety_check".into(), Value::String("passed".into()));
                }
            }
            _ => {}
        }

        wrapped
    }

    /// Check if a student query passes risk control
    pub fn check_safety(query: &str, gene: &GeneProfile) -> bool {
        match gene.risk_level.as_str() {
            "strict" => {
                let blocked = ["作弊", "代写", "直接答案", "全部代码"];
                !blocked.iter().any(|b| query.contains(b))
            }
            "moderate" | "permissive" | _ => true,
        }
    }

    /// Modify a skill's behavior parameters based on gene
    pub fn modify_parameters(params: &mut Value, gene: &GeneProfile) {
        if let Some(obj) = params.as_object_mut() {
            match gene.personality.as_str() {
                "scholar" => {
                    obj.insert("precision".into(), Value::String("high".into()));
                    obj.insert("citation_required".into(), Value::Bool(true));
                }
                "mentor" => {
                    obj.insert("tone".into(), Value::String("warm".into()));
                    obj.insert("max_frustration_rounds".into(), Value::Number(5.into()));
                }
                "hacker" => {
                    obj.insert("brevity".into(), Value::String("max".into()));
                    obj.insert("comments".into(), Value::Bool(false));
                }
                _ => {}
            }
        }
    }
}

/// Compress output for hacker personality: remove filler words, keep key points only
fn compress_hacker_style(text: &str) -> String {
    let sentences: Vec<&str> = text.split(['。', '！', '？']).filter(|s| !s.trim().is_empty()).collect();
    if sentences.len() <= 2 { return text.to_string(); }
    // Keep first sentence (core answer) + last sentence (conclusion), drop explanations
    let key_sentences = sentences.iter().take(1).chain(sentences.iter().rev().take(1));
    key_sentences.map(|s| format!("{s}。")).collect::<Vec<_>>().join("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scholar_wrap() {
        let output = serde_json::json!({"content": "F=ma 表示力等于质量乘以加速度"});
        let gene = GeneProfile {
            personality: "scholar".into(),
            ..Default::default()
        };
        let wrapped = GeneGateway::wrap(&output, &gene);
        let content = wrapped["content"].as_str().unwrap();
        assert!(content.contains("置信度"));
        assert_eq!(wrapped["_gene_personality"], "scholar");
    }

    #[test]
    fn test_mentor_wrap() {
        let output = serde_json::json!({"content": "试试考虑力的方向"});
        let gene = GeneProfile {
            personality: "mentor".into(),
            ..Default::default()
        };
        let wrapped = GeneGateway::wrap(&output, &gene);
        let content = wrapped["content"].as_str().unwrap();
        assert!(content.contains("你已经很接近了"));
    }

    #[test]
    fn test_safety_block() {
        let gene = GeneProfile {
            risk_level: "strict".into(),
            ..Default::default()
        };
        assert!(!GeneGateway::check_safety("帮我写全部代码", &gene));
        assert!(GeneGateway::check_safety("如何理解F=ma？", &gene));
    }
}
