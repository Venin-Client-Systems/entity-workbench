use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Person,
    Organisation,
    Group,
    Account,
    Place,
    DigitalIdentifier,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Identifier {
    pub namespace: String,
    pub value: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Entity {
    pub id: String,
    pub name: String,
    pub kind: EntityKind,
    pub identifiers: Vec<Identifier>,
    pub merged_into: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceAnchor {
    Text {
        evidence_id: String,
        line_start: u32,
        line_end: u32,
    },
    Page {
        evidence_id: String,
        page: u32,
        region: Option<[f64; 4]>,
    },
    Cell {
        evidence_id: String,
        sheet: String,
        row: u32,
        column: String,
    },
    Message {
        evidence_id: String,
        message_id: String,
    },
    Capture {
        evidence_id: String,
        selector: String,
    },
}
impl SourceAnchor {
    pub fn evidence_id(&self) -> &str {
        match self {
            Self::Text { evidence_id, .. }
            | Self::Page { evidence_id, .. }
            | Self::Cell { evidence_id, .. }
            | Self::Message { evidence_id, .. }
            | Self::Capture { evidence_id, .. } => evidence_id,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub id: String,
    pub name: String,
    pub sha256: String,
    pub bytes: u64,
    pub media_type: String,
    pub origin_group: String,
    pub imported_at: String,
    pub extraction_status: String,
    pub text: Option<String>,
    #[serde(default)]
    pub acquisitions: Vec<Acquisition>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Acquisition {
    pub job_id: String,
    pub url: String,
    pub retrieved_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    Pending,
    Accepted,
    Rejected,
    Deferred,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub id: String,
    pub entity_id: String,
    pub field: String,
    pub value: String,
    pub anchor: SourceAnchor,
    pub extraction_quality: Option<f64>,
    pub review: ReviewState,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Assertion {
    pub id: String,
    pub subject_id: String,
    pub predicate: String,
    pub object_id: String,
    pub observation_ids: Vec<String>,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub confidence: String,
    pub review: ReviewState,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewDecision {
    pub id: String,
    pub target_id: String,
    pub state: ReviewState,
    pub reason: String,
    pub at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Transaction {
    pub id: String,
    pub account: String,
    pub date: String,
    pub posting_date: Option<String>,
    pub description: String,
    pub amount: String,
    pub currency: String,
    pub balance: Option<String>,
    pub anchor: SourceAnchor,
    pub review: ReviewState,
    pub duplicate_candidates: Vec<String>,
    pub transfer_peer: Option<String>,
    pub merchant: Option<String>,
    pub version: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddressAssociation {
    pub id: String,
    pub entity_id: String,
    pub label: String,
    pub latitude: f64,
    pub longitude: f64,
    pub valid_from: String,
    pub valid_to: Option<String>,
    pub uncertainty_m: f64,
    pub anchor: SourceAnchor,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    InPerson,
    Online,
    Unknown,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MerchantLocation {
    pub id: String,
    pub transaction_id: String,
    pub merchant: String,
    pub branch: Option<String>,
    pub channel: Channel,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub uncertainty_m: f64,
    pub retrieved_at: String,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub anchor: SourceAnchor,
    pub review: ReviewState,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Lead {
    pub id: String,
    pub label: String,
    pub identifier: Identifier,
    pub source_id: Option<String>,
    pub state: ReviewState,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Hypothesis {
    pub id: String,
    pub question: String,
    pub proposition: String,
    pub alternatives: Vec<String>,
    pub gaps: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub id: String,
    #[serde(default)]
    pub hypothesis_ids: Vec<String>,
    pub title: String,
    pub assessment: String,
    pub supporting_ids: Vec<String>,
    pub contradicting_ids: Vec<String>,
    pub limitations: String,
    pub needs_review: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HypothesisInput {
    pub question: String,
    pub proposition: String,
    pub alternatives: Vec<String>,
    pub gaps: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FindingInput {
    pub title: String,
    pub assessment: String,
    pub supporting_ids: Vec<String>,
    pub contradicting_ids: Vec<String>,
    pub limitations: String,
    pub hypothesis_ids: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Running,
    Blocked,
    QuotaExhausted,
    Failed,
    SuccessfulNoResults,
    Successful,
    Cancelled,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CollectionJob {
    pub id: String,
    pub queries: Vec<String>,
    pub adapters: Vec<String>,
    pub max_hops: u32,
    pub max_requests: u32,
    pub max_seconds: u64,
    pub requests_used: u32,
    pub state: JobState,
    pub detail: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnalysisManifest {
    pub id: String,
    pub workspace_revision: u64,
    pub engine: String,
    pub engine_version: String,
    pub input_ids: Vec<String>,
    pub outputs: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportSnapshot {
    pub id: String,
    pub workspace_revision: u64,
    pub created_at: String,
    pub sha256: String,
    pub html: String,
}
/// Lightweight catalogue entry. Report bytes are retrieved and verified explicitly.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportMetadata {
    pub id: String,
    pub workspace_revision: u64,
    pub created_at: String,
    pub sha256: String,
    pub html_bytes: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MergeDecision {
    pub id: String,
    pub source: String,
    pub target: String,
    pub reason: String,
    pub reversed: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EntityInput {
    pub name: String,
    pub kind: EntityKind,
    pub identifiers: Vec<Identifier>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObservationInput {
    pub entity_id: String,
    pub field: String,
    pub value: String,
    pub anchor: SourceAnchor,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum IdentityOutcome {
    KeepSeparate,
    Defer,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IdentityDecision {
    pub id: String,
    pub left_id: String,
    pub right_id: String,
    pub outcome: IdentityOutcome,
    pub reason: String,
    pub at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonSignal {
    InsufficientReviewedEvidence,
    SharedReviewedValues,
    DifferentReviewedValues,
    MixedReviewedValues,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComparisonField {
    pub field: String,
    pub left: Vec<Observation>,
    pub right: Vec<Observation>,
    pub signal: ComparisonSignal,
    pub source_groups: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IdentityComparison {
    pub workspace_revision: u64,
    pub left: Entity,
    pub right: Entity,
    pub fields: Vec<ComparisonField>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceExcerpt {
    pub evidence_id: String,
    pub workspace_revision: u64,
    pub location: String,
    pub quote: String,
    pub truncated: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "WorkspaceView")]
pub struct WorkspaceView<R = ReportSnapshot> {
    pub schema_version: u32,
    pub revision: u64,
    pub entities: Vec<Entity>,
    pub evidence: Vec<Evidence>,
    pub observations: Vec<Observation>,
    pub assertions: Vec<Assertion>,
    pub transactions: Vec<Transaction>,
    pub addresses: Vec<AddressAssociation>,
    pub locations: Vec<MerchantLocation>,
    pub leads: Vec<Lead>,
    pub jobs: Vec<CollectionJob>,
    pub findings: Vec<Finding>,
    pub hypotheses: Vec<Hypothesis>,
    pub decisions: Vec<ReviewDecision>,
    pub merges: Vec<MergeDecision>,
    #[serde(default)]
    pub identity_decisions: Vec<IdentityDecision>,
    pub reports: Vec<R>,
    #[serde(default)]
    pub statement_profiles: Vec<crate::statements::StatementProfile>,
    #[serde(default)]
    pub statement_imports: Vec<crate::statements::StatementImport>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    PageTransferCandidates {
        request: crate::transfer_candidates::TransferCandidatesRequest,
        expected_revision: u64,
    },
    ReadTransactionBalances {
        request: crate::transaction_balance::TransactionBalancesRequest,
        expected_revision: u64,
    },
    PageCitationCatalogue {
        request: crate::citation_catalogue::CitationCatalogueRequest,
        expected_revision: u64,
    },
    ReadCitationSelections {
        request: crate::citation_catalogue::CitationSelectionsRequest,
        expected_revision: u64,
    },
    SearchTransactions {
        request: crate::transaction_search::TransactionSearchRequest,
        expected_revision: u64,
    },
    PageTransactionFacets {
        request: crate::transaction_facets::TransactionFacetRequest,
        expected_revision: u64,
    },
    PageReviewDecisions {
        request: crate::review_decision_page::ReviewDecisionPageRequest,
        expected_revision: u64,
    },
    ReadTransactionSources {
        request: crate::transaction_sources::TransactionSourcesRequest,
        expected_revision: u64,
    },
    PageTransactions {
        request: crate::transaction_page::TransactionPageRequest,
        expected_revision: u64,
    },
    InspectReportSnapshot {
        report_id: String,
        expected_sha256: String,
    },
    CompareTransactionPeriods {
        request: crate::transaction_comparison::TransactionComparisonRequest,
        expected_revision: u64,
    },
    AnalyzeTransactions {
        request: crate::transaction_analysis::TransactionAnalysisRequest,
        expected_revision: u64,
    },
    QueuePdfPageOcr {
        evidence_id: String,
        request_key: String,
        page_number: u32,
        dpi: u32,
    },
    InspectPdfExtraction {
        extraction_id: String,
    },
    QueueImageOcrRegions {
        evidence_id: String,
        request_key: String,
    },
    InspectImageRegionExtraction {
        extraction_id: String,
    },
    QueueImageOcr {
        evidence_id: String,
        request_key: String,
    },
    InspectImageExtraction {
        extraction_id: String,
    },
    QueueDocumentParse {
        evidence_id: String,
        request_key: String,
    },
    ListProcessingJobs {},
    InspectExtraction {
        extraction_id: String,
    },
    InspectProcessingJob {
        job_id: String,
    },
    CancelProcessingJob {
        job_id: String,
        expected_attempt: u32,
    },
    RetryProcessingJob {
        job_id: String,
        expected_attempt: u32,
        reason: String,
    },
    View {},
    Search {
        query: String,
    },
    CollectWeb {
        urls: Vec<String>,
        max_hops: u32,
        max_requests: u32,
        max_seconds: u64,
    },
    InspectCollection {
        job_id: String,
    },
    ExportCollection {
        job_id: String,
    },
    SeedDemo {},
    Import {
        name: String,
        bytes: Vec<u8>,
    },
    InspectStatement {
        bytes: Vec<u8>,
        delimiter: crate::statements::Delimiter,
    },
    PreviewStatement {
        name: String,
        bytes: Vec<u8>,
        mapping: crate::statements::StatementMapping,
    },
    ImportStatement {
        name: String,
        bytes: Vec<u8>,
        mapping: crate::statements::StatementMapping,
        preview_token: String,
        save_profile_name: Option<String>,
        expected_revision: u64,
    },
    AddEntity {
        entity: EntityInput,
        reason: String,
        expected_revision: u64,
    },
    UpdateEntity {
        id: String,
        entity: EntityInput,
        reason: String,
        expected_revision: u64,
    },
    AddObservation {
        observation: ObservationInput,
        reason: String,
        expected_revision: u64,
    },
    CorrectObservation {
        id: String,
        value: String,
        anchor: SourceAnchor,
        reason: String,
        expected_revision: u64,
    },
    ReviewObservation {
        id: String,
        state: ReviewState,
        reason: String,
        expected_revision: u64,
    },
    CompareEntities {
        left_id: String,
        right_id: String,
    },
    InspectSource {
        anchor: SourceAnchor,
    },
    DecideIdentity {
        left_id: String,
        right_id: String,
        outcome: IdentityOutcome,
        reason: String,
        expected_revision: u64,
    },
    ReviewTransaction {
        id: String,
        state: ReviewState,
        reason: String,
        expected_revision: u64,
    },
    CorrectTransaction {
        id: String,
        amount: String,
        reason: String,
        expected_revision: u64,
    },
    MatchTransfer {
        first: String,
        second: String,
        reason: String,
        expected_revision: u64,
    },
    Merge {
        source: String,
        target: String,
        reason: String,
        expected_revision: u64,
    },
    ReverseMerge {
        id: String,
        reason: String,
        expected_revision: u64,
    },
    AddQuestion {
        question: HypothesisInput,
        reason: String,
        expected_revision: u64,
    },
    UpdateQuestion {
        id: String,
        question: HypothesisInput,
        reason: String,
        expected_revision: u64,
    },
    UpdateFinding {
        id: String,
        finding: FindingInput,
        reason: String,
        expected_revision: u64,
    },
    ReviewFinding {
        id: String,
        reason: String,
        expected_revision: u64,
    },
    AddFinding {
        #[serde(default)]
        hypothesis_ids: Vec<String>,
        title: String,
        assessment: String,
        supporting_ids: Vec<String>,
        contradicting_ids: Vec<String>,
        limitations: String,
        expected_revision: u64,
    },
    SaveReport {},
    Backup {},
}
