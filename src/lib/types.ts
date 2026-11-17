// Mirrors the serde types in crates/core. Keep the two in sync.

export type Engine = "mysql" | "postgres" | "sqlserver";
export type Environment = "development" | "staging" | "production";
export type TlsMode = "disable" | "prefer" | "require" | "verify-ca" | "verify-full";

export interface ConnectionProfile {
  id: string;
  name: string;
  engine: Engine;
  environment: Environment;
  host: string;
  port: number;
  username: string;
  database: string | null;
  tls: TlsMode;
  tlsCaPath: string | null;
  readOnly: boolean;
  favorite: boolean;
  tags: string[];
  connectTimeoutSecs: number;
  bulkReviewRows: number;
  createdAt: number;
  updatedAt: number;
  lastUsedAt: number | null;
}

export interface ConnectionView extends ConnectionProfile {
  hasPassword: boolean;
}

export type PasswordChange =
  | { action: "keep" }
  | { action: "set"; value: string }
  | { action: "clear" };

export interface Capabilities {
  schemas: boolean;
  transactionalDdl: boolean;
  returning: boolean;
}

export interface ServerInfo {
  engine: Engine;
  version: string;
  capabilities: Capabilities;
  tls: boolean | null;
  readOnly: boolean;
  connectMs: number;
}

export interface Namespace {
  name: string;
  system: boolean;
}

export type ObjectKind = "table" | "view" | "matview";

export interface DbObject {
  name: string;
  kind: ObjectKind;
}

export interface TableRef {
  database: string;
  schema: string | null;
  name: string;
}

export type ValueKind = "number" | "text" | "bool" | "temporal" | "binary" | "json" | "other";

export interface ColumnMeta {
  name: string;
  dataType: string;
  kind: ValueKind;
  nullable: boolean;
  default: string | null;
  primaryKey: boolean;
  generated: boolean;
  editable: boolean;
}

export interface SortSpec {
  column: string;
  descending: boolean;
}

/** One cell read in full (`db::CellContent`). */
export interface CellContent {
  value: string | null;
  /** Even this was cut: the value is longer than the backend sends for one cell. */
  cut: boolean;
}

export type FilterOp =
  | "equals"
  | "notEquals"
  | "contains"
  | "notContains"
  | "startsWith"
  | "endsWith"
  | "greater"
  | "greaterOrEqual"
  | "less"
  | "lessOrEqual"
  | "isNull"
  | "isNotNull";

/** One condition on the rows of a table. All conditions of a request must hold. */
export interface RowFilter {
  column: string;
  op: FilterOp;
  /** `null` for the NULL tests, which take no value. */
  value: string | null;
}

export interface RowPage {
  columns: ColumnMeta[];
  rows: (string | null)[][];
  truncated: [number, number][];
  primaryKey: string[];
  estimatedRows: number | null;
  offset: number;
  hasMore: boolean;
  orderedBy: string[];
  elapsedMs: number;
}

export interface CellValue {
  column: string;
  value: string | null;
}

export interface ChangeSet {
  updates: { key: CellValue[]; changes: { column: string; original: string | null; value: string | null }[] }[];
  inserts: { values: CellValue[] }[];
  deletes: { key: CellValue[] }[];
}

export interface Statement {
  kind: "update" | "insert" | "delete";
  sql: string;
  params: (string | null)[];
  row: string;
}

export interface ApplyResult {
  updated: number;
  inserted: number;
  deleted: number;
  elapsedMs: number;
}

export type PasteMode = "insert" | "update" | "upsert";

export interface PasteRequest {
  mode: PasteMode;
  columns: string[];
  key: string[];
  update: string[];
  rows: (string | null)[][];
}

export interface UniqueKey {
  name: string;
  columns: string[];
  primary: boolean;
}

export type PasteRowStatus =
  | "insert"
  | "update"
  | "unchanged"
  | "unmatched"
  | "missing_key"
  | "duplicate"
  | "exists"
  | "invalid";

export interface PasteCounts {
  insert: number;
  update: number;
  unchanged: number;
  unmatched: number;
  problems: number;
}

export type FileKind = "text" | "workbook";

/** A file chosen in the native Open dialog. The backend keeps its path; this is its handle. */
export interface PickedFile {
  token: string;
  name: string;
  kind: FileKind;
}

export interface FileReadOptions {
  /** Workbooks: the sheet to read (the first one by default). */
  sheet?: string;
  /** Text: an encoding label such as `windows-874`. Without it the file must be UTF-8 or UTF-16. */
  encoding?: string;
  /** Text: the character between fields. Detected when absent. */
  delimiter?: string;
}

/** Rows of text read from a CSV file or a workbook sheet (`table_file::FileTable`). */
export interface FileTable {
  kind: FileKind;
  rows: string[][];
  /** Rows in the file; more than `rows.length` when it holds more than one run can take. */
  totalRows: number;
  sheets: string[];
  sheet: string | null;
  encoding: string | null;
  delimiter: string | null;
}

export interface PasteAnalysis {
  rows: { status: PasteRowStatus; message: string | null; changed: string[] }[];
  counts: PasteCounts;
  /** Identifies this exact outcome; a run is accepted only if a fresh analysis matches it. */
  fingerprint: string;
  problems: string[];
  elapsedMs: number;
}

export type SqlKind = "read" | "dml" | "ddl" | "transaction" | "other";

export interface SqlStatement {
  text: string;
  start: number;
  kind: SqlKind;
  danger: string | null;
}

export interface ResultColumn {
  name: string;
  typeName: string;
  kind: ValueKind;
}

export interface ResultPreview {
  id: number;
  columns: ResultColumn[];
  rows: (string | null)[][];
  truncated: [number, number][];
  capped: boolean;
}

export type SqlStatus = "ok" | "error" | "blocked" | "cancelled";

export interface StatementOutcome {
  sql: string;
  kind: SqlKind;
  status: SqlStatus;
  results: ResultPreview[];
  rowsAffected: number | null;
  message: string | null;
  elapsedMs: number;
}

export interface RunOutcome {
  statements: StatementOutcome[];
  skipped: number;
  resume: string | null;
  inTransaction: boolean;
  /** The tab's connection died during the run; the next run opens a new one. */
  connectionLost: boolean;
  elapsedMs: number;
}

export interface HistoryEntry {
  id: number;
  connectionId: string;
  database: string | null;
  sql: string;
  status: SqlStatus;
  summary: string;
  elapsedMs: number;
  startedAt: number;
}

export type ExportFormat = "csv" | "json" | "xlsx";

export interface TableColumns {
  columns: ColumnMeta[];
  primaryKey: string[];
}

export type AiProvider = "claude" | "codex";

export interface CliStatus {
  provider: AiProvider;
  installed: boolean;
  path: string | null;
  version: string | null;
  authenticated: boolean | null;
}

/** What the backend streams while an agent turn runs. */
export type AgentEvent =
  | { type: "started"; model: string | null }
  | { type: "delta"; text: string }
  | { type: "message"; text: string }
  /** The model called a tool; `input` is the SQL or the name it asked about. */
  | { type: "toolStarted"; id: string; name: string; input: string }
  /** `output` is exactly the text the model was given. */
  | { type: "toolFinished"; id: string; ok: boolean; output: string }
  /** SQL the model wants run. Nothing has run: the user decides. */
  | { type: "proposal"; id: string; sql: string; reason: string | null; statements: SqlStatement[] };

/** What the AI may do with the database in front: nothing, look at its structure, or also read rows. */
export type AgentAccess = "none" | "schema" | "data";

/** The database a turn's tools are bound to. */
export interface ToolScope {
  connectionId: string;
  database: string;
  schema: string | null;
  /** The model may read rows, not only the catalogue. */
  readRows: boolean;
}

export type AuditEvent = "read" | "read_failed" | "proposed" | "approved" | "rejected" | "executed" | "blocked" | "failed";

/** One line of the AI activity log. */
export interface AuditEntry {
  id: number;
  at: number;
  connectionId: string;
  connectionName: string;
  database: string;
  provider: AiProvider;
  model: string | null;
  event: AuditEvent;
  sql: string;
  detail: string;
}

export interface ModelChoice {
  id: string;
  label: string;
  description: string | null;
}

export interface ChatMessage {
  role: "user" | "assistant";
  text: string;
}

export interface ChatRequest {
  provider: AiProvider;
  /** `null` leaves the choice to the CLI. */
  model: string | null;
  /** The schema context, exactly as shown in the context inspector. */
  context: string | null;
  messages: ChatMessage[];
  /** `null`: the model gets no tools at all. */
  tools: ToolScope | null;
}

export interface AppError {
  kind: "invalid" | "not_found" | "database" | "store" | "secret" | "unsupported" | "conflict" | "io" | "unknown";
  message: string;
}
