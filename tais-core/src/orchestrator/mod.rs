// Orchestrator — DAG workflow generation and execution
//
// Steps:
//   1. Parse teacher goal → extract subject, concept, mode, level
//   2. Generate DAG nodes based on teaching mode
//   3. Assign agents (TAIS skills) to each node
//   4. Inject gene profile at decision points
//   5. Set HITL triggers at critical nodes
//   6. Execute nodes in topological order

pub mod parser;
pub mod dag;
pub mod executor;
pub mod task;

use crate::*;
use std::sync::Arc;

/// The Orchestrator creates and runs teaching workflows.
pub struct Orchestrator {
    /// Pre-built node templates for different teaching modes
    templates: Vec<NodeTemplate>,
    /// MCP gateway for tool access
    mcp_gateway: Option<Arc<mcp::Gateway>>,
}

/// A reusable node template for workflow generation
#[derive(Debug, Clone)]
pub struct NodeTemplate {
    pub mode: TeachingMode,
    pub phase: u32, // 0=pre, 1=during, 2=post, 3=review
    pub name: String,
    pub default_agent: Vec<String>,
    pub required_tools: Vec<String>,
    pub has_hitl: bool,
    pub default_hitl: Option<HitlTrigger>,
    pub can_skip: bool,
}

impl Orchestrator {
    pub fn new() -> Self {
        Self {
            templates: default_templates(),
            mcp_gateway: None,
        }
    }

    pub fn with_mcp(mut self, gateway: Arc<mcp::Gateway>) -> Self {
        self.mcp_gateway = Some(gateway);
        self
    }

    /// Generate a workflow DAG from a teaching goal.
    pub fn generate(&self, goal: TeachingGoal) -> Workflow {
        let workflow_id = Uuid::new_v4();

        // 1. Filter templates matching the teaching mode
        let mut matching: Vec<&NodeTemplate> = self
            .templates
            .iter()
            .filter(|t| t.mode == goal.mode)
            .collect();
        matching.sort_by_key(|t| t.phase);

        // 2. Build nodes from templates
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        let mut prev_id: Option<String> = None;

        for template in &matching {
            let node_id = format!("{}_{}", template.name.to_lowercase().replace(' ', "_"), nodes.len());

            let hitl = if template.has_hitl {
                template.default_hitl.clone()
            } else {
                None
            };

            let node = WorkflowNode {
                id: node_id.clone(),
                name: template.name.clone(),
                agent: template.default_agent.clone(),
                gene: vec![goal_level_gene(&goal)],
                mcp_tools: template.required_tools.clone(),
                input: serde_json::json!({
                    "concept": goal.concept,
                    "level": goal.target_level,
                    "mode": goal.mode
                }),
                hitl_trigger: hitl,
            };

            if let Some(ref prev) = prev_id {
                edges.push((prev.clone(), node_id.clone()));
            }
            prev_id = Some(node_id.clone());
            nodes.push(node);
        }

        // 3. Add terminal review node (always present)
        let review_id = "teacher_review".to_string();
        let review_node = WorkflowNode {
            id: review_id.clone(),
            name: "教师审查".into(),
            agent: vec!["tais-workflow".into()],
            gene: vec!["gene-scholar".into()],
            mcp_tools: vec![],
            input: serde_json::json!({"action": "review_report"}),
            hitl_trigger: Some(HitlTrigger {
                condition: HitlCondition::ConfidenceBelow(1.0), // always triggers
                action: HitlAction::EscalateToTeacher,
            }),
        };

        if let Some(ref prev) = prev_id {
            edges.push((prev.clone(), review_id.clone()));
        }
        nodes.push(review_node);

        Workflow {
            id: workflow_id,
            entry: nodes.first().map(|n| n.id.clone()).unwrap_or_default(),
            nodes,
            edges,
            goal,
            gene_profile: GeneProfile::default(),
        }
    }
}

impl Default for Orchestrator {
    fn default() -> Self {
        Self::new()
    }
}

/// Select the best gene based on the goal's target level
fn goal_level_gene(goal: &TeachingGoal) -> String {
    match goal.target_level {
        StudentLevel::Beginner => "gene-mentor",
        StudentLevel::Intermediate => "gene-scholar",
        StudentLevel::Advanced => "gene-hacker",
    }
    .into()
}

/// Default node templates for common teaching modes
fn default_templates() -> Vec<NodeTemplate> {
    vec![
        // ── Inquiry-Based Learning ──
        NodeTemplate {
            mode: TeachingMode::InquiryBased,
            phase: 0,
            name: "课前诊断".into(),
            default_agent: vec!["tais-learning-analyst".into()],
            required_tools: vec!["oo-domain-model".into()],
            has_hitl: true,
            default_hitl: Some(HitlTrigger {
                condition: HitlCondition::ConfidenceBelow(0.7),
                action: HitlAction::EscalateToTeacher,
            }),
            can_skip: false,
        },
        NodeTemplate {
            mode: TeachingMode::InquiryBased,
            phase: 1,
            name: "概念导入".into(),
            default_agent: vec!["tais-socratic-tutor".into()],
            required_tools: vec![],
            has_hitl: false,
            default_hitl: None,
            can_skip: true,
        },
        NodeTemplate {
            mode: TeachingMode::InquiryBased,
            phase: 1,
            name: "探究引导".into(),
            default_agent: vec!["tais-socratic-tutor".into()],
            required_tools: vec!["physics-simulator".into()],
            has_hitl: true,
            default_hitl: Some(HitlTrigger {
                condition: HitlCondition::NoProgressAfter(3),
                action: HitlAction::PauseAndNotify,
            }),
            can_skip: false,
        },
        NodeTemplate {
            mode: TeachingMode::InquiryBased,
            phase: 2,
            name: "练习巩固".into(),
            default_agent: vec!["tais-skill-coach".into(), "tais-resource-pusher".into()],
            required_tools: vec!["code-runner".into()],
            has_hitl: true,
            default_hitl: Some(HitlTrigger {
                condition: HitlCondition::CommonErrorAbove(0.4),
                action: HitlAction::EscalateToTeacher,
            }),
            can_skip: false,
        },
        NodeTemplate {
            mode: TeachingMode::InquiryBased,
            phase: 2,
            name: "课后评估".into(),
            default_agent: vec!["tais-learning-analyst".into()],
            required_tools: vec![],
            has_hitl: false,
            default_hitl: None,
            can_skip: false,
        },
        // ── Socratic Dialogue ──
        NodeTemplate {
            mode: TeachingMode::SocraticDialogue,
            phase: 1,
            name: "苏格拉底追问".into(),
            default_agent: vec!["tais-socratic-tutor".into()],
            required_tools: vec![],
            has_hitl: true,
            default_hitl: Some(HitlTrigger {
                condition: HitlCondition::NoProgressAfter(3),
                action: HitlAction::PauseAndNotify,
            }),
            can_skip: false,
        },
    ]
}
