use crate::task_queue::{TaskQueue, TaskState};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Progress info for a single task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskProgress {
    pub task_id: String,
    pub state: TaskState,
    pub assigned_to: Option<String>,
    pub started_at: Option<u32>,
    pub completed_at: Option<u32>,
    pub ticks_remaining: u32,
}

impl TaskProgress {
    pub fn completion_pct(&self) -> f64 {
        match self.state {
            TaskState::Completed => 100.0,
            TaskState::Pending | TaskState::Ready => 0.0,
            TaskState::InProgress | TaskState::Assigned => {
                if let (_start, Some(estimated)) = (self.started_at, Some(self.ticks_remaining + self.elapsed())) {
                    if estimated > 0 {
                        (self.elapsed() as f64 / estimated as f64 * 100.0).min(99.0)
                    } else {
                        50.0
                    }
                } else {
                    0.0
                }
            }
            TaskState::Failed => 0.0,
            TaskState::Blocked => 0.0,
        }
    }

    pub fn elapsed(&self) -> u32 {
        self.started_at.unwrap_or(0)
    }
}

/// Tracks progress by reading task states (simulating git state inspection).
#[derive(Debug, Default)]
pub struct ProgressTracker {
    progress: HashMap<String, TaskProgress>,
}

impl ProgressTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Update progress from the task queue (simulates reading git state).
    pub fn update_from_queue(&mut self, queue: &TaskQueue, current_tick: u32) {
        for task in queue.all() {
            let ticks_remaining = match task.state {
                TaskState::InProgress => task.estimated_ticks.saturating_sub(
                    current_tick.saturating_sub(task.started_at.unwrap_or(current_tick))
                ),
                _ => task.estimated_ticks,
            };

            let progress = TaskProgress {
                task_id: task.id.clone(),
                state: task.state,
                assigned_to: task.assigned_to.clone(),
                started_at: task.started_at,
                completed_at: task.completed_at,
                ticks_remaining,
            };

            self.progress.insert(task.id.clone(), progress);
        }
    }

    /// Get progress for a specific task.
    pub fn get(&self, task_id: &str) -> Option<&TaskProgress> {
        self.progress.get(task_id)
    }

    /// Get overall completion percentage.
    pub fn overall_completion(&self) -> f64 {
        if self.progress.is_empty() {
            return 0.0;
        }
        let total: f64 = self.progress.values()
            .map(|p| p.completion_pct())
            .sum();
        total / self.progress.len() as f64
    }

    /// Get tasks completed so far.
    pub fn completed_count(&self) -> usize {
        self.progress.values()
            .filter(|p| p.state == TaskState::Completed)
            .count()
    }

    /// Get total number of tracked tasks.
    pub fn total_count(&self) -> usize {
        self.progress.len()
    }

    /// Check if a specific agent's task is complete (simulates checking git state).
    pub fn agent_task_complete(&self, agent_id: &str) -> bool {
        self.progress.values()
            .filter(|p| p.assigned_to.as_deref() == Some(agent_id))
            .any(|p| p.state == TaskState::Completed)
    }

    /// Generate a progress report string.
    pub fn report(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!("📊 Progress: {}/{} tasks ({:.1}%)",
            self.completed_count(),
            self.total_count(),
            self.overall_completion()));

        for (id, p) in &self.progress {
            let agent = p.assigned_to.as_deref().unwrap_or("unassigned");
            lines.push(format!("  {} [{}] {} — agent: {}",
                id, p.state, format!("{:.0}%", p.completion_pct()), agent));
        }

        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_queue::Task;

    #[test]
    fn test_progress_update() {
        let mut queue = TaskQueue::new();
        queue.add(Task::new("t1", "Task 1")).unwrap();
        queue.add(Task::new("t2", "Task 2")).unwrap();

        let mut tracker = ProgressTracker::new();
        tracker.update_from_queue(&queue, 0);

        assert_eq!(tracker.total_count(), 2);
        assert_eq!(tracker.completed_count(), 0);
    }

    #[test]
    fn test_completion_after_finish() {
        let mut queue = TaskQueue::new();
        queue.add(Task::new("t1", "Task 1")).unwrap();
        queue.assign("t1", "a1").unwrap();
        queue.start("t1", 0).unwrap();
        queue.complete("t1", 3).unwrap();

        let mut tracker = ProgressTracker::new();
        tracker.update_from_queue(&queue, 5);

        assert_eq!(tracker.completed_count(), 1);
        assert_eq!(tracker.get("t1").unwrap().completion_pct(), 100.0);
    }

    #[test]
    fn test_overall_completion() {
        let mut queue = TaskQueue::new();
        queue.add(Task::new("t1", "Task 1")).unwrap();
        queue.add(Task::new("t2", "Task 2")).unwrap();

        queue.assign("t1", "a1").unwrap();
        queue.start("t1", 0).unwrap();
        queue.complete("t1", 2).unwrap();

        let mut tracker = ProgressTracker::new();
        tracker.update_from_queue(&queue, 3);

        assert_eq!(tracker.overall_completion(), 50.0); // 1 of 2 complete
    }

    #[test]
    fn test_agent_task_complete() {
        let mut queue = TaskQueue::new();
        queue.add(Task::new("t1", "Task 1")).unwrap();
        queue.assign("t1", "a1").unwrap();
        queue.start("t1", 0).unwrap();
        queue.complete("t1", 2).unwrap();

        let mut tracker = ProgressTracker::new();
        tracker.update_from_queue(&queue, 3);

        assert!(tracker.agent_task_complete("a1"));
        assert!(!tracker.agent_task_complete("a2"));
    }
}
