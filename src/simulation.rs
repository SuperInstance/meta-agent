use crate::agent_pool::{AgentPool, Agent};
use crate::task_queue::{TaskQueue, Task, TaskState};
use crate::dispatcher::Dispatcher;
use crate::work_graph::WorkGraph;
use crate::progress::ProgressTracker;
use crate::health::{HealthMonitor, AgentHealth};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Result of running a simulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationResult {
    pub total_tasks: usize,
    pub total_agents: usize,
    pub total_ticks: u32,
    pub critical_path_weighted: f64,
    pub critical_path_nodes: Vec<String>,
    pub assignment_order: Vec<AssignmentRecord>,
    pub agent_utilization: HashMap<String, f64>,
    pub completion_order: Vec<String>,
    pub health_events: Vec<HealthEvent>,
    pub parallelism: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignmentRecord {
    pub tick: u32,
    pub task_id: String,
    pub task_name: String,
    pub agent_id: String,
    pub agent_name: String,
    pub estimated_duration: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthEvent {
    pub tick: u32,
    pub agent_id: String,
    pub agent_name: String,
    pub health: String,
    pub note: String,
}

/// Runs a full simulation of task assignment and execution.
pub struct Simulation {
    pool: AgentPool,
    queue: TaskQueue,
    dispatcher: Dispatcher,
    work_graph: WorkGraph,
    progress: ProgressTracker,
    health_monitor: HealthMonitor,
    stuck_threshold: u32,
    max_ticks: u32,
}

impl Simulation {
    pub fn new(stuck_threshold: u32, max_ticks: u32) -> Self {
        Self {
            pool: AgentPool::new(),
            queue: TaskQueue::new(),
            dispatcher: Dispatcher::new(),
            work_graph: WorkGraph::new(),
            progress: ProgressTracker::new(),
            health_monitor: HealthMonitor::new(stuck_threshold),
            stuck_threshold,
            max_ticks,
        }
    }

    pub fn add_agent(&mut self, agent: Agent) {
        self.pool.add(agent);
    }

    pub fn add_task(&mut self, task: Task) -> anyhow::Result<()> {
        self.queue.add(task)
    }

    /// Run the simulation and return results.
    pub fn run(&mut self) -> anyhow::Result<SimulationResult> {
        self.work_graph.build_from_queue(&self.queue)?;
        self.queue.validate_no_cycles()?;

        let critical_path_weighted = self.work_graph.critical_path_weighted(&self.queue);
        let critical_path_nodes = self.work_graph.critical_path_nodes(&self.queue);

        let mut assignment_order = Vec::new();
        let mut completion_order = Vec::new();
        let mut health_events = Vec::new();

        // Track which tasks are currently executing and when they'll finish
        let mut executing: HashMap<String, (String, u32)> = HashMap::new(); // task_id -> (agent_id, finish_tick)

        let mut tick = 0u32;

        while tick < self.max_ticks {
            // 1. Complete tasks that finished this tick
            let finishing: Vec<_> = executing.iter()
                .filter(|(_, (_, finish))| *finish <= tick)
                .map(|(task_id, _)| task_id.clone())
                .collect();

            for task_id in finishing {
                if let Some((agent_id, _)) = executing.remove(&task_id) {
                    self.queue.complete(&task_id, tick)?;
                    self.pool.release_load(&agent_id)?;
                    completion_order.push(task_id);
                }
            }

            // 2. Check if all done
            let completed_count = self.queue.by_state(TaskState::Completed).len();
            if completed_count == self.queue.len() {
                break;
            }

            // 3. Dispatch new tasks
            let new_assignments = self.dispatcher.dispatch_round(&mut self.queue, &mut self.pool, tick);

            for assignment in &new_assignments {
                let task = self.queue.get(&assignment.task_id).unwrap();
                let agent = self.pool.get(&assignment.agent_id).unwrap();
                let finish_tick = tick + (task.estimated_ticks as f64 / agent.speed).ceil() as u32;

                self.queue.start(&assignment.task_id, tick)?;
                executing.insert(assignment.task_id.clone(), (assignment.agent_id.clone(), finish_tick));

                assignment_order.push(AssignmentRecord {
                    tick,
                    task_id: assignment.task_id.clone(),
                    task_name: assignment.task_name.clone(),
                    agent_id: assignment.agent_id.clone(),
                    agent_name: assignment.agent_name.clone(),
                    estimated_duration: assignment.estimated_duration,
                });
            }

            // 4. Health check every 5 ticks
            if tick % 5 == 0 {
                let reports = self.health_monitor.check_health(&self.pool, &self.queue, tick);
                for r in &reports {
                    if r.health == AgentHealth::Stuck {
                        health_events.push(HealthEvent {
                            tick,
                            agent_id: r.agent_id.clone(),
                            agent_name: r.agent_name.clone(),
                            health: format!("{}", r.health),
                            note: r.recommendation.clone(),
                        });
                    }
                }
            }

            // 5. Update progress
            self.progress.update_from_queue(&self.queue, tick);

            tick += 1;
        }

        // Calculate agent utilization
        let total_ticks = tick as f64;
        let mut agent_utilization = HashMap::new();
        for agent in self.pool.all() {
            let agent_tasks: Vec<_> = assignment_order.iter()
                .filter(|a| a.agent_id == agent.id)
                .collect();
            let busy_ticks: f64 = agent_tasks.iter()
                .map(|a| a.estimated_duration)
                .sum();
            agent_utilization.insert(agent.id.clone(), if total_ticks > 0.0 {
                (busy_ticks / total_ticks * 100.0).min(100.0)
            } else {
                0.0
            });
        }

        // Calculate average parallelism
        let parallelism = if tick > 0 {
            let total_work: f64 = self.queue.all().iter()
                .map(|t| t.estimated_ticks as f64)
                .sum();
            total_work / tick as f64
        } else {
            0.0
        };

        Ok(SimulationResult {
            total_tasks: self.queue.len(),
            total_agents: self.pool.len(),
            total_ticks: tick,
            critical_path_weighted,
            critical_path_nodes,
            assignment_order,
            agent_utilization,
            completion_order,
            health_events,
            parallelism,
        })
    }
}

impl SimulationResult {
    /// Format a human-readable report.
    pub fn report(&self) -> String {
        let mut lines = Vec::new();

        lines.push("═══════════════════════════════════════".to_string());
        lines.push("  META-AGENT SIMULATION RESULTS".to_string());
        lines.push("═══════════════════════════════════════".to_string());
        lines.push(format!("  Tasks: {}  |  Agents: {}  |  Ticks: {}",
            self.total_tasks, self.total_agents, self.total_ticks));
        lines.push(format!("  Critical Path (weighted): {:.1} ticks", self.critical_path_weighted));
        lines.push(format!("  Critical Path: {}",
            self.critical_path_nodes.join(" → ")));
        lines.push(format!("  Avg Parallelism: {:.2}x", self.parallelism));
        lines.push(String::new());

        lines.push("─── Assignment Order ───".to_string());
        for a in &self.assignment_order {
            lines.push(format!("  Tick {:>3}: {} → {} ({:.1} ticks est.)",
                a.tick, a.task_id, a.agent_name, a.estimated_duration));
        }
        lines.push(String::new());

        lines.push("─── Completion Order ───".to_string());
        for (i, id) in self.completion_order.iter().enumerate() {
            lines.push(format!("  {}. {}", i + 1, id));
        }
        lines.push(String::new());

        lines.push("─── Agent Utilization ───".to_string());
        let mut agents: Vec<_> = self.agent_utilization.iter().collect();
        agents.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
        for (id, util) in &agents {
            lines.push(format!("  {}: {:.1}%", id, util));
        }
        lines.push(String::new());

        if !self.health_events.is_empty() {
            lines.push("─── Health Events ───".to_string());
            for e in &self.health_events {
                lines.push(format!("  Tick {}: {} — {} — {}", e.tick, e.agent_name, e.health, e.note));
            }
        } else {
            lines.push("─── Health: All agents healthy ✓ ───".to_string());
        }

        lines.push("═══════════════════════════════════════".to_string());
        lines.join("\n")
    }
}
