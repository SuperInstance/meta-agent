use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Task priority level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Priority {
    Low = 0,
    Medium = 1,
    High = 2,
    Critical = 3,
}

impl Default for Priority {
    fn default() -> Self {
        Priority::Medium
    }
}

/// Current state of a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TaskState {
    Pending,
    Ready,      // All dependencies satisfied
    Assigned,
    InProgress,
    Completed,
    Failed,
    Blocked,
}

impl std::fmt::Display for TaskState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskState::Pending => write!(f, "Pending"),
            TaskState::Ready => write!(f, "Ready"),
            TaskState::Assigned => write!(f, "Assigned"),
            TaskState::InProgress => write!(f, "InProgress"),
            TaskState::Completed => write!(f, "Completed"),
            TaskState::Failed => write!(f, "Failed"),
            TaskState::Blocked => write!(f, "Blocked"),
        }
    }
}

/// A task to be executed by an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub name: String,
    pub description: String,
    pub required_capabilities: HashSet<crate::agent_pool::Capability>,
    pub dependencies: HashSet<String>,
    pub priority: Priority,
    pub state: TaskState,
    /// Estimated ticks (time units) to complete.
    pub estimated_ticks: u32,
    /// Actual ticks spent (updated during simulation).
    pub actual_ticks: u32,
    /// Agent assigned to this task.
    pub assigned_to: Option<String>,
    /// Tick when the task was started.
    pub started_at: Option<u32>,
    /// Tick when the task was completed.
    pub completed_at: Option<u32>,
}

impl Task {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            required_capabilities: HashSet::new(),
            dependencies: HashSet::new(),
            priority: Priority::default(),
            state: TaskState::Pending,
            estimated_ticks: 1,
            actual_ticks: 0,
            assigned_to: None,
            started_at: None,
            completed_at: None,
        }
    }

    pub fn with_description(mut self, desc: &str) -> Self {
        self.description = desc.to_string();
        self
    }

    pub fn with_capabilities(mut self, caps: &[&str]) -> Self {
        self.required_capabilities = caps.iter()
            .map(|c| crate::agent_pool::Capability::new(c))
            .collect();
        self
    }

    pub fn with_dependencies(mut self, deps: &[&str]) -> Self {
        self.dependencies = deps.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn with_priority(mut self, p: Priority) -> Self {
        self.priority = p;
        self
    }

    pub fn with_estimated_ticks(mut self, ticks: u32) -> Self {
        self.estimated_ticks = ticks;
        self
    }
}

/// A prioritized task queue with dependency tracking.
#[derive(Debug, Clone, Default)]
pub struct TaskQueue {
    tasks: HashMap<String, Task>,
}

impl TaskQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, task: Task) -> anyhow::Result<()> {
        if self.tasks.contains_key(&task.id) {
            anyhow::bail!("Task {} already exists", task.id);
        }
        // Validate dependencies exist
        for dep in &task.dependencies {
            if !self.tasks.contains_key(dep) {
                anyhow::bail!("Dependency {} not found for task {}", dep, task.id);
            }
        }
        self.tasks.insert(task.id.clone(), task);
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&Task> {
        self.tasks.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Task> {
        self.tasks.get_mut(id)
    }

    pub fn all(&self) -> Vec<&Task> {
        self.tasks.values().collect()
    }

    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// Get tasks that are ready (all dependencies completed, not yet assigned).
    pub fn ready_tasks(&self) -> Vec<&Task> {
        let completed: HashSet<&str> = self.tasks.values()
            .filter(|t| t.state == TaskState::Completed)
            .map(|t| t.id.as_str())
            .collect();

        self.tasks.values()
            .filter(|t| t.state == TaskState::Pending)
            .filter(|t| t.dependencies.iter().all(|d| completed.contains(d.as_str())))
            .collect()
    }

    /// Get ready tasks sorted by priority (highest first), then by estimated ticks (shortest first).
    pub fn prioritized_ready(&self) -> Vec<&Task> {
        let mut ready = self.ready_tasks();
        ready.sort_by(|a, b| {
            b.priority.cmp(&a.priority)
                .then_with(|| a.estimated_ticks.cmp(&b.estimated_ticks))
        });
        ready
    }

    /// Mark a task as assigned to an agent.
    pub fn assign(&mut self, task_id: &str, agent_id: &str) -> anyhow::Result<()> {
        let task = self.tasks.get_mut(task_id)
            .ok_or_else(|| anyhow::anyhow!("Task {} not found", task_id))?;
        if task.state != TaskState::Pending {
            anyhow::bail!("Task {} is not pending (state: {})", task_id, task.state);
        }
        task.state = TaskState::Assigned;
        task.assigned_to = Some(agent_id.to_string());
        Ok(())
    }

    /// Mark a task as in progress.
    pub fn start(&mut self, task_id: &str, tick: u32) -> anyhow::Result<()> {
        let task = self.tasks.get_mut(task_id)
            .ok_or_else(|| anyhow::anyhow!("Task {} not found", task_id))?;
        task.state = TaskState::InProgress;
        task.started_at = Some(tick);
        Ok(())
    }

    /// Mark a task as completed.
    pub fn complete(&mut self, task_id: &str, tick: u32) -> anyhow::Result<()> {
        let task = self.tasks.get_mut(task_id)
            .ok_or_else(|| anyhow::anyhow!("Task {} not found", task_id))?;
        task.state = TaskState::Completed;
        task.completed_at = Some(tick);
        Ok(())
    }

    /// Get all tasks in a given state.
    pub fn by_state(&self, state: TaskState) -> Vec<&Task> {
        self.tasks.values().filter(|t| t.state == state).collect()
    }

    /// Validate the task graph has no circular dependencies.
    pub fn validate_no_cycles(&self) -> anyhow::Result<()> {
        let ids: Vec<&str> = self.tasks.keys().map(|s| s.as_str()).collect();
        let mut visited = HashSet::new();
        let mut in_stack = HashSet::new();

        for id in &ids {
            if !visited.contains(*id) {
                self.dfs_check(*id, &mut visited, &mut in_stack)?;
            }
        }
        Ok(())
    }

    fn dfs_check<'a>(
        &'a self,
        id: &'a str,
        visited: &mut HashSet<&'a str>,
        in_stack: &mut HashSet<&'a str>,
    ) -> anyhow::Result<()> {
        visited.insert(id);
        in_stack.insert(id);

        if let Some(task) = self.tasks.get(id) {
            for dep in &task.dependencies {
                if !visited.contains(dep.as_str()) {
                    self.dfs_check(dep, visited, in_stack)?;
                } else if in_stack.contains(dep.as_str()) {
                    anyhow::bail!("Circular dependency detected involving task {}", id);
                }
            }
        }

        in_stack.remove(id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_and_get() {
        let mut q = TaskQueue::new();
        q.add(Task::new("t1", "Task 1")).unwrap();
        assert_eq!(q.get("t1").unwrap().name, "Task 1");
    }

    #[test]
    fn test_duplicate_task_rejected() {
        let mut q = TaskQueue::new();
        q.add(Task::new("t1", "Task 1")).unwrap();
        assert!(q.add(Task::new("t1", "Task 1 Again")).is_err());
    }

    #[test]
    fn test_dependency_validation() {
        let mut q = TaskQueue::new();
        assert!(q.add(Task::new("t2", "Task 2").with_dependencies(&["t1"])).is_err());
    }

    #[test]
    fn test_ready_tasks_with_dependencies() {
        let mut q = TaskQueue::new();
        q.add(Task::new("t1", "First")).unwrap();
        q.add(Task::new("t2", "Second").with_dependencies(&["t1"])).unwrap();
        q.add(Task::new("t3", "Third (no deps)")).unwrap();

        let ready = q.ready_tasks();
        assert_eq!(ready.len(), 2); // t1 and t3
        assert!(ready.iter().any(|t| t.id == "t1"));
        assert!(ready.iter().any(|t| t.id == "t3"));
    }

    #[test]
    fn test_prioritized_ready_sorts() {
        let mut q = TaskQueue::new();
        q.add(Task::new("t1", "Low").with_priority(Priority::Low)).unwrap();
        q.add(Task::new("t2", "Critical").with_priority(Priority::Critical)).unwrap();
        q.add(Task::new("t3", "High").with_priority(Priority::High)).unwrap();

        let ready = q.prioritized_ready();
        assert_eq!(ready[0].id, "t2");
        assert_eq!(ready[1].id, "t3");
        assert_eq!(ready[2].id, "t1");
    }

    #[test]
    fn test_state_transitions() {
        let mut q = TaskQueue::new();
        q.add(Task::new("t1", "Task")).unwrap();
        q.assign("t1", "a1").unwrap();
        assert_eq!(q.get("t1").unwrap().state, TaskState::Assigned);
        assert_eq!(q.get("t1").unwrap().assigned_to.as_deref(), Some("a1"));

        q.start("t1", 5).unwrap();
        assert_eq!(q.get("t1").unwrap().state, TaskState::InProgress);
        assert_eq!(q.get("t1").unwrap().started_at, Some(5));

        q.complete("t1", 10).unwrap();
        assert_eq!(q.get("t1").unwrap().state, TaskState::Completed);
        assert_eq!(q.get("t1").unwrap().completed_at, Some(10));
    }

    #[test]
    fn test_cycle_detection() {
        let mut q = TaskQueue::new();
        q.add(Task::new("t1", "First")).unwrap();
        q.add(Task::new("t2", "Second").with_dependencies(&["t1"])).unwrap();
        // t1 depends on t2 — but we can't add t1 with dep on t2 before t2 exists.
        // Test that a valid graph passes validation.
        assert!(q.validate_no_cycles().is_ok());
    }
}
