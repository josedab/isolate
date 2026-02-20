//! Workflow Orchestration
//!
//! Multi-sandbox pipelines and DAG execution:
//! - Define complex workflows as directed acyclic graphs
//! - Parallel and sequential execution
//! - Conditional branching and error handling
//! - Data passing between sandboxes
//! - Retry and timeout policies

#![allow(missing_docs)]
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, SystemTime};

pub mod executor;
pub mod pipeline;

/// Unique workflow ID.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct WorkflowId(pub String);

impl WorkflowId {
    /// Generate a new workflow ID.
    pub fn generate() -> Self {
        Self(generate_id("wf"))
    }
}

/// Workflow definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    /// Workflow ID.
    pub id: WorkflowId,
    /// Workflow name.
    pub name: String,
    /// Description.
    pub description: Option<String>,
    /// Steps in the workflow.
    pub steps: HashMap<String, Step>,
    /// Entry step.
    pub entry: String,
    /// Global timeout.
    pub timeout: Option<Duration>,
    /// Retry policy.
    pub retry_policy: Option<RetryPolicy>,
    /// On failure handler.
    pub on_failure: Option<String>,
    /// Metadata.
    pub metadata: HashMap<String, String>,
}

impl Workflow {
    /// Create a new workflow.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: WorkflowId::generate(),
            name: name.into(),
            description: None,
            steps: HashMap::new(),
            entry: String::new(),
            timeout: None,
            retry_policy: None,
            on_failure: None,
            metadata: HashMap::new(),
        }
    }

    /// Add a step.
    pub fn add_step(&mut self, name: impl Into<String>, step: Step) {
        let name = name.into();
        if self.entry.is_empty() {
            self.entry = name.clone();
        }
        self.steps.insert(name, step);
    }

    /// Set entry step.
    pub fn set_entry(&mut self, name: impl Into<String>) {
        self.entry = name.into();
    }

    /// Validate workflow.
    pub fn validate(&self) -> Result<(), WorkflowError> {
        if self.steps.is_empty() {
            return Err(WorkflowError::EmptyWorkflow);
        }

        if !self.steps.contains_key(&self.entry) {
            return Err(WorkflowError::InvalidEntry(self.entry.clone()));
        }

        // Check for cycles
        if self.has_cycle() {
            return Err(WorkflowError::CycleDetected);
        }

        // Check all dependencies exist
        for (name, step) in &self.steps {
            for dep in &step.depends_on {
                if !self.steps.contains_key(dep) {
                    return Err(WorkflowError::InvalidDependency {
                        step: name.clone(),
                        dependency: dep.clone(),
                    });
                }
            }
        }

        Ok(())
    }

    fn has_cycle(&self) -> bool {
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();

        for step_name in self.steps.keys() {
            if self.has_cycle_util(step_name, &mut visited, &mut rec_stack) {
                return true;
            }
        }

        false
    }

    fn has_cycle_util(
        &self,
        step: &str,
        visited: &mut HashSet<String>,
        rec_stack: &mut HashSet<String>,
    ) -> bool {
        if rec_stack.contains(step) {
            return true;
        }

        if visited.contains(step) {
            return false;
        }

        visited.insert(step.to_string());
        rec_stack.insert(step.to_string());

        if let Some(s) = self.steps.get(step) {
            for next in &s.next {
                if self.has_cycle_util(next, visited, rec_stack) {
                    return true;
                }
            }
        }

        rec_stack.remove(step);
        false
    }

    /// Get execution order (topological sort).
    pub fn execution_order(&self) -> Vec<Vec<String>> {
        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();

        // Initialize
        for name in self.steps.keys() {
            in_degree.insert(name, 0);
            dependents.insert(name, Vec::new());
        }

        // Calculate in-degrees
        for (name, step) in &self.steps {
            for dep in &step.depends_on {
                if let Some(count) = in_degree.get_mut(name.as_str()) {
                    *count += 1;
                }
                if let Some(deps) = dependents.get_mut(dep.as_str()) {
                    deps.push(name);
                }
            }
        }

        // Kahn's algorithm
        let mut result = Vec::new();
        let mut queue: VecDeque<&str> =
            in_degree.iter().filter(|(_, &deg)| deg == 0).map(|(&name, _)| name).collect();

        while !queue.is_empty() {
            let level: Vec<String> = queue.drain(..).map(|s| s.to_string()).collect();

            for name in &level {
                if let Some(deps) = dependents.get(name.as_str()) {
                    for dep in deps {
                        if let Some(count) = in_degree.get_mut(*dep) {
                            *count -= 1;
                            if *count == 0 {
                                queue.push_back(*dep);
                            }
                        }
                    }
                }
            }

            result.push(level);
        }

        result
    }
}

/// Workflow step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    /// Step type.
    pub step_type: StepType,
    /// Dependencies (steps that must complete first).
    pub depends_on: Vec<String>,
    /// Next steps to execute.
    pub next: Vec<String>,
    /// Condition for execution.
    pub condition: Option<Condition>,
    /// Retry policy override.
    pub retry_policy: Option<RetryPolicy>,
    /// Timeout override.
    pub timeout: Option<Duration>,
    /// Input mappings.
    pub inputs: HashMap<String, InputMapping>,
    /// Output mappings.
    pub outputs: HashMap<String, String>,
}

impl Step {
    /// Create a sandbox step.
    pub fn sandbox(sandbox_config: SandboxStepConfig) -> Self {
        Self {
            step_type: StepType::Sandbox(sandbox_config),
            depends_on: Vec::new(),
            next: Vec::new(),
            condition: None,
            retry_policy: None,
            timeout: None,
            inputs: HashMap::new(),
            outputs: HashMap::new(),
        }
    }

    /// Create a parallel step.
    pub fn parallel(branches: Vec<String>) -> Self {
        Self {
            step_type: StepType::Parallel { branches },
            depends_on: Vec::new(),
            next: Vec::new(),
            condition: None,
            retry_policy: None,
            timeout: None,
            inputs: HashMap::new(),
            outputs: HashMap::new(),
        }
    }

    /// Create a choice step.
    pub fn choice(choices: Vec<Choice>) -> Self {
        Self {
            step_type: StepType::Choice { choices },
            depends_on: Vec::new(),
            next: Vec::new(),
            condition: None,
            retry_policy: None,
            timeout: None,
            inputs: HashMap::new(),
            outputs: HashMap::new(),
        }
    }

    /// Add dependency.
    pub fn depends_on(mut self, step: impl Into<String>) -> Self {
        self.depends_on.push(step.into());
        self
    }

    /// Add next step.
    pub fn then(mut self, step: impl Into<String>) -> Self {
        self.next.push(step.into());
        self
    }
}

/// Step type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StepType {
    /// Execute a sandbox.
    Sandbox(SandboxStepConfig),
    /// Execute steps in parallel.
    Parallel { branches: Vec<String> },
    /// Conditional branching.
    Choice { choices: Vec<Choice> },
    /// Wait for duration.
    Wait { duration: Duration },
    /// Pass through (no-op, for data transformation).
    Pass,
    /// Terminate workflow.
    End { success: bool },
    /// Sub-workflow.
    SubWorkflow { workflow_id: String },
}

/// Sandbox step configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxStepConfig {
    /// Module reference.
    pub module: String,
    /// Function to call.
    pub function: Option<String>,
    /// Resource limits.
    pub memory_limit: Option<u64>,
    /// Fuel limit.
    pub fuel_limit: Option<u64>,
    /// Capabilities.
    pub capabilities: Vec<String>,
}

/// Choice for conditional branching.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    /// Condition to evaluate.
    pub condition: Condition,
    /// Step to execute if condition is true.
    pub next: String,
}

/// Condition for branching.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Condition {
    /// Check if value equals.
    Equals { variable: String, value: serde_json::Value },
    /// Check if value is greater than.
    GreaterThan { variable: String, value: f64 },
    /// Check if value is less than.
    LessThan { variable: String, value: f64 },
    /// Check if string contains.
    Contains { variable: String, substring: String },
    /// Logical AND.
    And(Vec<Condition>),
    /// Logical OR.
    Or(Vec<Condition>),
    /// Logical NOT.
    Not(Box<Condition>),
    /// Always true.
    Always,
}

impl Condition {
    /// Evaluate condition against context.
    pub fn evaluate(&self, context: &ExecutionContext) -> bool {
        match self {
            Condition::Equals { variable, value } => {
                context.get_variable(variable).map(|v| v == value).unwrap_or(false)
            }
            Condition::GreaterThan { variable, value } => context
                .get_variable(variable)
                .and_then(|v| v.as_f64())
                .map(|v| v > *value)
                .unwrap_or(false),
            Condition::LessThan { variable, value } => context
                .get_variable(variable)
                .and_then(|v| v.as_f64())
                .map(|v| v < *value)
                .unwrap_or(false),
            Condition::Contains { variable, substring } => context
                .get_variable(variable)
                .and_then(|v| v.as_str())
                .map(|v| v.contains(substring))
                .unwrap_or(false),
            Condition::And(conditions) => conditions.iter().all(|c| c.evaluate(context)),
            Condition::Or(conditions) => conditions.iter().any(|c| c.evaluate(context)),
            Condition::Not(condition) => !condition.evaluate(context),
            Condition::Always => true,
        }
    }
}

/// Input mapping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InputMapping {
    /// Direct value.
    Value(serde_json::Value),
    /// From previous step output.
    FromStep { step: String, output: String },
    /// From workflow input.
    FromInput(String),
    /// Expression.
    Expression(String),
}

/// Retry policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Maximum retry attempts.
    pub max_attempts: u32,
    /// Initial delay.
    pub initial_delay: Duration,
    /// Maximum delay.
    pub max_delay: Duration,
    /// Backoff multiplier.
    pub backoff_multiplier: f64,
    /// Retryable errors.
    pub retryable_errors: Vec<String>,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            backoff_multiplier: 2.0,
            retryable_errors: vec!["TIMEOUT".to_string(), "RESOURCE_EXHAUSTED".to_string()],
        }
    }
}

/// Execution context.
#[derive(Debug, Clone, Default)]
pub struct ExecutionContext {
    /// Workflow ID.
    pub workflow_id: WorkflowId,
    /// Run ID.
    pub run_id: String,
    /// Variables.
    pub variables: HashMap<String, serde_json::Value>,
    /// Step outputs.
    pub step_outputs: HashMap<String, HashMap<String, serde_json::Value>>,
}

impl ExecutionContext {
    /// Create new context.
    pub fn new(workflow_id: WorkflowId) -> Self {
        Self {
            workflow_id,
            run_id: generate_id("run"),
            variables: HashMap::new(),
            step_outputs: HashMap::new(),
        }
    }

    /// Get variable.
    pub fn get_variable(&self, name: &str) -> Option<&serde_json::Value> {
        // Check step outputs first (format: "step_name.output_name")
        if let Some((step, output)) = name.split_once('.') {
            return self.step_outputs.get(step)?.get(output);
        }
        self.variables.get(name)
    }

    /// Set variable.
    pub fn set_variable(&mut self, name: impl Into<String>, value: serde_json::Value) {
        self.variables.insert(name.into(), value);
    }

    /// Set step output.
    pub fn set_step_output(
        &mut self,
        step: impl Into<String>,
        output: impl Into<String>,
        value: serde_json::Value,
    ) {
        self.step_outputs.entry(step.into()).or_default().insert(output.into(), value);
    }
}

/// Workflow execution state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowExecution {
    /// Run ID.
    pub run_id: String,
    /// Workflow ID.
    pub workflow_id: WorkflowId,
    /// Status.
    pub status: ExecutionStatus,
    /// Current step.
    pub current_step: Option<String>,
    /// Step statuses.
    pub step_statuses: HashMap<String, StepStatus>,
    /// Started at.
    pub started_at: SystemTime,
    /// Completed at.
    pub completed_at: Option<SystemTime>,
    /// Error.
    pub error: Option<String>,
}

/// Execution status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionStatus {
    Pending,
    Running,
    Paused,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
}

/// Step status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepStatus {
    /// Status.
    pub status: ExecutionStatus,
    /// Attempt count.
    pub attempts: u32,
    /// Started at.
    pub started_at: Option<SystemTime>,
    /// Completed at.
    pub completed_at: Option<SystemTime>,
    /// Duration.
    pub duration: Option<Duration>,
    /// Error.
    pub error: Option<String>,
}

impl Default for StepStatus {
    fn default() -> Self {
        Self {
            status: ExecutionStatus::Pending,
            attempts: 0,
            started_at: None,
            completed_at: None,
            duration: None,
            error: None,
        }
    }
}

/// Workflow executor.
pub struct WorkflowExecutor {
    workflows: HashMap<WorkflowId, Workflow>,
    executions: HashMap<String, WorkflowExecution>,
}

impl Default for WorkflowExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkflowExecutor {
    /// Create new executor.
    pub fn new() -> Self {
        Self { workflows: HashMap::new(), executions: HashMap::new() }
    }

    /// Register a workflow.
    pub fn register(&mut self, workflow: Workflow) -> Result<(), WorkflowError> {
        workflow.validate()?;
        self.workflows.insert(workflow.id.clone(), workflow);
        Ok(())
    }

    /// Start workflow execution.
    pub fn start(
        &mut self,
        workflow_id: &WorkflowId,
        inputs: HashMap<String, serde_json::Value>,
    ) -> Result<String, WorkflowError> {
        let workflow = self
            .workflows
            .get(workflow_id)
            .ok_or_else(|| WorkflowError::WorkflowNotFound(workflow_id.0.clone()))?;

        let run_id = generate_id("run");
        let mut step_statuses = HashMap::new();

        for name in workflow.steps.keys() {
            step_statuses.insert(name.clone(), StepStatus::default());
        }

        let execution = WorkflowExecution {
            run_id: run_id.clone(),
            workflow_id: workflow_id.clone(),
            status: ExecutionStatus::Running,
            current_step: Some(workflow.entry.clone()),
            step_statuses,
            started_at: SystemTime::now(),
            completed_at: None,
            error: None,
        };

        self.executions.insert(run_id.clone(), execution);

        // In production, would actually execute steps
        // For now, simulate completion
        if let Some(exec) = self.executions.get_mut(&run_id) {
            exec.status = ExecutionStatus::Completed;
            exec.completed_at = Some(SystemTime::now());

            for (_, status) in exec.step_statuses.iter_mut() {
                status.status = ExecutionStatus::Completed;
                status.attempts = 1;
                status.started_at = Some(SystemTime::now());
                status.completed_at = Some(SystemTime::now());
            }
        }

        let _ = inputs; // Would use inputs in real execution

        Ok(run_id)
    }

    /// Get execution status.
    pub fn get_execution(&self, run_id: &str) -> Option<&WorkflowExecution> {
        self.executions.get(run_id)
    }

    /// Cancel execution.
    pub fn cancel(&mut self, run_id: &str) -> Result<(), WorkflowError> {
        let execution = self
            .executions
            .get_mut(run_id)
            .ok_or_else(|| WorkflowError::ExecutionNotFound(run_id.to_string()))?;

        execution.status = ExecutionStatus::Cancelled;
        execution.completed_at = Some(SystemTime::now());
        Ok(())
    }

    /// List executions for workflow.
    pub fn list_executions(&self, workflow_id: &WorkflowId) -> Vec<&WorkflowExecution> {
        self.executions.values().filter(|e| &e.workflow_id == workflow_id).collect()
    }
}

/// Workflow error.
#[derive(Debug, Clone)]
pub enum WorkflowError {
    /// Empty workflow.
    EmptyWorkflow,
    /// Invalid entry step.
    InvalidEntry(String),
    /// Cycle detected.
    CycleDetected,
    /// Invalid dependency.
    InvalidDependency { step: String, dependency: String },
    /// Workflow not found.
    WorkflowNotFound(String),
    /// Execution not found.
    ExecutionNotFound(String),
    /// Step failed.
    StepFailed { step: String, error: String },
    /// Timeout.
    Timeout,
}

impl std::fmt::Display for WorkflowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyWorkflow => write!(f, "Workflow has no steps"),
            Self::InvalidEntry(s) => write!(f, "Invalid entry step: {}", s),
            Self::CycleDetected => write!(f, "Cycle detected in workflow"),
            Self::InvalidDependency { step, dependency } => {
                write!(f, "Step {} has invalid dependency: {}", step, dependency)
            }
            Self::WorkflowNotFound(id) => write!(f, "Workflow not found: {}", id),
            Self::ExecutionNotFound(id) => write!(f, "Execution not found: {}", id),
            Self::StepFailed { step, error } => write!(f, "Step {} failed: {}", step, error),
            Self::Timeout => write!(f, "Workflow timed out"),
        }
    }
}

impl std::error::Error for WorkflowError {}

/// Builder for workflows.
pub struct WorkflowBuilder {
    workflow: Workflow,
}

impl WorkflowBuilder {
    /// Create new builder.
    pub fn new(name: impl Into<String>) -> Self {
        Self { workflow: Workflow::new(name) }
    }

    /// Set description.
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.workflow.description = Some(desc.into());
        self
    }

    /// Add step.
    pub fn step(mut self, name: impl Into<String>, step: Step) -> Self {
        self.workflow.add_step(name, step);
        self
    }

    /// Set entry.
    pub fn entry(mut self, name: impl Into<String>) -> Self {
        self.workflow.set_entry(name);
        self
    }

    /// Set timeout.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.workflow.timeout = Some(timeout);
        self
    }

    /// Set retry policy.
    pub fn retry(mut self, policy: RetryPolicy) -> Self {
        self.workflow.retry_policy = Some(policy);
        self
    }

    /// Build workflow.
    pub fn build(self) -> Result<Workflow, WorkflowError> {
        self.workflow.validate()?;
        Ok(self.workflow)
    }
}

fn generate_id(prefix: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let mut hasher = DefaultHasher::new();
    SystemTime::now().hash(&mut hasher);
    COUNTER.fetch_add(1, Ordering::Relaxed).hash(&mut hasher);
    format!("{}-{:016x}", prefix, hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workflow_creation() {
        let workflow = Workflow::new("test-workflow");
        assert_eq!(workflow.name, "test-workflow");
    }

    #[test]
    fn test_workflow_builder() {
        let workflow = WorkflowBuilder::new("pipeline")
            .description("Test pipeline")
            .step(
                "start",
                Step::sandbox(SandboxStepConfig {
                    module: "processor".to_string(),
                    function: Some("process".to_string()),
                    memory_limit: Some(128 * 1024 * 1024),
                    fuel_limit: Some(1_000_000),
                    capabilities: vec!["network".to_string()],
                })
                .then("end"),
            )
            .step(
                "end",
                Step {
                    step_type: StepType::End { success: true },
                    depends_on: vec!["start".to_string()],
                    next: vec![],
                    condition: None,
                    retry_policy: None,
                    timeout: None,
                    inputs: HashMap::new(),
                    outputs: HashMap::new(),
                },
            )
            .timeout(Duration::from_secs(300))
            .build()
            .unwrap();

        assert_eq!(workflow.steps.len(), 2);
    }

    #[test]
    fn test_workflow_validation_empty() {
        let workflow = Workflow::new("empty");
        assert!(matches!(workflow.validate(), Err(WorkflowError::EmptyWorkflow)));
    }

    #[test]
    fn test_workflow_validation_invalid_entry() {
        let mut workflow = Workflow::new("test");
        workflow.add_step(
            "step1",
            Step {
                step_type: StepType::Pass,
                depends_on: vec![],
                next: vec![],
                condition: None,
                retry_policy: None,
                timeout: None,
                inputs: HashMap::new(),
                outputs: HashMap::new(),
            },
        );
        workflow.entry = "nonexistent".to_string();

        assert!(matches!(workflow.validate(), Err(WorkflowError::InvalidEntry(_))));
    }

    #[test]
    fn test_workflow_cycle_detection() {
        let mut workflow = Workflow::new("cyclic");
        workflow.add_step(
            "a",
            Step {
                step_type: StepType::Pass,
                depends_on: vec![],
                next: vec!["b".to_string()],
                condition: None,
                retry_policy: None,
                timeout: None,
                inputs: HashMap::new(),
                outputs: HashMap::new(),
            },
        );
        workflow.add_step(
            "b",
            Step {
                step_type: StepType::Pass,
                depends_on: vec![],
                next: vec!["a".to_string()],
                condition: None,
                retry_policy: None,
                timeout: None,
                inputs: HashMap::new(),
                outputs: HashMap::new(),
            },
        );

        assert!(matches!(workflow.validate(), Err(WorkflowError::CycleDetected)));
    }

    #[test]
    fn test_execution_order() {
        let mut workflow = Workflow::new("dag");
        workflow.add_step(
            "a",
            Step {
                step_type: StepType::Pass,
                depends_on: vec![],
                next: vec![],
                condition: None,
                retry_policy: None,
                timeout: None,
                inputs: HashMap::new(),
                outputs: HashMap::new(),
            },
        );
        workflow.add_step(
            "b",
            Step {
                step_type: StepType::Pass,
                depends_on: vec!["a".to_string()],
                next: vec![],
                condition: None,
                retry_policy: None,
                timeout: None,
                inputs: HashMap::new(),
                outputs: HashMap::new(),
            },
        );
        workflow.add_step(
            "c",
            Step {
                step_type: StepType::Pass,
                depends_on: vec!["a".to_string()],
                next: vec![],
                condition: None,
                retry_policy: None,
                timeout: None,
                inputs: HashMap::new(),
                outputs: HashMap::new(),
            },
        );
        workflow.add_step(
            "d",
            Step {
                step_type: StepType::Pass,
                depends_on: vec!["b".to_string(), "c".to_string()],
                next: vec![],
                condition: None,
                retry_policy: None,
                timeout: None,
                inputs: HashMap::new(),
                outputs: HashMap::new(),
            },
        );

        let order = workflow.execution_order();
        assert_eq!(order.len(), 3); // 3 levels: [a], [b,c], [d]
        assert!(order[0].contains(&"a".to_string()));
        assert!(order[2].contains(&"d".to_string()));
    }

    #[test]
    fn test_condition_evaluation() {
        let mut context = ExecutionContext::new(WorkflowId::generate());
        context.set_variable("count", serde_json::json!(10));
        context.set_variable("name", serde_json::json!("test"));

        let cond = Condition::GreaterThan { variable: "count".to_string(), value: 5.0 };
        assert!(cond.evaluate(&context));

        let cond =
            Condition::Contains { variable: "name".to_string(), substring: "es".to_string() };
        assert!(cond.evaluate(&context));

        let cond = Condition::And(vec![
            Condition::GreaterThan { variable: "count".to_string(), value: 5.0 },
            Condition::LessThan { variable: "count".to_string(), value: 20.0 },
        ]);
        assert!(cond.evaluate(&context));
    }

    #[test]
    fn test_executor() {
        let mut executor = WorkflowExecutor::new();

        let workflow = WorkflowBuilder::new("test")
            .step(
                "main",
                Step {
                    step_type: StepType::Pass,
                    depends_on: vec![],
                    next: vec![],
                    condition: None,
                    retry_policy: None,
                    timeout: None,
                    inputs: HashMap::new(),
                    outputs: HashMap::new(),
                },
            )
            .build()
            .unwrap();

        let wf_id = workflow.id.clone();
        executor.register(workflow).unwrap();

        let run_id = executor.start(&wf_id, HashMap::new()).unwrap();
        let execution = executor.get_execution(&run_id).unwrap();

        assert_eq!(execution.status, ExecutionStatus::Completed);
    }

    #[test]
    fn test_parallel_step() {
        let step = Step::parallel(vec!["branch1".to_string(), "branch2".to_string()]);

        let StepType::Parallel { branches } = &step.step_type else {
            unreachable!("Expected parallel step");
        };
        assert_eq!(branches.len(), 2);
    }

    #[test]
    fn test_choice_step() {
        let step = Step::choice(vec![
            Choice {
                condition: Condition::GreaterThan { variable: "value".to_string(), value: 10.0 },
                next: "high".to_string(),
            },
            Choice { condition: Condition::Always, next: "default".to_string() },
        ]);

        let StepType::Choice { choices } = &step.step_type else {
            unreachable!("Expected choice step");
        };
        assert_eq!(choices.len(), 2);
    }

    #[test]
    fn test_cancel_execution() {
        let mut executor = WorkflowExecutor::new();

        let workflow = WorkflowBuilder::new("test")
            .step(
                "main",
                Step {
                    step_type: StepType::Pass,
                    depends_on: vec![],
                    next: vec![],
                    condition: None,
                    retry_policy: None,
                    timeout: None,
                    inputs: HashMap::new(),
                    outputs: HashMap::new(),
                },
            )
            .build()
            .unwrap();

        let wf_id = workflow.id.clone();
        executor.register(workflow).unwrap();

        let run_id = executor.start(&wf_id, HashMap::new()).unwrap();
        executor.cancel(&run_id).unwrap();

        let execution = executor.get_execution(&run_id).unwrap();
        assert_eq!(execution.status, ExecutionStatus::Cancelled);
    }
}
