// Orchestrator Executor — run workflow nodes in topological order via real SkillsBus

use crate::*;
use super::dag::WorkflowDag;
use std::collections::HashMap;
use std::sync::Arc;

pub struct ExecutionContext {
    pub session_id: Id,
    pub student_id: String,
    pub workflow: Workflow,
    pub node_results: HashMap<String, NodeResult>,
    pub hitl_queue: Vec<HitlEvent>,
    skills_bus: Arc<skills::SkillsBus>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NodeResult {
    pub node_id: String,
    pub output: serde_json::Value,
    pub hitl_triggered: bool,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HitlEvent {
    pub node_id: String,
    pub condition: HitlCondition,
    pub action: HitlAction,
    pub context: serde_json::Value,
}

impl ExecutionContext {
    pub fn new(
        session_id: Id,
        student_id: String,
        workflow: Workflow,
        skills_bus: Arc<skills::SkillsBus>,
    ) -> Self {
        Self {
            session_id,
            student_id,
            workflow,
            node_results: HashMap::new(),
            hitl_queue: Vec::new(),
            skills_bus,
        }
    }

    /// Execute all nodes in topological order via real SkillsBus.
    pub async fn execute_all(
        &mut self,
        _orchestrator: &super::Orchestrator,
    ) -> (Vec<NodeResult>, Vec<HitlEvent>) {
        let dag = WorkflowDag::build(
            self.workflow.nodes.clone(),
            &self.workflow.edges,
        );

        let order = dag.execution_order();
        let mut results = Vec::new();

        for node in order {
            let result = self.execute_node(node).await;
            self.node_results.insert(node.id.clone(), result.clone());
            results.push(result);
        }

        (results, self.hitl_queue.clone())
    }

    async fn execute_node(&mut self, node: &WorkflowNode) -> NodeResult {
        let start = std::time::Instant::now();
        let agent = node.agent.first().cloned().unwrap_or_default();
        let gene = GeneProfile::default();

        // Real execution via SkillsBus
        let output = match self.skills_bus.execute(&agent, node.input.clone(), &gene).await {
            Ok(result) => result,
            Err(e) => serde_json::json!({
                "node": node.name,
                "agent": node.agent,
                "status": "failed",
                "error": e.to_string(),
            }),
        };

        let duration_ms = start.elapsed().as_millis() as u64;

        // Check HITL triggers using real output data
        let hitl_triggered = if let Some(ref trigger) = node.hitl_trigger {
            let should_trigger = match &trigger.condition {
                HitlCondition::ConfidenceBelow(threshold) => {
                    output["confidence"]
                        .as_f64()
                        .map(|c| c < *threshold)
                        .unwrap_or(false)
                }
                HitlCondition::NoProgressAfter(rounds) => {
                    output["progress_rounds"]
                        .as_u64()
                        .map(|r| r >= *rounds as u64)
                        .unwrap_or(false)
                }
                HitlCondition::CommonErrorAbove(rate) => {
                    output["error_rate"]
                        .as_f64()
                        .map(|r| r > *rate)
                        .unwrap_or(false)
                }
            };

            if should_trigger {
                self.hitl_queue.push(HitlEvent {
                    node_id: node.id.clone(),
                    condition: trigger.condition.clone(),
                    action: trigger.action.clone(),
                    context: output.clone(),
                });
                true
            } else {
                false
            }
        } else {
            false
        };

        NodeResult {
            node_id: node.id.clone(),
            output,
            hitl_triggered,
            duration_ms,
        }
    }
}
