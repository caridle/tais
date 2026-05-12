// Skill Crystallizer — distill successful teaching sessions into reusable SOPs
//
// Inspired by GenericAgent's start_long_term_update + L1→L3 memory architecture.
//
// When a teaching session succeeds (high rating, teacher approved), the agent
// can call crystallize_skill() to write a reusable SOP file. This auto-grows
// TAIS's skill library without manual coding.
//
// Hooked into:
//   - H01 复盘 habit: daily scan → suggest crystallization
//   - H05 优化 habit: evolution trigger → update existing SOPs

use std::path::PathBuf;

/// Result of a crystallization attempt
#[derive(Debug, Clone)]
pub struct CrystallizeResult {
    pub skill_name: String,
    pub sop_path: String,
    pub index_updated: bool,
    pub message: String,
}

/// Crystallize a successful teaching pattern into a reusable skill SOP.
///
/// Arguments:
///   - skills_dir: path to memory/skills/
///   - skill_name: unique name for this skill (e.g. "physics_newton_inquiry")
///   - description: one-line description with trigger words
///   - strategy: the teaching strategy that worked
///   - pitfalls: common mistakes to avoid
///   - success_indicators: how to know it's working
pub fn crystallize_skill(
    skills_dir: &PathBuf,
    skill_name: &str,
    description: &str,
    strategy: &str,
    pitfalls: &[&str],
    success_indicators: &[&str],
) -> Result<CrystallizeResult, String> {
    // Build the SOP file content
    let pitfalls_str = if pitfalls.is_empty() {
        String::from("（暂无记录）")
    } else {
        pitfalls.iter().map(|p| format!("- ❌ {}", p)).collect::<Vec<_>>().join("\n")
    };

    let indicators_str = if success_indicators.is_empty() {
        String::from("- 学生能正确回答相关练习")
    } else {
        success_indicators.iter().map(|i| format!("- {}", i)).collect::<Vec<_>>().join("\n")
    };

    let sop_content = format!(
        r#"# {name}: {desc}

## 核心策略
{strategy}

## 常见坑点
{pitfalls}

## 成功指标
{indicators}

## 元信息
- 结晶时间: {timestamp}
- 来源: TAIS 习惯引擎自动结晶
- 技能类型: 自进化生成
"#,
        name = skill_name,
        desc = description,
        strategy = strategy,
        pitfalls = pitfalls_str,
        indicators = indicators_str,
        timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S"),
    );

    // Write SOP .md file
    let sop_file_name = format!("{}.md", skill_name);
    let sop_path = skills_dir.join(&sop_file_name);
    std::fs::write(&sop_path, &sop_content)
        .map_err(|e| format!("Failed to write SOP file: {e}"))?;

    // Update the index file
    let index_updated = update_index(skills_dir, skill_name, description)?;

    tracing::info!(
        "Crystallized new skill: {} → {:?} (index_updated={})",
        skill_name, sop_path, index_updated
    );

    Ok(CrystallizeResult {
        skill_name: skill_name.into(),
        sop_path: sop_path.to_string_lossy().into(),
        index_updated,
        message: format!("Skill '{}' crystallized successfully", skill_name),
    })
}

/// Update the _index.md to include a new skill entry.
fn update_index(skills_dir: &PathBuf, skill_name: &str, description: &str) -> Result<bool, String> {
    let index_path = skills_dir.join("_index.md");
    if !index_path.exists() {
        return Ok(false);
    }

    let content = std::fs::read_to_string(&index_path)
        .map_err(|e| format!("Failed to read index: {e}"))?;

    // Check if this skill is already indexed
    if content.contains(&format!("{}:", skill_name)) {
        tracing::debug!("Skill '{}' already in index, skipping index update", skill_name);
        return Ok(false);
    }

    // Find the best section to add to (after ## Self-Evolved Skills, or at end)
    let insertion_line = if let Some(pos) = content.find("## Self-Evolved Skills") {
        // Insert after the heading
        content[..pos].lines().count()
    } else {
        // Append at end
        content.lines().count()
    };

    let new_entry = format!("{}: {} [自进化]", skill_name, description);

    let mut lines: Vec<String> = content.lines().map(String::from).collect();

    if insertion_line < lines.len() {
        // Insert after the "## Self-Evolved Skills" heading (next line)
        lines.insert(insertion_line + 1, new_entry);
    } else {
        // Append new section
        lines.push(String::new());
        lines.push("## Self-Evolved Skills".into());
        lines.push(new_entry);
    }

    let updated = lines.join("\n") + "\n";
    std::fs::write(&index_path, &updated)
        .map_err(|e| format!("Failed to write index: {e}"))?;

    Ok(true)
}

/// List all crystallized (self-evolved) skills.
pub fn list_crystallized_skills(skills_dir: &PathBuf) -> Vec<String> {
    let mut skills = Vec::new();
    if let Ok(entries) = std::fs::read_dir(skills_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            // Skip _index.md and built-in tais_*.md files
            if name.ends_with(".md") && !name.starts_with("_") && !name.starts_with("tais_") {
                skills.push(name.trim_end_matches(".md").to_string());
            }
        }
    }
    skills
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tais_cryst_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cleanup(dir: &PathBuf) {
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_crystallize_and_list() {
        let skills_dir = setup_dir();

        // Create a minimal index
        std::fs::write(
            skills_dir.join("_index.md"),
            "# Test Index\nexisting-skill: test [test]\n",
        ).unwrap();

        let result = crystallize_skill(
            &skills_dir,
            "physics_newton_inquiry",
            "牛顿定律探究式教学 [牛顿,探究,力学]",
            "1. 从生活例子引入\n2. 追问引导学生发现 F=ma\n3. 实验验证",
            &["学生可能混淆力和加速度的概念", "不要一开始就给出公式"],
            &["学生能用自己的话解释 F=ma", "能用公式解简单题目"],
        );

        assert!(result.is_ok());
        let r = result.unwrap();
        assert!(r.index_updated);
        assert!(r.sop_path.contains("physics_newton_inquiry.md"));

        // Verify SOP file was written
        let sop = std::fs::read_to_string(&r.sop_path).unwrap();
        assert!(sop.contains("牛顿定律探究式教学"));
        assert!(sop.contains("常见坑点"));
        assert!(sop.contains("成功指标"));

        // Verify index was updated
        let index = std::fs::read_to_string(skills_dir.join("_index.md")).unwrap();
        assert!(index.contains("physics_newton_inquiry"));

        // List crystallized skills
        let skills = list_crystallized_skills(&skills_dir);
        assert!(skills.contains(&"physics_newton_inquiry".to_string()));
        cleanup(&skills_dir);
    }

    #[test]
    fn test_duplicate_skill_no_double_index() {
        let skills_dir = setup_dir();

        std::fs::write(
            skills_dir.join("_index.md"),
            "# Test\nphysics_newton_inquiry: already here [牛顿]\n",
        ).unwrap();

        let result = crystallize_skill(
            &skills_dir,
            "physics_newton_inquiry",
            "updated description [牛顿]",
            "strategy",
            &[],
            &[],
        );
        assert!(result.is_ok());
        assert!(!result.unwrap().index_updated);

        // Index should still have only one entry
        let index = std::fs::read_to_string(skills_dir.join("_index.md")).unwrap();
        assert_eq!(index.matches("physics_newton_inquiry").count(), 1);
        cleanup(&skills_dir);
    }
}
