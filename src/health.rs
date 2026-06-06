use crate::agent_pool::AgentPool;
use crate::task_queue::{TaskQueue, TaskState};
use serde::{Deserialize, Serialize};

/// Health status of an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentHealth {
    Healthy,
    Idle,
    Busy,
    Stuck,
    Unknown,
}

impl std::fmt::Display for AgentHealth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentHealth::Healthy => write!(f, "Healthy"),
            AgentHealth::Idle => write!(f, "Idle"),
            AgentHealth::Busy => write!(f, "Busy"),
            AgentHealth::Stuck => write!(f, "Stuck⚠"),
            AgentHealth::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Health report for a single agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthReport {
    pub agent_id: String,
    pub agent_name: String,
    pub health: AgentHealth,
    pub current_load: u32,
    pub last_activity_tick: u32,
    pub recommendation: String,
}

/// Monitors agent health and detects stuck agents.
#[derive(Debug)]
pub struct HealthMonitor {
    /// Number of ticks of inactivity before an agent is considered stuck.
    stuck_threshold: u32,
}

impl HealthMonitor {
    pub fn new(stuck_threshold: u32) -> Self {
        Self { stuck_threshold }
    }

    /// Check health of all agents based on their task states and last activity.
    pub fn check_health(
        &self,
        pool: &AgentPool,
        queue: &TaskQueue,
        current_tick: u32,
    ) -> Vec<HealthReport> {
        let mut reports = Vec::new();

        for agent in pool.all() {
            // Find this agent's in-progress tasks
            let agent_tasks: Vec<_> = queue.all()
                .into_iter()
                .filter(|t| t.assigned_to.as_deref() == Some(&agent.id))
                .collect();

            let (health, last_activity, recommendation) = if agent_tasks.is_empty() {
                if agent.current_load > 0 {
                    // Load says busy but no tasks found — inconsistency
                    (AgentHealth::Unknown, 0, "Verify agent state consistency".to_string())
                } else {
                    (AgentHealth::Idle, 0, "Available for assignment".to_string())
                }
            } else {
                let has_in_progress = agent_tasks.iter().any(|t| t.state == TaskState::InProgress);
                let has_failed = agent_tasks.iter().any(|t| t.state == TaskState::Failed);

                let earliest_start = agent_tasks.iter()
                    .filter_map(|t| t.started_at)
                    .min()
                    .unwrap_or(current_tick);

                let ticks_since_start = current_tick.saturating_sub(earliest_start);

                if has_failed {
                    let failed_tasks: Vec<_> = agent_tasks.iter()
                        .filter(|t| t.state == TaskState::Failed)
                        .map(|t| t.id.clone())
                        .collect();
                    (AgentHealth::Stuck, earliest_start,
                     format!("Tasks failed: {}. Consider reassignment.", failed_tasks.join(", ")))
                } else if has_in_progress && ticks_since_start > self.stuck_threshold {
                    let stuck_tasks: Vec<_> = agent_tasks.iter()
                        .filter(|t| t.state == TaskState::InProgress)
                        .filter(|t| {
                            let elapsed = current_tick.saturating_sub(t.started_at.unwrap_or(current_tick));
                            elapsed > t.estimated_ticks * 2
                        })
                        .map(|t| t.id.clone())
                        .collect();
                    (AgentHealth::Stuck, earliest_start,
                     format!("Tasks overdue: {}. Consider reassignment.", stuck_tasks.join(", ")))
                } else if has_in_progress {
                    (AgentHealth::Busy, earliest_start, "Working normally".to_string())
                } else {
                    (AgentHealth::Idle, 0, "Assigned tasks not yet started".to_string())
                }
            };

            reports.push(HealthReport {
                agent_id: agent.id.clone(),
                agent_name: agent.name.clone(),
                health,
                current_load: agent.current_load,
                last_activity_tick: last_activity,
                recommendation,
            });
        }

        reports
    }

    /// Suggest reassignment for stuck tasks.
    pub fn suggest_reassignments(
        &self,
        pool: &AgentPool,
        queue: &mut TaskQueue,
        current_tick: u32,
    ) -> Vec<(String, Option<String>)> {
        let reports = self.check_health(pool, queue, current_tick);
        let mut suggestions = Vec::new();

        for report in &reports {
            if report.health == AgentHealth::Stuck {
                // Find stuck tasks for this agent
                let stuck_tasks: Vec<_> = queue.all()
                    .into_iter()
                    .filter(|t| t.assigned_to.as_deref() == Some(&report.agent_id))
                    .filter(|t| t.state == TaskState::InProgress || t.state == TaskState::Failed)
                    .collect();

                for task in stuck_tasks {
                    // Find alternative agent
                    let alt = pool.best_agent(&task.required_capabilities);
                    let alt_id = alt.map(|a| a.id.clone());
                    suggestions.push((task.id.clone(), alt_id));
                }
            }
        }

        suggestions
    }

    /// Generate a health report string.
    pub fn report(&self, pool: &AgentPool, queue: &TaskQueue, current_tick: u32) -> String {
        let reports = self.check_health(pool, queue, current_tick);
        let mut lines = vec!["🏥 Agent Health Report".to_string()];

        for r in &reports {
            lines.push(format!("  {} ({}) — {} [load: {}/{}] — {}",
                r.agent_name, r.agent_id, r.health, r.current_load,
                pool.get(&r.agent_id).map(|a| a.max_load).unwrap_or(0),
                r.recommendation));
        }

        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_pool::Agent;
    use crate::task_queue::Task;

    #[test]
    fn test_idle_agent_health() {
        let mut pool = AgentPool::new();
        pool.add(Agent::new("a1", "Alice").with_capabilities(&["rust"]));

        let queue = TaskQueue::new();
        let monitor = HealthMonitor::new(10);
        let reports = monitor.check_health(&pool, &queue, 0);

        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].health, AgentHealth::Idle);
    }

    #[test]
    fn test_busy_agent_health() {
        let mut pool = AgentPool::new();
        pool.add(Agent::new("a1", "Alice").with_capabilities(&["rust"]).with_load(1, 3));

        let mut queue = TaskQueue::new();
        queue.add(Task::new("t1", "Task").with_capabilities(&["rust"])).unwrap();
        queue.assign("t1", "a1").unwrap();
        queue.start("t1", 0).unwrap();

        let monitor = HealthMonitor::new(10);
        let reports = monitor.check_health(&pool, &queue, 3);

        assert_eq!(reports[0].health, AgentHealth::Busy);
    }

    #[test]
    fn test_stuck_agent_detection() {
        let mut pool = AgentPool::new();
        pool.add(Agent::new("a1", "Alice").with_capabilities(&["rust"]));

        let mut queue = TaskQueue::new();
        queue.add(Task::new("t1", "Task").with_capabilities(&["rust"]).with_estimated_ticks(2)).unwrap();
        queue.assign("t1", "a1").unwrap();
        queue.start("t1", 0).unwrap();

        let monitor = HealthMonitor::new(5); // stuck after 5 ticks
        let reports = monitor.check_health(&pool, &queue, 20); // 20 ticks elapsed, task should take 2

        assert_eq!(reports[0].health, AgentHealth::Stuck);
    }

    #[test]
    fn test_suggest_reassignment() {
        let mut pool = AgentPool::new();
        pool.add(Agent::new("a1", "Stuck Alice").with_capabilities(&["rust"]).with_load(1, 3));
        pool.add(Agent::new("a2", "Free Bob").with_capabilities(&["rust"]).with_load(0, 3));

        let mut queue = TaskQueue::new();
        queue.add(Task::new("t1", "Task").with_capabilities(&["rust"]).with_estimated_ticks(2)).unwrap();
        queue.assign("t1", "a1").unwrap();
        queue.start("t1", 0).unwrap();

        let monitor = HealthMonitor::new(5);
        let suggestions = monitor.suggest_reassignments(&pool, &mut queue, 20);

        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].0, "t1");
        assert_eq!(suggestions[0].1.as_deref(), Some("a2"));
    }
}
