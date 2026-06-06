use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// A capability an agent possesses (e.g., "rust", "frontend", "testing").
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Capability(pub String);

impl Capability {
    pub fn new(s: &str) -> Self {
        Self(s.to_lowercase())
    }
}

impl std::fmt::Display for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// An agent in the pool — represents a worker that can execute tasks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub capabilities: HashSet<Capability>,
    #[serde(default)]
    pub current_load: u32,
    #[serde(default = "default_max_load")]
    pub max_load: u32,
    /// How many ticks (time units) this agent takes per unit of work.
    #[serde(default = "default_speed")]
    pub speed: f64,
    /// Path to the agent's workspace for git state inspection.
    pub workspace: Option<String>,
}

fn default_max_load() -> u32 { 3 }
fn default_speed() -> f64 { 1.0 }

impl Agent {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            capabilities: HashSet::new(),
            current_load: 0,
            max_load: 3,
            speed: 1.0,
            workspace: None,
        }
    }

    pub fn with_capabilities(mut self, caps: &[&str]) -> Self {
        self.capabilities = caps.iter().map(|c| Capability::new(c)).collect();
        self
    }

    pub fn with_load(mut self, current: u32, max: u32) -> Self {
        self.current_load = current;
        self.max_load = max;
        self
    }

    pub fn with_speed(mut self, speed: f64) -> Self {
        self.speed = speed;
        self
    }

    pub fn with_workspace(mut self, path: impl Into<String>) -> Self {
        self.workspace = Some(path.into());
        self
    }

    /// Can this agent take on more work?
    pub fn available(&self) -> bool {
        self.current_load < self.max_load
    }

    /// Remaining capacity.
    pub fn remaining_capacity(&self) -> u32 {
        self.max_load.saturating_sub(self.current_load)
    }

    /// Does this agent have the required capabilities?
    pub fn has_capabilities(&self, required: &HashSet<Capability>) -> bool {
        required.iter().all(|c| self.capabilities.contains(c))
    }

    /// Score for load-balancing: lower = better candidate.
    pub fn fitness_score(&self, required: &HashSet<Capability>) -> Option<f64> {
        if !self.has_capabilities(required) || !self.available() {
            return None;
        }
        // Lower load fraction + faster speed = higher fitness (lower score).
        Some((self.current_load as f64 / self.max_load as f64) / self.speed)
    }

    /// Parse from an AGENT.yaml-style config.
    pub fn from_yaml(yaml_str: &str) -> anyhow::Result<Self> {
        Ok(serde_yaml::from_str(yaml_str)?)
    }
}

/// The pool of available agents.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentPool {
    agents: Vec<Agent>,
}

impl AgentPool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, agent: Agent) {
        self.agents.push(agent);
    }

    pub fn get(&self, id: &str) -> Option<&Agent> {
        self.agents.iter().find(|a| a.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Agent> {
        self.agents.iter_mut().find(|a| a.id == id)
    }

    pub fn all(&self) -> &[Agent] {
        &self.agents
    }

    pub fn len(&self) -> usize {
        self.agents.len()
    }

    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }

    /// Find the best agent for a set of required capabilities.
    pub fn best_agent(&self, required: &HashSet<Capability>) -> Option<&Agent> {
        self.agents
            .iter()
            .filter_map(|a| a.fitness_score(required).map(|score| (a, score)))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(a, _)| a)
    }

    /// Find all agents capable of handling the required capabilities.
    pub fn capable_agents(&self, required: &HashSet<Capability>) -> Vec<&Agent> {
        self.agents
            .iter()
            .filter(|a| a.has_capabilities(required) && a.available())
            .collect()
    }

    /// Increment an agent's load (after assignment).
    pub fn assign_load(&mut self, agent_id: &str) -> anyhow::Result<()> {
        let agent = self.agents.iter_mut().find(|a| a.id == agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent {} not found", agent_id))?;
        if agent.current_load >= agent.max_load {
            anyhow::bail!("Agent {} is at max load", agent_id);
        }
        agent.current_load += 1;
        Ok(())
    }

    /// Decrement an agent's load (after task completion).
    pub fn release_load(&mut self, agent_id: &str) -> anyhow::Result<()> {
        let agent = self.agents.iter_mut().find(|a| a.id == agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent {} not found", agent_id))?;
        agent.current_load = agent.current_load.saturating_sub(1);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_creation() {
        let agent = Agent::new("a1", "Alice")
            .with_capabilities(&["rust", "testing"]);
        assert_eq!(agent.id, "a1");
        assert!(agent.has_capabilities(&HashSet::from([
            Capability::new("rust"), Capability::new("testing")
        ])));
        assert!(!agent.has_capabilities(&HashSet::from([Capability::new("python")])));
    }

    #[test]
    fn test_agent_availability() {
        let agent = Agent::new("a1", "Alice").with_load(3, 3);
        assert!(!agent.available());
        assert_eq!(agent.remaining_capacity(), 0);

        let agent2 = Agent::new("a2", "Bob").with_load(1, 3);
        assert!(agent2.available());
        assert_eq!(agent2.remaining_capacity(), 2);
    }

    #[test]
    fn test_pool_best_agent() {
        let mut pool = AgentPool::new();
        pool.add(Agent::new("a1", "Busy Alice")
            .with_capabilities(&["rust"])
            .with_load(2, 3));
        pool.add(Agent::new("a2", "Free Bob")
            .with_capabilities(&["rust"])
            .with_load(0, 3));

        let caps = HashSet::from([Capability::new("rust")]);
        let best = pool.best_agent(&caps).unwrap();
        assert_eq!(best.id, "a2"); // Bob has lower load
    }

    #[test]
    fn test_pool_assign_release() {
        let mut pool = AgentPool::new();
        pool.add(Agent::new("a1", "Alice").with_load(0, 2));

        pool.assign_load("a1").unwrap();
        assert_eq!(pool.get("a1").unwrap().current_load, 1);

        pool.release_load("a1").unwrap();
        assert_eq!(pool.get("a1").unwrap().current_load, 0);
    }

    #[test]
    fn test_fitness_prefers_faster() {
        let slow = Agent::new("slow", "Slow").with_capabilities(&["rust"]).with_speed(1.0).with_load(0, 3);
        let fast = Agent::new("fast", "Fast").with_capabilities(&["rust"]).with_speed(2.0).with_load(0, 3);
        let caps = HashSet::from([Capability::new("rust")]);
        // fitness_score = (load/max) / speed. Lower is better.
        // slow: (0/3)/1.0 = 0.0, fast: (0/3)/2.0 = 0.0 — same load, so speed makes score lower
        // Both have 0 load, so both score 0.0. Check that fast <= slow.
        let slow_score = slow.fitness_score(&caps).unwrap();
        let fast_score = fast.fitness_score(&caps).unwrap();
        assert!(fast_score <= slow_score);

        // With non-zero load the difference shows
        let slow2 = Agent::new("slow2", "Slow").with_capabilities(&["rust"]).with_speed(1.0).with_load(1, 3);
        let fast2 = Agent::new("fast2", "Fast").with_capabilities(&["rust"]).with_speed(2.0).with_load(1, 3);
        assert!(fast2.fitness_score(&caps).unwrap() < slow2.fitness_score(&caps).unwrap());
    }

    #[test]
    fn test_agent_yaml_parse() {
        let yaml = r#"
id: a1
name: Test Agent
capabilities:
  - rust
  - testing
current_load: 0
max_load: 5
speed: 1.5
"#;
        let agent = Agent::from_yaml(yaml).unwrap();
        assert_eq!(agent.id, "a1");
        assert_eq!(agent.max_load, 5);
        assert!((agent.speed - 1.5).abs() < 0.01);
    }
}
