// Orchestrator DAG — workflow graph representation using petgraph

use petgraph::graph::DiGraph;
use petgraph::visit::Topo;
use crate::WorkflowNode;

/// A DAG wrapper around petgraph for workflow execution order
pub struct WorkflowDag {
    graph: DiGraph<WorkflowNode, ()>,
    node_indices: Vec<petgraph::graph::NodeIndex>,
}

impl WorkflowDag {
    /// Build a DAG from nodes and edges
    pub fn build(nodes: Vec<WorkflowNode>, edges: &[(String, String)]) -> Self {
        let mut graph = DiGraph::new();
        let mut index_map = std::collections::HashMap::new();

        for node in &nodes {
            let idx = graph.add_node(node.clone());
            index_map.insert(node.id.clone(), idx);
        }

        for (from, to) in edges {
            if let (Some(&f), Some(&t)) = (index_map.get(from), index_map.get(to)) {
                graph.add_edge(f, t, ());
            }
        }

        let node_indices: Vec<_> = index_map.values().copied().collect();

        Self {
            graph,
            node_indices,
        }
    }

    /// Get nodes in topological (execution) order
    pub fn execution_order(&self) -> Vec<&WorkflowNode> {
        let mut topo = Topo::new(&self.graph);
        let mut order = Vec::new();

        while let Some(idx) = topo.next(&self.graph) {
            order.push(&self.graph[idx]);
        }

        order
    }

    /// Find all predecessors of a node (nodes that must complete first)
    pub fn predecessors(&self, node_id: &str) -> Vec<&WorkflowNode> {
        self.node_indices
            .iter()
            .filter(|&&idx| {
                let _node = &self.graph[idx];
                self.graph
                    .neighbors_directed(idx, petgraph::Direction::Incoming)
                    .any(|pred| self.graph[pred].id == node_id)
            })
            .map(|&idx| &self.graph[idx])
            .collect()
    }

    /// Count total nodes
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node(id: &str, name: &str) -> WorkflowNode {
        WorkflowNode {
            id: id.into(),
            name: name.into(),
            agent: vec![],
            gene: vec![],
            mcp_tools: vec![],
            input: serde_json::json!({}),
            hitl_trigger: None,
        }
    }

    #[test]
    fn test_dag_order() {
        let a = make_node("A", "诊断");
        let b = make_node("B", "导入");
        let c = make_node("C", "练习");

        let dag = WorkflowDag::build(
            vec![a, b, c],
            &[("A".into(), "B".into()), ("B".into(), "C".into())],
        );

        let order: Vec<_> = dag.execution_order().iter().map(|n| n.name.as_str()).collect();
        assert_eq!(order, vec!["诊断", "导入", "练习"]);
        assert_eq!(dag.node_count(), 3);
    }
}
