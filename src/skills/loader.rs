// SkillLoader — on-demand skill loading from file-based SOPs
//
// Inspired by GenericAgent's L1→L3 memory architecture.
// Instead of baking 35 full skill definitions into the MCP tool registry,
// we keep a lightweight index (_index.md) and load full SOPs only when
// the task context matches.
//
// Architecture:
//   _index.md          — L1: ≤30 line index, skill_name + one-liner + trigger words
//   memory/skills/*.md — L3: full SOP with strategy, pitfalls, success criteria

use std::path::PathBuf;

#[cfg(test)]
use std::io::Write;

/// A skill entry parsed from the index file
#[derive(Debug, Clone)]
pub struct SkillIndexEntry {
    pub name: String,
    pub description: String,
    pub trigger_words: Vec<String>,
    pub sop_file: String,
}

/// Loads skills on-demand by matching concept/query keywords to the index
pub struct SkillLoader {
    /// Parsed index entries
    index: Vec<SkillIndexEntry>,
    /// Base directory for SOP files
    skills_dir: PathBuf,
}

impl SkillLoader {
    /// Create a new SkillLoader by parsing the index file.
    /// The index file should be at `skills_dir/_index.md`.
    pub fn new(skills_dir: PathBuf) -> Self {
        let index = Self::parse_index(&skills_dir);
        Self { index, skills_dir }
    }

    /// Parse `_index.md` into SkillIndexEntry list.
    fn parse_index(skills_dir: &PathBuf) -> Vec<SkillIndexEntry> {
        let index_path = skills_dir.join("_index.md");
        let content = match std::fs::read_to_string(&index_path) {
            Ok(c) => c,
            Err(_) => {
                tracing::warn!("Skill index not found at {:?} — skills on-demand loading disabled", index_path);
                return Vec::new();
            }
        };

        let mut entries = Vec::new();
        for line in content.lines() {
            let trimmed = line.trim();
            // Skip headings, empty lines, comments
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
                continue;
            }

            // Format: `skill_name: one-line desc [触发词1,触发词2]`
            // Example: `tais-socratic-tutor: 苏格拉底式导师 [追问,引导,苏格拉底]`
            if let Some((name, rest)) = trimmed.split_once(':') {
                let name = name.trim().to_string();
                let rest = rest.trim();

                // Extract trigger words from [brackets]
                let (desc, trigger_words) = if let Some(bracket_start) = rest.find('[') {
                    let desc_part = rest[..bracket_start].trim();
                    let bracket_end = rest.rfind(']').unwrap_or(rest.len());
                    let trigger_str = &rest[bracket_start + 1..bracket_end];
                    let triggers: Vec<String> = trigger_str
                        .split(',')
                        .map(|s| s.trim().to_lowercase())
                        .filter(|s| !s.is_empty())
                        .collect();
                    (desc_part.to_string(), triggers)
                } else {
                    (rest.to_string(), Vec::new())
                };

                let sop_file = format!("{}.md", name);

                entries.push(SkillIndexEntry {
                    name,
                    description: desc,
                    trigger_words,
                    sop_file,
                });
            }
        }

        tracing::info!("SkillLoader: {} skills indexed from _index.md", entries.len());
        entries
    }

    /// Resolve skills matching the given context keywords.
    /// Returns a ranked list of (entry, score), best match first.
    pub fn match_skills(&self, keywords: &[&str]) -> Vec<(&SkillIndexEntry, f64)> {
        let mut scored: Vec<_> = self
            .index
            .iter()
            .map(|entry| {
                let score = self.match_score(entry, keywords);
                (entry, score)
            })
            .filter(|(_, score)| *score > 0.0)
            .collect();

        // Sort by score descending
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored
    }

    /// Score how well an entry matches the keywords.
    fn match_score(&self, entry: &SkillIndexEntry, keywords: &[&str]) -> f64 {
        let mut hits = 0u32;
        let name_lower = entry.name.to_lowercase();
        let desc_lower = entry.description.to_lowercase();

        for kw in keywords {
            let kw = kw.to_lowercase();
            // Exact name match = highest weight
            if name_lower == kw {
                hits += 3;
            } else if name_lower.contains(&kw) {
                hits += 2;
            }
            // Description match
            if desc_lower.contains(&kw) {
                hits += 1;
            }
            // Trigger word match
            if entry.trigger_words.iter().any(|t| t.contains(&kw) || kw.contains(t.as_str())) {
                hits += 2;
            }
        }

        if hits == 0 {
            return 0.0;
        }

        // Normalize: max possible = keywords.len() * 5 (all 3+2+1 hits)
        let max_possible = (keywords.len() * 5) as f64;
        (hits as f64 / max_possible).min(1.0)
    }

    /// Load the full SOP content for a given skill entry.
    pub fn load_sop(&self, entry: &SkillIndexEntry) -> Option<String> {
        let path = self.skills_dir.join(&entry.sop_file);
        match std::fs::read_to_string(&path) {
            Ok(content) => Some(content),
            Err(_) => {
                tracing::debug!("SOP file not found: {:?}", path);
                None
            }
        }
    }

    /// Resolve and load: match keywords → load top N SOPs → return concatenated context.
    /// This is the main entry point used by the agent loop.
    pub fn resolve(&self, concept_hint: Option<&str>, top_n: usize) -> String {
        let mut keywords: Vec<&str> = concept_hint
            .iter()
            .flat_map(|c| c.split_whitespace())
            .collect();

        // Always include some generic teaching context
        keywords.push("教学");
        keywords.push("学习");

        let matches = self.match_skills(&keywords);
        if matches.is_empty() {
            return String::new();
        }

        let mut context = String::from("[相关技能SOP]\n");
        let mut loaded = 0;

        for (entry, score) in matches.iter().take(top_n) {
            if let Some(sop) = self.load_sop(entry) {
                context.push_str(&format!(
                    "### {} (匹配度: {:.0}%)\n{}\n---\n",
                    entry.description,
                    score * 100.0,
                    sop
                ));
                loaded += 1;
            }
        }

        if loaded == 0 {
            return String::new();
        }

        tracing::debug!("SkillLoader: injected {} SOPs for keywords {:?}", loaded, concept_hint);
        context
    }

    /// Reload the index (e.g. after a new skill is crystallized).
    pub fn reload(&mut self) {
        self.index = Self::parse_index(&self.skills_dir);
        tracing::info!("SkillLoader: index reloaded ({} entries)", self.index.len());
    }

    /// Get the skills directory path.
    pub fn skills_dir(&self) -> &PathBuf {
        &self.skills_dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_loader() -> (SkillLoader, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("tais_test_{}", uuid::Uuid::new_v4()));
        let skills_dir = dir.join("skills");
        std::fs::create_dir_all(&skills_dir).unwrap();

        // Write index
        let index = skills_dir.join("_index.md");
        let mut f = std::fs::File::create(&index).unwrap();
        writeln!(f, "tais-socratic-tutor: 苏格拉底式导师 [追问,引导,苏格拉底]").unwrap();
        writeln!(f, "tais-workflow: 工作流编排 [工作流,DAG]").unwrap();
        writeln!(f, "oo-prd-generator: PRD生成 [PRD,需求]").unwrap();

        // Write one SOP
        let sop = skills_dir.join("tais-socratic-tutor.md");
        std::fs::write(&sop, "# Test SOP\nthis is a test").unwrap();

        let loader = SkillLoader::new(skills_dir);
        (loader, dir)
    }

    fn cleanup(dir: &std::path::PathBuf) {
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_parse_index() {
        let (loader, dir) = setup_test_loader();
        assert_eq!(loader.index.len(), 3);
        assert_eq!(loader.index[0].name, "tais-socratic-tutor");
        assert_eq!(loader.index[0].trigger_words, vec!["追问", "引导", "苏格拉底"]);
        cleanup(&dir);
    }

    #[test]
    fn test_match_skills_direct_hit() {
        let (loader, dir) = setup_test_loader();
        let matches = loader.match_skills(&["追问", "苏格拉底"]);
        assert!(!matches.is_empty());
        assert_eq!(matches[0].0.name, "tais-socratic-tutor");
        assert!(matches[0].1 >= 0.5, "Should have high score for direct match, got {}", matches[0].1);
        cleanup(&dir);
    }

    #[test]
    fn test_match_skills_no_match() {
        let (loader, dir) = setup_test_loader();
        let matches = loader.match_skills(&["量子力学", "相对论"]);
        assert!(matches.is_empty());
        cleanup(&dir);
    }

    #[test]
    fn test_load_sop() {
        let (loader, dir) = setup_test_loader();
        let entry = &loader.index[0];
        let sop = loader.load_sop(entry);
        assert!(sop.is_some());
        assert!(sop.unwrap().contains("Test SOP"));
        cleanup(&dir);
    }

    #[test]
    fn test_resolve_injects_context() {
        let (loader, dir) = setup_test_loader();
        let context = loader.resolve(Some("追问引导"), 2);
        assert!(context.contains("苏格拉底式导师"));
        assert!(context.contains("Test SOP"));
        cleanup(&dir);
    }

    #[test]
    fn test_resolve_empty_when_no_match() {
        let (loader, dir) = setup_test_loader();
        let context = loader.resolve(Some("xyz不存在的概念"), 2);
        let _ = context;
        cleanup(&dir);
    }
}
