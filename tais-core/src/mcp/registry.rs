// Tool Registry — stores all available MCP tools

use crate::McpTool;
use std::collections::HashMap;

#[derive(Default)]
pub struct ToolRegistry {
    tools: HashMap<String, McpTool>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: McpTool) {
        self.tools.insert(tool.name.clone(), tool);
    }

    pub fn get(&self, name: &str) -> Option<&McpTool> {
        self.tools.get(name)
    }

    pub fn list_all(&self) -> Vec<McpTool> {
        self.tools.values().cloned().collect()
    }

    /// Bulk register — for loading OO capsules at startup
    pub fn register_batch(&mut self, tools: Vec<McpTool>) {
        for tool in tools {
            self.tools.insert(tool.name.clone(), tool);
        }
    }
}
