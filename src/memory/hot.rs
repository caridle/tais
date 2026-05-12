// Hot Memory (热记忆) — Hermes-style MEMORY.md
// Injected every turn, strictly size-limited (≤500 chars).
// Cold/archival data is retrieved on-demand via session_search.

use std::fs;
use std::path::PathBuf;

const MAX_CHARS: usize = 500;
const MEMORY_PATH: &str = "memory/tais_hot_memory.md";

/// HotMemory — lightweight persistent facts, injected into every agent turn.
pub struct HotMemory {
    path: PathBuf,
}

impl HotMemory {
    pub fn new() -> Self {
        let path = PathBuf::from(MEMORY_PATH);
        // Ensure file exists
        if !path.exists() {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(&path, "# TAIS Hot Memory\n§\n");
        }
        Self { path }
    }

    /// Read hot memory, stripping the header and returning clean facts.
    pub fn read(&self) -> String {
        let raw = fs::read_to_string(&self.path).unwrap_or_default();
        let facts: String = raw
            .lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        // Truncate to MAX_CHARS
        if facts.len() > MAX_CHARS {
            let mut end = MAX_CHARS;
            while end > 0 && !facts.is_char_boundary(end) { end -= 1; }
            format!("{}...", &facts[..end])
        } else {
            facts
        }
    }

    /// Add a fact (append to file).
    pub fn add(&self, fact: &str) -> Result<(), String> {
        let content = fs::read_to_string(&self.path).unwrap_or_default();
        let fact = fact.trim();
        if fact.is_empty() { return Err("empty fact".into()); }
        if content.contains(fact) { return Err("duplicate".into()); }

        // Insert before last § if present, otherwise append
        let mut lines: Vec<String> = content.lines().map(String::from).collect();
        // Remove trailing empty lines
        while lines.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
            lines.pop();
        }
        let new_line = format!("{} §", fact);
        // Insert after last § entry, before closing
        if let Some(pos) = lines.iter().rposition(|l| l.contains('§')) {
            lines.insert(pos + 1, new_line);
        } else {
            lines.push(new_line);
        }
        let updated = lines.join("\n") + "\n";
        // Check size
        let facts_only: String = updated
            .lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if facts_only.len() > MAX_CHARS + 200 {
            return Err(format!("memory full ({} chars), remove old entries first", facts_only.len()));
        }
        fs::write(&self.path, &updated).map_err(|e| format!("write error: {e}"))
    }

    /// Replace a fact (substring match).
    pub fn replace(&self, old: &str, new: &str) -> Result<(), String> {
        let content = fs::read_to_string(&self.path).unwrap_or_default();
        if !content.contains(old) {
            return Err(format!("not found: {}", old));
        }
        let updated = content.replace(old, new);
        fs::write(&self.path, &updated).map_err(|e| format!("write error: {e}"))
    }

    /// Remove a fact (substring match).
    pub fn remove(&self, substring: &str) -> Result<(), String> {
        let content = fs::read_to_string(&self.path).unwrap_or_default();
        if !content.contains(substring) {
            return Err(format!("not found: {}", substring));
        }
        let old_len = content.lines().count();
        let updated: String = content
            .lines()
            .filter(|l| !l.contains(substring))
            .collect::<Vec<_>>()
            .join("\n");
        if updated.lines().count() == old_len {
            return Err("no lines removed".into());
        }
        fs::write(&self.path, updated + "\n")
            .map_err(|e| format!("write error: {e}"))
    }

    /// Get current size in chars.
    pub fn size(&self) -> usize { self.read().len() }

    /// Format for injection into system prompt.
    pub fn to_prompt(&self) -> String {
        let facts = self.read();
        if facts.is_empty() { return String::new(); }
        format!(
            "[热记忆 — 关于用户和环境的关键事实]\n{}\n[/热记忆]\n",
            facts
        )
    }
}

impl Default for HotMemory {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_empty() {
        let hm = HotMemory::new();
        let content = hm.read();
        assert!(content.len() <= MAX_CHARS);
    }
}
