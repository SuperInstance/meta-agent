use crate::agent_pool::AgentPool;
use crate::task_queue::{TaskQueue, TaskState};

/// Record of a task-to-agent assignment.
#[derive(Debug, Clone)]
pub struct Assignment {
    pub task_id: String,
    pub task_name: String,
    pub agent_id: String,
    pub agent_name: String,
    pub tick: u32,
    pub estimated_duration: f64,
}

/// Dispatches tasks from the queue to agents in the pool.
#[derive(Debug)]
pub struct Dispatcher {
    assignments: Vec<Assignment>,
}

impl Dispatcher {
    pub fn new() -> Self {
        Self {
            assignments: Vec::new(),
        }
    }

    /// Perform one round of dispatching: assign all ready tasks to available agents.
    /// Returns the list of new assignments made this round.
    pub fn dispatch_round(
        &mut self,
        queue: &mut TaskQueue,
        pool: &mut AgentPool,
        current_tick: u32,
    ) -> Vec<Assignment> {
        let mut new_assignments = Vec::new();

        let ready = queue.prioritized_ready();
        let ready_ids: Vec<String> = ready.iter().map(|t| t.id.clone()).collect();

        for task_id in &ready_ids {
            let task = queue.get(task_id).unwrap().clone();
            let required = task.required_capabilities.clone();

            if let Some(agent) = pool.best_agent(&required) {
                let agent_id = agent.id.clone();
                let agent_name = agent.name.clone();
                let task_name = task.name.clone();
                let speed = agent.speed;
                let est_duration = task.estimated_ticks as f64 / speed;

                if queue.assign(task_id, &agent_id).is_ok() {
                    if let Err(_) = pool.assign_load(&agent_id) {
                        // Should not happen since best_agent checks availability
                        let _ = queue.get_mut(task_id).map(|t| t.state = TaskState::Pending);
                        continue;
                    }

                    let assignment = Assignment {
                        task_id: task_id.clone(),
                        task_name,
                        agent_id: agent_id.clone(),
                        agent_name,
                        tick: current_tick,
                        estimated_duration: est_duration,
                    };

                    new_assignments.push(assignment.clone());
                    self.assignments.push(assignment);
                }
            }
        }

        new_assignments
    }

    /// Get all assignments ever made.
    pub fn all_assignments(&self) -> &[Assignment] {
        &self.assignments
    }

    /// Get assignments for a specific agent.
    pub fn assignments_for_agent(&self, agent_id: &str) -> Vec<&Assignment> {
        self.assignments.iter().filter(|a| a.agent_id == agent_id).collect()
    }

    /// Get the assignment for a specific task.
    pub fn assignment_for_task(&self, task_id: &str) -> Option<&Assignment> {
        self.assignments.iter().find(|a| a.task_id == task_id)
    }

    /// Clear all recorded assignments (for fresh simulation).
    pub fn reset(&mut self) {
        self.assignments.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Agent;
    use crate::task_queue::Task;

    #[test]
    fn test_basic_dispatch() {
        let mut pool = AgentPool::new();
        pool.add(Agent::new("a1", "Alice").with_capabilities(&["rust"]));
        pool.add(Agent::new("a2", "Bob").with_capabilities(&["python"]));

        let mut queue = TaskQueue::new();
        queue.add(Task::new("t1", "Rust task").with_capabilities(&["rust"])).unwrap();
        queue.add(Task::new("t2", "Python task").with_capabilities(&["python"])).unwrap();

        let mut dispatcher = Dispatcher::new();
        let assigned = dispatcher.dispatch_round(&mut queue, &mut pool, 0);

        assert_eq!(assigned.len(), 2);
        assert_eq!(pool.get("a1").unwrap().current_load, 1);
        assert_eq!(pool.get("a2").unwrap().current_load, 1);
    }

    #[test]
    fn test_dispatch_respects_capabilities() {
        let mut pool = AgentPool::new();
        pool.add(Agent::new("a1", "Alice").with_capabilities(&["rust"]));

        let mut queue = TaskQueue::new();
        queue.add(Task::new("t1", "Python task").with_capabilities(&["python"])).unwrap();

        let mut dispatcher = Dispatcher::new();
        let assigned = dispatcher.dispatch_round(&mut queue, &mut pool, 0);

        assert!(assigned.is_empty()); // No capable agent
    }

    #[test]
    fn test_dispatch_respects_dependencies() {
        let mut pool = AgentPool::new();
        pool.add(Agent::new("a1", "Alice").with_capabilities(&["rust"]).with_load(0, 10));

        let mut queue = TaskQueue::new();
        queue.add(Task::new("t1", "First")).unwrap();
        queue.add(Task::new("t2", "Second").with_dependencies(&["t1"])).unwrap();

        let mut dispatcher = Dispatcher::new();
        let assigned = dispatcher.dispatch_round(&mut queue, &mut pool, 0);
        assert_eq!(assigned.len(), 1); // Only t1 is ready
        assert_eq!(assigned[0].task_id, "t1");
    }

    #[test]
    fn test_dispatch_load_balances() {
        let mut pool = AgentPool::new();
        pool.add(Agent::new("a1", "Alice").with_capabilities(&["rust"]).with_load(2, 3));
        pool.add(Agent::new("a2", "Bob").with_capabilities(&["rust"]).with_load(0, 3));

        let mut queue = TaskQueue::new();
        queue.add(Task::new("t1", "Rust task").with_capabilities(&["rust"])).unwrap();

        let mut dispatcher = Dispatcher::new();
        let assigned = dispatcher.dispatch_round(&mut queue, &mut pool, 0);

        assert_eq!(assigned.len(), 1);
        assert_eq!(assigned[0].agent_id, "a2"); // Bob is less loaded
    }
}
