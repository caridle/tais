// Task Manager — task lifecycle management for agent orchestration
//
// 任务编排五问：
//   1. 如何列出任务  → GET /api/tasks/workflow/{id}
//   2. 如何分派任务  → assign(task_id, agent) → status=Assigned
//   3. 如何监督状态  → GET /api/tasks/{id}/status + HITL events
//   4. 如何增减任务  → POST/DELETE /api/tasks
//   5. 如何中断任务  → POST /api/tasks/{id}/interrupt?reason=
//
// 状态机：
//   Pending → Assigned → InProgress → Completed
//                    ↘           ↘ Failed
//                     ↘ Interrupted (manual or HITL)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::RwLock;

// ── Task Status ────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Waiting to be dispatched
    Pending,
    /// Assigned to a specific agent but not yet started
    Assigned,
    /// Currently executing
    InProgress,
    /// Successfully completed
    Completed,
    /// Execution failed
    Failed,
    /// Interrupted (manual user action or HITL trigger)
    Interrupted,
    /// Skipped (teacher chose to bypass)
    Skipped,
}

impl TaskStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(self, TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Skipped | TaskStatus::Interrupted)
    }

    pub fn is_active(&self) -> bool {
        matches!(self, TaskStatus::Pending | TaskStatus::Assigned | TaskStatus::InProgress)
    }
}

// ── Task ───────────────────────────────────────────────────────────────

/// A single unit of work in a teaching workflow
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub workflow_id: String,
    pub name: String,
    pub description: String,
    /// Which agents/skills handle this task
    pub assigned_agent: Vec<String>,
    pub status: TaskStatus,
    /// Priority: lower = more urgent (0 is highest)
    pub priority: u32,
    /// Task IDs that must complete before this one
    pub dependencies: Vec<String>,
    pub created_at: chrono::NaiveDateTime,
    pub started_at: Option<chrono::NaiveDateTime>,
    pub completed_at: Option<chrono::NaiveDateTime>,
    /// Result summary after completion
    pub result: Option<String>,
    /// Reason for interruption (if status=Interrupted)
    pub interrupt_reason: Option<String>,
}

impl Task {
    pub fn new(id: &str, workflow_id: &str, name: &str, description: &str) -> Self {
        Self {
            id: id.into(),
            workflow_id: workflow_id.into(),
            name: name.into(),
            description: description.into(),
            assigned_agent: vec![],
            status: TaskStatus::Pending,
            priority: 10,
            dependencies: vec![],
            created_at: chrono::Utc::now().naive_utc(),
            started_at: None,
            completed_at: None,
            result: None,
            interrupt_reason: None,
        }
    }

    /// Assign this task to an agent
    pub fn assign(&mut self, agent: &str) {
        if !self.assigned_agent.contains(&agent.to_string()) {
            self.assigned_agent.push(agent.into());
        }
        self.status = TaskStatus::Assigned;
    }

    /// Mark as in-progress
    pub fn start(&mut self) -> Result<(), String> {
        if self.status != TaskStatus::Assigned && self.status != TaskStatus::Pending {
            return Err(format!("Cannot start task {} from status {:?}", self.id, self.status));
        }
        self.status = TaskStatus::InProgress;
        self.started_at = Some(chrono::Utc::now().naive_utc());
        Ok(())
    }

    /// Mark as completed
    pub fn complete(&mut self, result: &str) {
        self.status = TaskStatus::Completed;
        self.completed_at = Some(chrono::Utc::now().naive_utc());
        self.result = Some(result.into());
    }

    /// Mark as failed
    pub fn fail(&mut self, reason: &str) {
        self.status = TaskStatus::Failed;
        self.completed_at = Some(chrono::Utc::now().naive_utc());
        self.result = Some(reason.into());
    }

    /// Interrupt this task
    pub fn interrupt(&mut self, reason: &str) {
        self.status = TaskStatus::Interrupted;
        self.interrupt_reason = Some(reason.into());
        self.completed_at = Some(chrono::Utc::now().naive_utc());
    }

    /// Skip this task
    pub fn skip(&mut self) {
        self.status = TaskStatus::Skipped;
        self.completed_at = Some(chrono::Utc::now().naive_utc());
    }

    /// Can this task be started? (all dependencies completed)
    pub fn dependencies_met(&self, all_tasks: &HashMap<String, Task>) -> bool {
        self.dependencies.iter().all(|dep_id| {
            all_tasks
                .get(dep_id)
                .map(|t| t.status == TaskStatus::Completed || t.status == TaskStatus::Skipped)
                .unwrap_or(true) // unknown dep → assume met
        })
    }
}

// ── TaskManager ────────────────────────────────────────────────────────

/// Manages the lifecycle of all tasks across workflows
pub struct TaskManager {
    tasks: RwLock<HashMap<String, Task>>,
    workflow_tasks: RwLock<HashMap<String, Vec<String>>>,
}

impl TaskManager {
    pub fn new() -> Self {
        Self {
            tasks: RwLock::new(HashMap::new()),
            workflow_tasks: RwLock::new(HashMap::new()),
        }
    }

    // ── Create ─────────────────────────────────────────────────────────

    /// Create a new task and add it to a workflow
    pub async fn create(
        &self,
        workflow_id: &str,
        name: &str,
        description: &str,
        agent: Option<&str>,
        dependencies: Vec<String>,
    ) -> Task {
        let id = format!("{}_{}", workflow_id, uuid::Uuid::new_v4().to_string()[..8].to_string());
        let mut task = Task::new(&id, workflow_id, name, description);
        task.dependencies = dependencies;

        if let Some(a) = agent {
            task.assign(a);
        }

        let mut tasks = self.tasks.write().await;
        tasks.insert(id.clone(), task.clone());

        let mut wt = self.workflow_tasks.write().await;
        wt.entry(workflow_id.into()).or_default().push(id);

        task
    }

    // ── Read ───────────────────────────────────────────────────────────

    /// Get a single task
    pub async fn get(&self, task_id: &str) -> Option<Task> {
        let tasks = self.tasks.read().await;
        tasks.get(task_id).cloned()
    }

    /// List all tasks for a workflow, sorted by priority
    pub async fn list_by_workflow(&self, workflow_id: &str) -> Vec<Task> {
        let wt = self.workflow_tasks.read().await;
        let tasks = self.tasks.read().await;

        if let Some(task_ids) = wt.get(workflow_id) {
            let mut result: Vec<Task> = task_ids
                .iter()
                .filter_map(|id| tasks.get(id).cloned())
                .collect();
            result.sort_by_key(|t| t.priority);
            result
        } else {
            vec![]
        }
    }

    /// Get next pending task that has all dependencies met
    pub async fn next_ready(&self, workflow_id: &str) -> Option<Task> {
        let all_tasks = self.tasks.read().await;
        let wt = self.workflow_tasks.read().await;

        if let Some(task_ids) = wt.get(workflow_id) {
            for id in task_ids {
                if let Some(task) = all_tasks.get(id) {
                    if task.status == TaskStatus::Pending
                        && task.dependencies_met(&all_tasks)
                    {
                        return Some(task.clone());
                    }
                }
            }
        }
        None
    }

    /// Get workflow status summary
    pub async fn workflow_summary(&self, workflow_id: &str) -> TaskSummary {
        let tasks = self.list_by_workflow(workflow_id).await;
        let total = tasks.len();
        let completed = tasks.iter().filter(|t| t.status == TaskStatus::Completed).count();
        let failed = tasks.iter().filter(|t| t.status == TaskStatus::Failed).count();
        let interrupted = tasks.iter().filter(|t| t.status == TaskStatus::Interrupted).count();
        let in_progress = tasks.iter().filter(|t| t.status == TaskStatus::InProgress).count();
        let pending = tasks.iter().filter(|t| t.status == TaskStatus::Pending || t.status == TaskStatus::Assigned).count();

        TaskSummary {
            workflow_id: workflow_id.into(),
            total,
            completed,
            failed,
            interrupted,
            in_progress,
            pending,
            progress_pct: if total > 0 {
                (completed as f64 / total as f64 * 100.0) as u32
            } else {
                0
            },
        }
    }

    // ── Update ─────────────────────────────────────────────────────────

    /// Update a task (name, description, agent, dependencies)
    pub async fn update(
        &self,
        task_id: &str,
        name: Option<String>,
        description: Option<String>,
        agent: Option<String>,
        dependencies: Option<Vec<String>>,
    ) -> Option<Task> {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            if let Some(n) = name { task.name = n; }
            if let Some(d) = description { task.description = d; }
            if let Some(a) = agent { task.assign(&a); }
            if let Some(deps) = dependencies { task.dependencies = deps; }
            Some(task.clone())
        } else {
            None
        }
    }

    // ── Delete ─────────────────────────────────────────────────────────

    /// Remove a task from the workflow
    pub async fn remove(&self, task_id: &str) -> bool {
        let mut tasks = self.tasks.write().await;
        let removed = tasks.remove(task_id).is_some();

        // Remove from workflow_tasks index
        if removed {
            let mut wt = self.workflow_tasks.write().await;
            for (_, ids) in wt.iter_mut() {
                ids.retain(|id| id != task_id);
            }
        }

        removed
    }

    // ── Dispatch ───────────────────────────────────────────────────────

    /// Assign a task to a specific agent
    pub async fn assign(&self, task_id: &str, agent: &str) -> Result<Task, String> {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            if task.status != TaskStatus::Pending {
                return Err(format!("Task {} is not pending (current: {:?})", task_id, task.status));
            }
            task.assign(agent);
            Ok(task.clone())
        } else {
            Err(format!("Task {} not found", task_id))
        }
    }

    /// Start executing a task
    pub async fn start(&self, task_id: &str) -> Result<Task, String> {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.start()?;
            Ok(task.clone())
        } else {
            Err(format!("Task {} not found", task_id))
        }
    }

    /// Mark task as completed
    pub async fn complete(&self, task_id: &str, result: &str) -> Result<Task, String> {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            if task.status != TaskStatus::InProgress {
                return Err(format!("Task {} is not in progress", task_id));
            }
            task.complete(result);
            Ok(task.clone())
        } else {
            Err(format!("Task {} not found", task_id))
        }
    }

    /// Mark task as failed
    pub async fn fail(&self, task_id: &str, reason: &str) -> Result<Task, String> {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            task.fail(reason);
            Ok(task.clone())
        } else {
            Err(format!("Task {} not found", task_id))
        }
    }

    // ── Interruption ───────────────────────────────────────────────────

    /// Interrupt a task manually
    pub async fn interrupt(&self, task_id: &str, reason: &str) -> Result<Task, String> {
        let mut tasks = self.tasks.write().await;
        if let Some(task) = tasks.get_mut(task_id) {
            if task.status.is_terminal() {
                return Err(format!("Task {} is already terminal ({:?})", task_id, task.status));
            }
            task.interrupt(reason);
            Ok(task.clone())
        } else {
            Err(format!("Task {} not found", task_id))
        }
    }
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}

// ── Task Summary ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct TaskSummary {
    pub workflow_id: String,
    pub total: usize,
    pub completed: usize,
    pub failed: usize,
    pub interrupted: usize,
    pub in_progress: usize,
    pub pending: usize,
    pub progress_pct: u32,
}

// ── TaskDispatcher ──────────────────────────────────────────────────────

use std::sync::Arc;

/// Dispatches tasks to agents using tokio::spawn (green threads).
///
/// Why NOT `std::thread::spawn` (OS threads)?
///   - Tokio green threads are ~1KB each vs OS threads ~2MB
///   - Already running on tokio runtime — no context switch overhead
///   - Can handle 100+ concurrent tasks without thread explosion
///
/// Flow:
///   1. POST /api/tasks/{id}/start
///   2. TaskDispatcher::dispatch(task_id)
///   3. → tokio::spawn(async { run task, update status })
///   4. → Return immediately (202 Accepted)
///   5. Caller polls GET /api/tasks/{id} for status
pub struct TaskDispatcher {
    task_manager: Arc<TaskManager>,
    skills_bus: Arc<crate::skills::SkillsBus>,
}

impl TaskDispatcher {
    pub fn new(task_manager: Arc<TaskManager>, skills_bus: Arc<crate::skills::SkillsBus>) -> Self {
        Self { task_manager, skills_bus }
    }

    /// Dispatch a task — spawns a green thread executing real agent code.
    /// Returns immediately with status=InProgress. Poll for completion.
    pub async fn dispatch(&self, task_id: &str) -> Result<Task, String> {
        let task = self.task_manager.start(task_id).await?;

        let tm = self.task_manager.clone();
        let sb = self.skills_bus.clone();
        let tid = task_id.to_string();
        let agent_name = task.assigned_agent.first().cloned().unwrap_or_default();
        let input = serde_json::json!({
            "task_id": tid,
            "workflow_id": task.workflow_id,
            "name": task.name,
        });

        tokio::spawn(async move {
            tracing::info!("[TaskDispatcher] executing task {tid} via agent '{agent_name}'");

            let gene = crate::GeneProfile::default();

            match sb.execute(&agent_name, input, &gene).await {
                Ok(output) => {
                    let content = output["content"]
                        .as_str()
                        .unwrap_or("completed")
                        .to_string();
                    let _ = tm.complete(&tid, &content).await;
                    tracing::info!("[TaskDispatcher] task {tid} completed: {content}");
                }
                Err(e) => {
                    let _ = tm.fail(&tid, &e.to_string()).await;
                    tracing::error!("[TaskDispatcher] task {tid} failed: {e}");
                }
            }
        });

        Ok(task)
    }

    /// Dispatch with a custom async executor (for production use with real agents)
    pub async fn dispatch_with<F>(&self, task_id: &str, executor: F) -> Result<Task, String>
    where
        F: std::future::Future<Output = Result<String, String>> + Send + 'static,
    {
        let task = self.task_manager.start(task_id).await?;

        let tm = self.task_manager.clone();
        let tid = task_id.to_string();

        tokio::spawn(async move {
            tracing::info!("[TaskDispatcher] executing task {tid}");
            match executor.await {
                Ok(result) => {
                    let _ = tm.complete(&tid, &result).await;
                }
                Err(e) => {
                    let _ = tm.fail(&tid, &e).await;
                }
            }
        });

        Ok(task)
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_task_lifecycle() {
        let tm = TaskManager::new();
        let task = tm.create("wf1", "诊断", "课前诊断", Some("analyst"), vec![]).await;
        assert_eq!(task.status, TaskStatus::Assigned);

        let t = tm.start(&task.id).await.unwrap();
        assert_eq!(t.status, TaskStatus::InProgress);
        assert!(t.started_at.is_some());

        let t = tm.complete(&task.id, "诊断完成").await.unwrap();
        assert_eq!(t.status, TaskStatus::Completed);
        assert!(t.result.unwrap().contains("诊断完成"));
    }

    #[tokio::test]
    async fn test_dependencies() {
        let tm = TaskManager::new();
        let t1 = tm.create("wf1", "task1", "first", None, vec![]).await;
        let t2 = tm.create("wf1", "task2", "depends on task1", None, vec![t1.id.clone()]).await;

        // task2 not ready because task1 is pending
        let next = tm.next_ready("wf1").await.unwrap();
        assert_eq!(next.id, t1.id);

        // Complete task1
        tm.start(&t1.id).await.unwrap();
        tm.complete(&t1.id, "done").await.unwrap();

        // Now task2 should be ready
        let next = tm.next_ready("wf1").await.unwrap();
        assert_eq!(next.id, t2.id);
    }

    #[tokio::test]
    async fn test_interrupt_task() {
        let tm = TaskManager::new();
        let task = tm.create("wf1", "risky", "high risk task", Some("agent"), vec![]).await;
        tm.start(&task.id).await.unwrap();

        let t = tm.interrupt(&task.id, "学生退出").await.unwrap();
        assert_eq!(t.status, TaskStatus::Interrupted);
        assert_eq!(t.interrupt_reason.unwrap(), "学生退出");

        // Cannot interrupt again (already interrupted)
        let err = tm.interrupt(&task.id, "again").await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn test_workflow_summary() {
        let tm = TaskManager::new();
        tm.create("wf1", "t1", "one", Some("a"), vec![]).await;
        let t2 = tm.create("wf1", "t2", "two", Some("b"), vec![]).await;
        tm.create("wf1", "t3", "three", None, vec![]).await;

        tm.start(&t2.id).await.unwrap();
        tm.complete(&t2.id, "ok").await.unwrap();

        let summary = tm.workflow_summary("wf1").await;
        assert_eq!(summary.total, 3);
        assert_eq!(summary.completed, 1);
        assert_eq!(summary.pending, 2); // t1 assigned + t3 pending
        assert_eq!(summary.progress_pct, 33);
    }

    #[tokio::test]
    async fn test_remove_task() {
        let tm = TaskManager::new();
        let t = tm.create("wf1", "remove_me", "unwanted", None, vec![]).await;
        assert_eq!(tm.list_by_workflow("wf1").await.len(), 1);

        assert!(tm.remove(&t.id).await);
        assert_eq!(tm.list_by_workflow("wf1").await.len(), 0);
        assert!(tm.get(&t.id).await.is_none());
    }

    #[tokio::test]
    async fn test_task_update() {
        let tm = TaskManager::new();
        let t = tm.create("wf1", "old_name", "old_desc", None, vec![]).await;

        let updated = tm.update(
            &t.id,
            Some("new_name".into()),
            Some("new_desc".into()),
            Some("new_agent".into()),
            None,
        ).await.unwrap();

        assert_eq!(updated.name, "new_name");
        assert_eq!(updated.description, "new_desc");
        assert!(updated.assigned_agent.contains(&"new_agent".to_string()));
    }

    #[tokio::test]
    async fn test_dispatch_spawns_green_thread() {
        let tm = Arc::new(TaskManager::new());
        let sb = Arc::new(crate::skills::SkillsBus::new());
        let dispatcher = TaskDispatcher::new(tm.clone(), sb);

        let task = tm.create("wf1", "dispatched", "test dispatch", Some("test-agent"), vec![]).await;
        assert_eq!(task.status, TaskStatus::Assigned);

        // Use dispatch_with to inject a real async executor
        let result = dispatcher.dispatch_with(&task.id, async {
            Ok("agent executed successfully".into())
        }).await.unwrap();
        assert_eq!(result.status, TaskStatus::InProgress);

        // Give the green thread time to complete
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let final_task = tm.get(&task.id).await.unwrap();
        assert_eq!(final_task.status, TaskStatus::Completed);
        assert!(final_task.result.unwrap().contains("agent executed"));
    }
}
