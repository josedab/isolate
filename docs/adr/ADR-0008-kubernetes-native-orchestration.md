# ADR-0008: Kubernetes-Native Orchestration via CRDs

## Status

Accepted

## Context

Running sandboxes at scale requires orchestration: scheduling, scaling, health checking, and lifecycle management. While Isolate could implement its own orchestration layer, Kubernetes is the de facto standard for container orchestration and provides:

- Declarative resource management
- Built-in scaling (HPA, VPA)
- Service discovery and load balancing
- Health checking and self-healing
- RBAC and network policies

We needed Kubernetes integration that:

- Feels native to Kubernetes users
- Leverages existing K8s infrastructure
- Supports sandbox-specific concepts (capabilities, fuel limits)
- Enables GitOps workflows

## Decision

We implemented **Kubernetes-native orchestration** via Custom Resource Definitions (CRDs) with an operator pattern.

### Custom Resource Definitions

Two primary CRDs:

```yaml
# Sandbox CRD - single sandbox instance
apiVersion: isolate.io/v1alpha1
kind: Sandbox
metadata:
  name: my-sandbox
spec:
  module:
    image: ghcr.io/example/wasm-module:v1
  resources:
    limits:
      memory: "128Mi"
      fuel: 1000000
  capabilities:
    - type: network
      allow: ["api.example.com"]
  replicas: 3

---
# SandboxPool CRD - warm pool of sandboxes
apiVersion: isolate.io/v1alpha1
kind: SandboxPool
metadata:
  name: my-pool
spec:
  template:
    module:
      image: ghcr.io/example/wasm-module:v1
  size: 10
  warmPoolSize: 5
  maxIdleSeconds: 300
```

### Operator Pattern

```rust
pub struct IsolateOperator {
    state: OperatorState,
    config: OperatorConfig,
    metrics: OperatorMetrics,
    event_queue: VecDeque<OperatorEvent>,
}

impl IsolateOperator {
    pub fn reconcile(&mut self, sandbox: &mut SandboxCrd) -> ReconcileAction {
        match sandbox.status.phase {
            SandboxPhase::Pending => self.handle_pending(sandbox),
            SandboxPhase::Initializing => self.handle_initializing(sandbox),
            SandboxPhase::Running => self.handle_running(sandbox),
            SandboxPhase::Failed => self.handle_failed(sandbox),
            // ...
        }
    }
}
```

### Resource-Aware Scheduling

```rust
pub struct SandboxScheduler {
    nodes: Vec<NodeResources>,
    strategy: SchedulingStrategy,
}

pub enum SchedulingStrategy {
    BinPacking,     // Maximize node utilization
    Spreading,      // Spread across nodes
    ResourceAware,  // Consider fuel/memory constraints
}

// Supports K8s native constructs
pub struct NodeAffinity {
    required: Vec<LabelExpression>,
    preferred: Vec<PreferredExpression>,
}

pub struct Toleration {
    key: String,
    operator: TolerationOperator,
    effect: TaintEffect,
}
```

### Helm Chart Generation

```rust
pub struct HelmChartGenerator {
    output_dir: PathBuf,
}

impl HelmChartGenerator {
    pub fn generate(&self, values: &HelmValues) -> Result<()> {
        self.write_chart_yaml(&values.metadata)?;
        self.write_values_yaml(values)?;
        self.write_templates(values)?;
        Ok(())
    }
}
```

Generates:
- `Chart.yaml` - metadata
- `values.yaml` - configurable values
- `templates/deployment.yaml`
- `templates/service.yaml`
- `templates/rbac.yaml`
- `templates/crds.yaml`

## Consequences

### Positive

- **Native feel**: K8s users use familiar patterns (`kubectl apply`, Helm, ArgoCD)
- **Ecosystem integration**: Works with Prometheus, Grafana, Istio, etc.
- **Declarative**: GitOps-friendly, auditable configuration
- **Scalable**: Leverages K8s scheduling and scaling
- **Observable**: Status subresource, conditions, events

### Negative

- **K8s dependency**: Requires Kubernetes cluster
- **CRD complexity**: More code to maintain than simple API
- **Operator overhead**: Reconciliation loops add complexity
- **Version coupling**: CRD schema changes require migration

### Implications

- Operator must be deployed as a controller in the cluster
- CRD versioning (v1alpha1) signals API instability
- Status updates require proper optimistic concurrency
- Helm values should cover common customizations
- RBAC rules must grant access to CRDs
