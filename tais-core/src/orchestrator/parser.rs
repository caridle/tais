// Orchestrator Parser — parse teacher goals from natural language
// Two modes: rule-based (fast, no LLM) + LLM-powered (async, accurate)

use crate::{StudentLevel, TeachingGoal, TeachingMode};
use std::sync::Arc;

/// Rule-based parser — fast, deterministic fallback (no LLM needed)
pub fn parse_goal(input: &str, teacher_id: &str) -> Option<TeachingGoal> {
    let input_lower = input.to_lowercase();

    let mode = if input_lower.contains("探究") || input_lower.contains("inquiry") {
        TeachingMode::InquiryBased
    } else if input_lower.contains("苏格拉底") || input_lower.contains("socratic") {
        TeachingMode::SocraticDialogue
    } else if input_lower.contains("项目") || input_lower.contains("project") {
        TeachingMode::ProjectBased
    } else if input_lower.contains("翻转") || input_lower.contains("flipped") {
        TeachingMode::FlippedClassroom
    } else {
        TeachingMode::DirectInstruction
    };

    let level = if input_lower.contains("基础") || input_lower.contains("初") || input_lower.contains("beginner") {
        StudentLevel::Beginner
    } else if input_lower.contains("高级") || input_lower.contains("advanced") {
        StudentLevel::Advanced
    } else {
        StudentLevel::Intermediate
    };

    let subject = extract_subject(&input_lower);
    let concept = extract_concept(&input_lower);

    Some(TeachingGoal {
        subject,
        concept,
        mode,
        target_level: level,
        constraints: vec![],
        teacher_id: teacher_id.into(),
    })
}

/// LLM-powered parser — uses LlmRouter for accurate NLU.
/// Falls back to rule-based if LLM is unavailable.
pub async fn parse_goal_with_llm(
    input: &str,
    teacher_id: &str,
    llm_router: &crate::llm::LlmRouter,
) -> TeachingGoal {
    let prompt = format!(
        "You are a teaching goal parser. Extract from: \"{input}\"\n\
         Return ONLY a JSON object: {{\"subject\":\"...\",\"concept\":\"...\",\"mode\":\"inquiry_based|direct_instruction|project_based|flipped_classroom|socratic_dialogue\",\"level\":\"beginner|intermediate|advanced\"}}\n\
         No explanation, no markdown."
    );

    let msg = crate::llm::ChatMessage {
        role: crate::llm::MessageRole::User,
        content: prompt,
    };
    match llm_router.chat(&[msg], None, None).await {
        Ok(response) => {
            let text = response.content.clone();
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) {
                let mode = match parsed["mode"].as_str().unwrap_or("direct_instruction") {
                    "socratic_dialogue" => TeachingMode::SocraticDialogue,
                    "project_based" => TeachingMode::ProjectBased,
                    "flipped_classroom" => TeachingMode::FlippedClassroom,
                    "inquiry_based" => TeachingMode::InquiryBased,
                    _ => TeachingMode::DirectInstruction,
                };
                let level = match parsed["level"].as_str().unwrap_or("intermediate") {
                    "beginner" => StudentLevel::Beginner,
                    "advanced" => StudentLevel::Advanced,
                    _ => StudentLevel::Intermediate,
                };
                return TeachingGoal {
                    subject: parsed["subject"].as_str().unwrap_or("通用").into(),
                    concept: parsed["concept"].as_str().unwrap_or(input).into(),
                    mode,
                    target_level: level,
                    constraints: vec![],
                    teacher_id: teacher_id.into(),
                };
            }
        }
        Err(e) => {
            tracing::warn!("LLM goal parsing failed: {e}, using rule-based fallback");
        }
    }

    // Fallback to rule-based
    parse_goal(input, teacher_id).unwrap_or_else(|| TeachingGoal {
        subject: "通用".into(),
        concept: input.into(),
        mode: TeachingMode::InquiryBased,
        target_level: StudentLevel::Intermediate,
        constraints: vec![],
        teacher_id: teacher_id.into(),
    })
}

fn extract_subject(input: &str) -> String {
    if input.contains("物理") { "物理".into() }
    else if input.contains("数学") { "数学".into() }
    else if input.contains("化学") { "化学".into() }
    else if input.contains("编程") || input.contains("代码") { "编程".into() }
    else if input.contains("写作") { "写作".into() }
    else { "通用".into() }
}

fn extract_concept(input: &str) -> String {
    if let Some(start) = input.find('「') {
        if let Some(end) = input[start..].find('」') {
            return input[start + 3..start + end].to_string();
        }
    }
    input.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_inquiry_goal() {
        let goal = parse_goal(
            "为高中物理「牛顿第二定律」设计探究式学习工作流",
            "teacher_001",
        ).unwrap();
        assert_eq!(goal.subject, "物理");
        assert!(matches!(goal.mode, TeachingMode::InquiryBased));
    }

    #[test]
    fn test_parse_socratic() {
        let goal = parse_goal("通过苏格拉底式追问引导学生理解力的合成", "teacher_001").unwrap();
        assert!(matches!(goal.mode, TeachingMode::SocraticDialogue));
    }
}
