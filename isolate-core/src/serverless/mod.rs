//! Serverless Framework Integration.
//!
//! Adapters for deploying Isolate sandboxes as serverless functions
//! on popular platforms like OpenFaaS, Knative, Fission, and AWS SAM.
//!
//! Provides:
//! - Framework-agnostic function definition
//! - Runtime configuration generators for each platform
//! - Request/response mapping between HTTP and sandbox I/O
//! - Deployment manifest generation


#![allow(missing_docs)]
pub mod adapter;
pub mod function;
pub mod runtime;
pub mod triggers;

pub use adapter::{
    AwsSamAdapter, DeploymentManifest, FissionAdapter, Framework, FrameworkAdapter, IssueSeverity,
    KnativeAdapter, ManifestFile, ManifestFormat, OpenFaaSAdapter, ValidationIssue,
};
pub use function::{
    FunctionBuilder, HandlerConfig, HttpMethod, InvocationRequest, InvocationResponse,
    ModuleSource, RuntimeConfig, ScalingConfig, ServerlessFunction, Trigger,
};
pub use runtime::{FunctionContext, RuntimeHandler, RuntimeMetrics};
pub use triggers::{
    CronConfig, DeadLetterEntry, EventOutcome, MessageQueueConfig, QueueProvider, RetryPolicy,
    TriggerDefinition, TriggerEvent, TriggerId, TriggerManager, TriggerSource,
    TriggerStatistics, WebhookConfig,
};
