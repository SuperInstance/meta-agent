# meta-agent

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Language: Rust](https://img.shields.io/badge/language-Rust-orange.svg)](https://www.rust-lang.org/)
[![SuperInstance](https://img.shields.io/badge/part%20of-SuperInstance-purple.svg)](https://github.com/SuperInstance)

A meta-agent coordinator that dispatches tasks to agents based on capabilities, load, and dependency graphs.

## What It Does

`meta-agent` solves the problem of "who does what" in a multi-agent system. It maintains an `AgentPool` of workers with declared capabilities, a `TaskQueue` with priority levels and dependency constraints, and a `Dispatcher` that assigns ready tasks to the best-fit agent. A `WorkGraph` computes the critical path through the dependency DAG, and a `Simulation` runs the full dispatch-execute cycle to predict makespan, agent utilization, and parallelism.

The conservation law **γ + η = C** applies directly: productive agent time (γ) plus idle/wasted time (η) sums to the total makespan C. The critical path analysis ensures γ is maximized.

## Architecture

```
┌──────────────────────────────────────────────────────┐
│                     Simulation                        │
│  run() → SimulationResult (ticks, utilization, etc.) │
├───────────┬──────────────────┬───────────────────────┤
│ AgentPool │    TaskQueue     │      Dispatcher        │
│ ┌───────┐ │ ┌──────────────┐ │  dispatch_round()      │
│ │Agent  │ │ │Task {        │ │  → Vec<Assignment>     │
│ │ caps  │ │ │  caps, deps, │ │                         │
│ │ load  │ │ │  priority,   │ │  best_agent() → lowest │
│ │ speed │ │ │  state       │ │  load/speed score       │
│ │}      │ │ │}             │ │                         │
│ └───────┘ │ └──────────────┘ │                         │
├───────────┴──────────────────┴───────────────────────┤
│                 WorkGraph (petgraph)                  │
│  build_from_queue() → DAG validation                 │
│  critical_path_weighted() → f64                      │
│  critical_path_nodes() → Vec<String>                 │
├──────────────────────────────────────────────────────┤
│    HealthMonitor         ProgressTracker              │
│  check_health()         update_from_queue()          │
│  suggest_reassignments() overall_completion()        │
└──────────────────────────────────────────────────────┘
```

## Installation

```toml
[dependencies]
meta-agent = { git = "https://github.com/SuperInstance/meta-agent" }
```

Or clone:

```bash
git clone https://github.com/SuperInstance/meta-agent.git
cd meta-agent
cargo build
```

## Usage

### Define agents and tasks

```rust
use meta_agent::*;

let mut pool = AgentPool::new();
pool.add(Agent::new("a1", "Alice")
    .with_capabilities(&["rust", "testing"])
    .with_load(0, 3));  // current 0, max 3 concurrent tasks
pool.add(Agent::new("a2", "Bob")
    .with_capabilities(&["python", "frontend"])
    .with_speed(2.0));  // twice as fast
```

### Create a dependency graph

```rust
let mut queue = TaskQueue::new();
queue.add(Task::new("t1", "Write library")
    .with_capabilities(&["rust"])
    .with_estimated_ticks(5)).unwrap();
queue.add(Task::new("t2", "Write tests")
    .with_capabilities(&["rust", "testing"])
    .with_dependencies(&["t1"])
    .with_estimated_ticks(3)).unwrap();
queue.add(Task::new("t3", "Build dashboard")
    .with_capabilities(&["frontend"])
    .with_estimated_ticks(4)).unwrap();
```

### Run a full simulation

```rust
let mut sim = Simulation::new(/* stuck_threshold */ 10, /* max_ticks */ 100);
sim.add_agent(Agent::new("a1", "Alice").with_capabilities(&["rust"]).with_speed(1.0));
sim.add_agent(Agent::new("a2", "Bob").with_capabilities(&["rust"]).with_speed(1.5));
sim.add_task(Task::new("t1", "Build core").with_capabilities(&["rust"]).with_estimated_ticks(5)).unwrap();
sim.add_task(Task::new("t2", "Tests").with_capabilities(&["rust"]).with_dependencies(&["t1"]).with_estimated_ticks(3)).unwrap();

let result = sim.run().unwrap();
println!("{}", result.report());
// Output: total ticks, critical path, agent utilization %, parallelism factor
```

### Analyze the critical path

```rust
let mut graph = WorkGraph::new();
graph.build_from_queue(&queue).unwrap(); // validates no cycles

let weighted_length = graph.critical_path_weighted(&queue);
let path_nodes = graph.critical_path_nodes(&queue);
// e.g., critical path: t1(5 ticks) → t2(3 ticks) = 8.0 ticks
```

### Monitor agent health

```rust
let monitor = HealthMonitor::new(/* stuck_threshold_ticks */ 10);
let reports = monitor.check_health(&pool, &queue, /* current_tick */ 20);

for r in &reports {
    println!("{}: {} — {}", r.agent_name, r.health, r.recommendation);
}

// Detect stuck agents and suggest reassignment
let suggestions = monitor.suggest_reassignments(&pool, &mut queue, 20);
// Returns Vec<(task_id, Option<alternative_agent_id>)>
```

## API Reference

### `Agent` — Worker with capabilities and load

| Field | Type | Description |
|-------|------|-------------|
| `id` | `String` | Unique identifier |
| `capabilities` | `HashSet<Capability>` | Skills (e.g., "rust", "testing") |
| `current_load` | `u32` | Active task count |
| `max_load` | `u32` | Concurrent task limit (default: 3) |
| `speed` | `f64` | Work rate multiplier (default: 1.0) |

`fitness_score(required)` returns `load_fraction / speed` — lower is a better candidate.

### `Task` — Work unit with dependencies

| Field | Type | Description |
|-------|------|-------------|
| `id` | `String` | Unique identifier |
| `required_capabilities` | `HashSet<Capability>` | Required agent skills |
| `dependencies` | `HashSet<String>` | Tasks that must complete first |
| `priority` | `Priority` | `Low` / `Medium` / `High` / `Critical` |
| `state` | `TaskState` | `Pending` → `Assigned` → `InProgress` → `Completed` |
| `estimated_ticks` | `u32` | Expected duration (default: 1) |

### `Simulation::run()` — Full execution

Returns `SimulationResult` with:
- `total_ticks: u32` — makespan
- `critical_path_weighted: f64` — longest dependency chain in ticks
- `agent_utilization: HashMap<String, f64>` — % busy per agent
- `parallelism: f64` — average concurrent tasks (total work / makespan)
- `health_events: Vec<HealthEvent>` — stuck agent detections
- `completion_order: Vec<String>` — task finish sequence

### `WorkGraph` — Dependency DAG analysis

| Method | Returns | Description |
|--------|---------|-------------|
| `build_from_queue(&mut self, queue)` | `Result<()>` | Build DAG, detect cycles |
| `topological_order(&self)` | `Vec<String>` | Valid execution order |
| `critical_path_weighted(&self, queue)` | `f64` | Longest path by estimated ticks |
| `critical_path_nodes(&self, queue)` | `Vec<String>` | Tasks on the critical path |

## Related Crates (SuperInstance Ecosystem)

- **symplectic-fleet** — Fleet dynamics modeled as Hamiltonian flow, structure-preserving evolution
- **ternary-bus** — Agent communication bus for fleet coordination
- **ternary-sync** — Synchronization primitives for distributed agents
- **ternary-consensus** — Distributed agreement protocols
- **forgemaster** — GPU fleet orchestration backend
- **license-compliance** — License compatibility checking for fleet crates
