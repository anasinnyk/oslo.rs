use crate::{telemetry, Error, Metrics, Result, DurationShorthand};
use chrono::{DateTime, Utc};
use futures::StreamExt;
use kube::{
    api::{Api, ListParams, Patch, PatchParams, ResourceExt},
    client::Client,
    runtime::{
        controller::{Action, Controller},
        events::{Event, EventType, Recorder, Reporter},
        finalizer::{finalizer, Event as Finalizer},
    },
    CustomResource, Resource,
};
use k8s_openapi::NamespaceResourceScope;
use async_trait::async_trait;
use validator::Validate;
use std::{fmt::{Display,Debug}, any::type_name, hash::Hash, collections::HashMap};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::json;
use std::sync::Arc;
use tokio::{sync::RwLock, time::Duration};
use reqwest::Client as HTTPClient;
use tracing::*;

pub static OSLO_GROUP: &str = "oslo.sre.macpaw.dev";
pub static OSLO_VERSION: &str = "v1alpha1";

/// Service
/// A Service is a high-level grouping of SLO.
#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, Validate, JsonSchema)]
#[kube(kind = "Service", group = "oslo.rs", version = "v1alpha1", namespaced, status = "ServiceStatus", shortname = "svc")]
#[serde(rename_all = "camelCase")]
pub struct ServiceSpec {
    #[validate(length(min = 1))]
    display_name: Option<String>,

    #[validate(length(max = 1050))]
    description: Option<String>,
}

/// The status object of `Service`
#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct ServiceStatus {
   conditions: Vec<StatusCondition>,
}

/// SLO
/// A service level objective (SLO) is a target value or a range of values for a service level that is described by a service level indicator (SLI).
#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, Validate, JsonSchema)]
#[kube(kind = "SLO", group = "oslo.sre.macpaw.dev", version = "v1alpha1", namespaced, status = "SLOStatus")]
#[serde(rename_all = "camelCase")]
pub struct SLOSpec {
    #[validate(length(min = 1))]
    display_name: Option<String>,
    #[validate(length(max = 1050))]
    description: Option<String>,

    service: String,

    indicator_ref: String,

    indicator: SLI,

    budgeting_method: BudgetingMethod,

    objectives: Vec<Objective>,

    time_window: Vec<String>,

    alert_policies: Vec<String>,
}

#[derive(Debug, Clone, JsonSchema, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BudgetingMethod {
    Occurrences,
    Timeslices
}

#[derive(Clone, Debug, JsonSchema, Deserialize, Validate, Serialize)]
pub struct Objective {
    #[validate(length(min = 1))]
    display_name: Option<String>,

    op: Option<Op>,

    value: Option<i32>,

    #[validate(range(min = 0.0, max = 1.0))]
    target: Option<f32>,

    #[validate(range(min = 0.0, max = 100.0))]
    target_percent: Option<f32>,

    #[validate(range(min = 0.0, max = 1.0))]
    time_slice_target: Option<f32>,

    time_slice_window: DurationShorthand,
}

/// The status object of `SLO`
#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct SLOStatus {
   conditions: Vec<StatusCondition>,
}

/// SLI
/// A service level indicator (SLI) represents how to read metrics from data sources.
#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, Validate, JsonSchema)]
#[kube(kind = "SLI", group = "oslo.sre.macpaw.dev", version = "v1alpha1", namespaced, status = "SLIStatus")]
#[serde(rename_all = "camelCase")]
pub struct SLISpec {
    #[validate(length(min = 1))]
    display_name: Option<String>,
    threshold_metric: ThresholdMetric,
    ratio_metric: RatioMetric
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MetricSource {
    metric_source_ref: Option<String>,
    spec: MetricSourceType,
    r#type: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[serde(untagged)]
pub enum MetricSourceType {
    PromMetricSource(PromMetricSource)
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct PromMetricSource {}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ThresholdMetric {
    metric_source: MetricSource
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RatioMetric {
    counter: Option<bool>,
    total: MetricSource,
    good: Option<MetricSource>,
    bad: Option<MetricSource>,
    raw_type: Option<RawType>,
    raw: Option<MetricSource>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum RawType {
    Success,
    Failure,
}

/// The status object of `SLI`
#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct SLIStatus {
   conditions: Vec<StatusCondition>,
}

/// DataSource
/// A DataSource represents connection details with a particular metric source.
#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, Validate, JsonSchema)]
#[kube(kind = "DataSource", group = "oslo.sre.macpaw.dev", version = "v1alpha1", namespaced, status = "DataSourceStatus", shortname = "ds")]
#[serde(rename_all = "camelCase")]
pub struct DataSourceSpec {
    #[validate(length(min = 1))]
    display_name: Option<String>,
    #[validate(length(max = 1050))]
    description: Option<String>,
    #[serde(rename = "type")]
    connection_type: DataSourceConnectionType,
    connection_details: DataSourceConnectionDetails,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub enum DataSourceConnectionType {
    Prometheus,
    Datadog,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[serde(untagged)]
pub enum DataSourceConnectionDetails {
    PrometheusConnectionDetails(PrometheusConnectionDetails),
    DatadogConnectionDetails(DatadogConnectionDetails),
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct DatadogConnectionDetails {
    url: String,
    paths: PrometheusPaths,
}
#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct PrometheusConnectionDetails {
    url: String,
    paths: PrometheusPaths,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct PrometheusPaths {
    #[serde(default = "prometheus_default_path_status_config")]
    status_config: String,
}

fn prometheus_default_path_status_config() -> String { "/prometheus/api/v1/status/config".to_string() }

/// The status object of `DataSource`
#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct DataSourceStatus {
    conditions: Vec<StatusCondition>,
    connected: bool,
}

/// AlertPolicy
/// An Alert Policy allows you to define the alert conditions for an SLO.
#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, Validate, JsonSchema)]
#[kube(kind = "AlertPolicy", group = "oslo.sre.macpaw.dev", version = "v1alpha1", namespaced, status = "AlertPolicyStatus", shortname = "ap")]
#[serde(rename_all = "camelCase")]
pub struct AlertPolicySpec {
    #[validate(length(min = 1))]
    display_name: Option<String>,
    #[validate(length(max = 1050))]
    description: Option<String>,
    alert_when_no_data: bool,
    alert_when_resolved: bool,
    alert_when_breaching: bool,
    #[validate(length(min = 1, max = 1))]
    conditions: Vec<AlertConditionRef>,
    #[validate(length(min = 1))]
    notification_targets: Vec<AlertNotificationTargetRef>
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AlertConditionRef {
    condition_ref: Option<String>,
    kind: AlertConditionRefKind,
    metadata: Metadata,
    spec: AlertConditionSpec,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AlertNotificationTargetRef {
    target_ref: Option<String>,
    kind: AlertNotificationTargetRefKind,
    metadata: Metadata,
    spec: AlertNotificationTargetSpec,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
enum AlertNotificationTargetRefKind {
    AlertNotificationTarget
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
enum AlertConditionRefKind {
    AlertCondition
}

/// The status object of `AlertPolicy`
#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct AlertPolicyStatus {
   conditions: Vec<StatusCondition>,
}

/// AlertCondition
/// An Alert Condition allows you to define under which conditions an alert for an SLO needs to be triggered.
#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, Validate, JsonSchema)]
#[kube(kind = "AlertCondition", group = "oslo.sre.macpaw.dev", version = "v1alpha1", namespaced, status = "AlertConditionStatus", shortname = "ac")]
#[serde(rename_all = "camelCase")]
pub struct AlertConditionSpec {
    #[validate(length(min = 1))]
    display_name: Option<String>,
    #[validate(length(max = 1050))]
    description: Option<String>,

    severity: String,

    condition: Condition,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Condition {
    kind: ConditionKind,
    op: Op,
    threshold: i32,
    #[serde()]
    lookback_window: DurationShorthand,
    alert_after: DurationShorthand,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum ConditionKind {
    Burnrate,
}

/// The status object of `AlertCondition`
#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct AlertConditionStatus {
   conditions: Vec<StatusCondition>,
}

/// AlertNotificationTarget
/// An Alert Notification Target defines the possible targets where alert notifications should be delivered to. For example, this can be a web-hook, Slack or any other custom target.
#[derive(CustomResource, Deserialize, Serialize, Clone, Debug, Validate, JsonSchema)]
#[kube(kind = "AlertNotificationTarget", group = "oslo.sre.macpaw.dev", version = "v1alpha1", namespaced, status = "AlertNotificationTargetStatus", shortname = "ant")]
#[serde(rename_all = "camelCase")]
pub struct AlertNotificationTargetSpec {
    #[validate(length(min = 1))]
    display_name: Option<String>,
    #[validate(length(max = 1050))]
    description: Option<String>,

    target: String,
}

/// The status object of `AlertNotificationTarget`
#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct AlertNotificationTargetStatus {
   conditions: Vec<StatusCondition>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum Op {
    Lte,
    Gte,
    Lt,
    Gt,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
pub struct Metadata {
    name: String,
    labels: Option<HashMap<String, String>>,
    annotations: Option<HashMap<String, String>>,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatusCondition {
    #[serde(rename = "type")]
    condition_type: StatusType,
    status: Status,
    observd_generation: Option<i64>,
    last_transition_time: DateTime<Utc>,
    reason: StatusReason,
    message: String
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, Display, PartialEq)]
pub enum StatusType {
    Ready,
    NotReady,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, Display)]
pub enum Status {
    True,
    False,
    Unknown,
}

#[derive(Deserialize, Serialize, Clone, Debug, JsonSchema, Display)]
pub enum StatusReason {
    Connected,
    CanNotConnect,
    DependencyNotReady,
}


// Context for our reconciler
#[derive(Clone)]
pub struct Context {
    /// Kubernetes client
    pub client: Client,
    /// Diagnostics read by the web server
    pub diagnostics: Arc<RwLock<Diagnostics>>,
    /// Prometheus metrics
    pub metrics: Metrics,
}

impl StatusCondition {
    pub fn not_connected(message: &OSLOError, observd_generation: Option<i64>) -> Self {
        Self {
            condition_type: StatusType::NotReady,
            status: Status::True,
            last_transition_time: Utc::now(),
            reason: StatusReason::CanNotConnect,
            message: message.to_string(),
            observd_generation,
        }
    }

    pub fn connected(observd_generation: Option<i64>) -> Self {
         Self {
            condition_type: StatusType::Ready,
            status: Status::True,
            last_transition_time: Utc::now(),
            reason: StatusReason::Connected,
            message: "Connected successful".into(),
            observd_generation,
        }
    }
}

#[async_trait]
impl Reconciler for Service {
    // Reconcile (for non-finalizer related changes)
    async fn reconcile(&self, _ctx: Arc<Context>) -> Result<Action> { todo!() }

    async fn cleanup(&self, _ctx: Arc<Context>) -> Result<Action> { todo!() }
}

#[async_trait]
impl Reconciler for SLO {
    // Reconcile (for non-finalizer related changes)
    async fn reconcile(&self, _ctx: Arc<Context>) -> Result<Action> { todo!() }

    async fn cleanup(&self, _ctx: Arc<Context>) -> Result<Action> { todo!() }
}

#[async_trait]
impl Reconciler for SLI {
    // Reconcile (for non-finalizer related changes)
    async fn reconcile(&self, _ctx: Arc<Context>) -> Result<Action> { todo!() }

    async fn cleanup(&self, _ctx: Arc<Context>) -> Result<Action> { todo!() }
}

#[async_trait]
impl Reconciler for AlertPolicy {
    // Reconcile (for non-finalizer related changes)
    async fn reconcile(&self, _ctx: Arc<Context>) -> Result<Action> { todo!() }

    async fn cleanup(&self, _ctx: Arc<Context>) -> Result<Action> { todo!() }
}

#[async_trait]
impl Reconciler for AlertCondition {
    // Reconcile (for non-finalizer related changes)
    async fn reconcile(&self, _ctx: Arc<Context>) -> Result<Action> { todo!() }

    async fn cleanup(&self, _ctx: Arc<Context>) -> Result<Action> { todo!() }
}

#[derive(Error, Debug)]
pub enum OSLOError {
    #[error("Prometheus: {0}")]
    PrometheusError(String),

    #[error("Provider: {0}")]
    ProviderError(String),
}

#[async_trait]
impl Reconciler for AlertNotificationTarget {
    // Reconcile (for non-finalizer related changes)
    async fn reconcile(&self, _ctx: Arc<Context>) -> Result<Action> { todo!() }

    async fn cleanup(&self, _ctx: Arc<Context>) -> Result<Action> { todo!() }
}

#[async_trait]
trait DataSourceProvider: Sized {
    fn new(details: DataSourceConnectionDetails) -> Result<Self, OSLOError>;

    async fn check_connection(&mut self) -> Option<OSLOError>;
}
struct PrometheusProvider {
    client: HTTPClient,
    config: PrometheusConnectionDetails,
}

#[async_trait]
impl DataSourceProvider for PrometheusProvider {
    fn new(details: DataSourceConnectionDetails) -> Result<Self, OSLOError> {
        match details {
            DataSourceConnectionDetails::PrometheusConnectionDetails(config) => Ok(Self {
                client: HTTPClient::new(),
                config,
            }),
            _ => Err(OSLOError::ProviderError(format!("parse incorrect, {:?}", details))),
        }
    }

    async fn check_connection(&mut self) -> Option<OSLOError> {
        #[derive(Debug, Deserialize)]
        struct PrometheusResponse {
            status: String,
        }

        match self.client.post(format!("{}{}", self.config.url, self.config.paths.status_config)).send().await {
            Ok(response) => match response.status() {
                reqwest::StatusCode::OK => match response.json::<PrometheusResponse>().await {
                    Ok(result) => if result.status == "success" {
                        None
                    } else {
                        Some(OSLOError::PrometheusError(format!("Prometheus configuration is unsucceed. Status {}", result.status)))
                    },
                    Err(err) => Some(OSLOError::PrometheusError(format!("Parse error: {}", err)))
                }
                _ => Some(OSLOError::PrometheusError(format!("Response code is incorrect {}.", response.status())))
            }
            Err(err) => Some(OSLOError::PrometheusError(format!("Connection error: {}", err)))
        }
    }
}

#[async_trait]
impl Reconciler for DataSource {
    // Reconcile (for non-finalizer related changes)
    async fn reconcile(&self, ctx: Arc<Context>) -> Result<Action> {
        let client = ctx.client.clone();
        //let recorder = ctx.diagnostics.read().await.recorder(client.clone(), self);
        let ns = self.namespace().unwrap();
        let name = self.name_any();
        let dss: Api<DataSource> = Api::namespaced(client, &ns);

        let connection_details = self.spec.connection_details.clone();
        let condition = match PrometheusProvider::new(connection_details) {
            Err(err) => StatusCondition::not_connected(&err, self.metadata.generation),
            Ok(mut p) => match p.check_connection().await {
                Some(err) => StatusCondition::not_connected(&err, self.metadata.generation),
                None => StatusCondition::connected(self.metadata.generation)
            }
        };
        let connected = condition.condition_type == StatusType::Ready;
        let is_patch_needed;

        let conditions = match &self.status {
            Some(s) => if s.connected != connected {
                    is_patch_needed = true;
                    vec![s.conditions[s.conditions.len() - 1].clone(), condition]
                } else {
                    is_patch_needed = false;
                    vec![]
                }
            None => {
              is_patch_needed = true;
              vec![condition]
            }
        };

        if is_patch_needed {
            dss.patch_status(&name, &PatchParams::apply("oslo").force(), &Patch::Apply(json!({
                "apiVersion": format!("{}/{}", OSLO_GROUP, OSLO_VERSION),
                "kind": "DataSource",
                "status": DataSourceStatus {
                    connected,
                    conditions,
                }
            })))
            .await
            .map_err(Error::KubeError)?;
        }

        // If no events were received, check back every 5 minutes
        Ok(Action::requeue(Duration::from_secs(5 * 60)))
    }

    // Finalizer cleanup (the object was deleted, ensure nothing is orphaned)
    async fn cleanup(&self, ctx: Arc<Context>) -> Result<Action> {
        let recorder = ctx.diagnostics.read().await.recorder(ctx.client.clone(), self);
        // Document doesn't have any real cleanup, so we just publish an event
        recorder
            .publish(Event {
                type_: EventType::Normal,
                reason: "DeleteRequested".into(),
                note: Some(format!("Delete `{}`", self.name_any())),
                action: "Deleting".into(),
                secondary: None,
            })
            .await
            .map_err(Error::KubeError)?;
        Ok(Action::await_change())
    }
}

/// Diagnostics to be exposed by the web server
#[derive(Clone, Serialize)]
pub struct Diagnostics {
    #[serde(deserialize_with = "from_ts")]
    pub last_event: DateTime<Utc>,
    #[serde(skip)]
    pub reporter: Reporter,
}
impl Default for Diagnostics {
    fn default() -> Self {
        Self {
            last_event: Utc::now(),
            reporter: "ds-controller".into(),
        }
    }
}
impl Diagnostics {
    fn recorder(&self, client: Client, doc: &DataSource) -> Recorder {
        Recorder::new(client, self.reporter.clone(), doc.object_ref(&()))
    }
}

/// State shared between the controller and the web server
#[derive(Clone, Default)]
pub struct State {
    /// Diagnostics populated by the reconciler
    diagnostics: Arc<RwLock<Diagnostics>>,
    /// Metrics registry
    registry: prometheus::Registry,
}

/// State wrapper around the controller outputs for the web server
impl State {
    /// Metrics getter
    pub fn metrics(&self) -> Vec<prometheus::proto::MetricFamily> {
        self.registry.gather()
    }

    /// State getter
    pub async fn diagnostics(&self) -> Diagnostics {
        self.diagnostics.read().await.clone()
    }

    // Create a Controller Context that can update State
    pub fn to_context(&self, client: Client) -> Arc<Context> {
        Arc::new(Context {
            client,
            metrics: Metrics::default().register(&self.registry).unwrap(),
            diagnostics: self.diagnostics.clone(),
        })
    }
}

#[async_trait]
pub trait Reconciler {
    async fn reconcile(&self, ctx: Arc<Context>) -> Result<Action>;
    async fn cleanup(&self, ctx: Arc<Context>) -> Result<Action>;
}

#[async_trait]
pub trait ManagerTrait<K>
where
    K: Clone + Resource<Scope = NamespaceResourceScope> + Reconciler + Serialize + DeserializeOwned + Debug + Send + Sync + 'static,
    K::DynamicType: Eq + Hash + Clone + Default + Debug + Unpin
{
    async fn run(state: State) {
        let client = Client::try_default().await.expect("failed to create kube Client");
        let rs = Api::<K>::all(client.clone());
        if let Err(e) = rs.list(&ListParams::default().limit(1)).await {
            error!("CRD is not queryable; {e:?}. Is the CRD installed?");
            info!("Installation: cargo run --bin crdgen | kubectl apply -f -");
            std::process::exit(1);
        }

        Controller::new(Api::<K>::all(client.clone()), ListParams::default())
            .shutdown_on_signal()
            .run(
                &Self::reconcile,
                &Self::error_policy,
                state.to_context(client))
            .filter_map(|x| async move { std::result::Result::ok(x) })
            .for_each(|_| futures::future::ready(()))
            .await;
    }

    #[instrument(skip(ctx, r), fields(trace_id))]
    async fn reconcile(r: Arc<K>, ctx: Arc<Context>) -> Result<Action> {
        let trace_id = telemetry::get_trace_id();
        Span::current().record("trace_id", &field::display(&trace_id));
        let _timer = ctx.metrics.count_and_measure();
        ctx.diagnostics.write().await.last_event = Utc::now();
        let ns = r.namespace().unwrap(); // ds is namespace scoped
        let rs: Api<K> = Api::namespaced(ctx.client.clone(), &ns);
        let kind = type_name::<K>();
        info!("Reconciling {} \"{}\" in {}", kind, r.name_any(), ns);
        finalizer(&rs, &format!("{}.{}", kind.to_lowercase(), OSLO_GROUP), r, |event| async {
            match event {
                Finalizer::Apply(r) => r.reconcile(ctx.clone()).await,
                Finalizer::Cleanup(r) => r.cleanup(ctx.clone()).await,
            }
        })
        .await
        .map_err(|e| Error::FinalizerError(Box::new(e)))
    }

    fn error_policy(r: Arc<K>, error: &Error, ctx: Arc<Context>) -> Action {
        warn!("reconcile failed: {:?}", error);
        ctx.metrics.reconcile_failure(&r.name_any(), error);
        Action::requeue(Duration::from_secs(5 * 60))
    }
}

pub struct Manager<K> {
    _data: K
}

#[async_trait]
impl<K> ManagerTrait<K> for Manager<K>
where
    K: Clone + Resource<Scope = NamespaceResourceScope> + Reconciler + Serialize + DeserializeOwned + Debug + Send + Sync + 'static,
    K::DynamicType: Eq + Hash + Clone + Default + Debug + Unpin
{
}

// Mock tests relying on fixtures.rs and its primitive apiserver mocks
#[cfg(test)]
mod test {
    use super::{error_policy, reconcile, Context, Document};
    use crate::fixtures::{timeout_after_1s, Scenario};
    use std::sync::Arc;

    #[tokio::test]
    async fn documents_without_finalizer_gets_a_finalizer() {
        let (testctx, fakeserver, _) = Context::test();
        let doc = Document::test();
        let mocksrv = fakeserver.run(Scenario::FinalizerCreation(doc.clone()));
        reconcile(Arc::new(doc), testctx).await.expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }

    #[tokio::test]
    async fn finalized_doc_causes_status_patch() {
        let (testctx, fakeserver, _) = Context::test();
        let doc = Document::test().finalized();
        let mocksrv = fakeserver.run(Scenario::StatusPatch(doc.clone()));
        reconcile(Arc::new(doc), testctx).await.expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }

    #[tokio::test]
    async fn finalized_doc_with_hide_causes_event_and_hide_patch() {
        let (testctx, fakeserver, _) = Context::test();
        let doc = Document::test().finalized().needs_hide();
        let scenario = Scenario::EventPublishThenStatusPatch("HideRequested".into(), doc.clone());
        let mocksrv = fakeserver.run(scenario);
        reconcile(Arc::new(doc), testctx).await.expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }

    #[tokio::test]
    async fn finalized_doc_with_delete_timestamp_causes_delete() {
        let (testctx, fakeserver, _) = Context::test();
        let doc = Document::test().finalized().needs_delete();
        let mocksrv = fakeserver.run(Scenario::Cleanup("DeleteRequested".into(), doc.clone()));
        reconcile(Arc::new(doc), testctx).await.expect("reconciler");
        timeout_after_1s(mocksrv).await;
    }

    #[tokio::test]
    async fn illegal_doc_reconcile_errors_which_bumps_failure_metric() {
        let (testctx, fakeserver, _registry) = Context::test();
        let doc = Arc::new(Document::illegal().finalized());
        let mocksrv = fakeserver.run(Scenario::RadioSilence);
        let res = reconcile(doc.clone(), testctx.clone()).await;
        timeout_after_1s(mocksrv).await;
        assert!(res.is_err(), "apply reconciler fails on illegal doc");
        let err = res.unwrap_err();
        assert!(err.to_string().contains("IllegalDocument"));
        // calling error policy with the reconciler error should cause the correct metric to be set
        error_policy(doc.clone(), &err, testctx.clone());
        //dbg!("actual metrics: {}", registry.gather());
        let failures = testctx
            .metrics
            .failures
            .with_label_values(&["illegal", "finalizererror(applyfailed(illegaldocument))"])
            .get();
        assert_eq!(failures, 1);
    }

    // Integration test without mocks
    use kube::api::{Api, ListParams, Patch, PatchParams};
    #[tokio::test]
    #[ignore = "uses k8s current-context"]
    async fn integration_reconcile_should_set_status_and_send_event() {
        let client = kube::Client::try_default().await.unwrap();
        let ctx = super::State::default().to_context(client.clone());

        // create a test doc
        let doc = Document::test().finalized().needs_hide();
        let docs: Api<Document> = Api::namespaced(client.clone(), "default");
        let ssapply = PatchParams::apply("ctrltest");
        let patch = Patch::Apply(doc.clone());
        docs.patch("test", &ssapply, &patch).await.unwrap();

        // reconcile it (as if it was just applied to the cluster like this)
        reconcile(Arc::new(doc), ctx).await.unwrap();

        // verify side-effects happened
        let output = docs.get_status("test").await.unwrap();
        assert!(output.status.is_some());
        // verify hide event was found
        let events: Api<k8s_openapi::api::core::v1::Event> = Api::all(client.clone());
        let opts = ListParams::default().fields("involvedObject.kind=Document,involvedObject.name=test");
        let event = events
            .list(&opts)
            .await
            .unwrap()
            .into_iter()
            .filter(|e| e.reason.as_deref() == Some("HideRequested"))
            .last()
            .unwrap();
        dbg!("got ev: {:?}", &event);
        assert_eq!(event.action.as_deref(), Some("Hiding"));
    }
}
