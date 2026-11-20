// Mirrors the serde types in crates/core. Keep the two in sync.

export type Engine = "mysql" | "postgres" | "sqlite" | "sqlserver";
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
  /** SQLite only: the database file, as an absolute path. */
  file: string | null;
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
  | "isNotNull"
  | "in"
  | "notIn"
  | "between"
  | "isEmpty"
  | "isNotEmpty";

/** One condition on the rows of a table. The conditions of a request must all hold, or any one of them. */
export interface RowFilter {
  column: string;
  op: FilterOp;
  /** `null` for the operators that take no value, a list or a range. */
  value: string | null;
  /** The list of "is one of" / "is not one of", or the two ends of "is between". */
  values?: string[];
}

/** The rows of a table, or those a filter leaves: counted on the server, not estimated. */
export interface RowCount {
  count: number;
  elapsedMs: number;
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
  /** Leave out the rows that have a problem and run the rest. */
  skipProblems?: boolean;
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

export interface PasteRowReport {
  status: PasteRowStatus;
  message: string | null;
  changed: string[];
  /** The other pasted row a duplicate collides with: an index into the request's rows. */
  other: number | null;
}

export interface PasteAnalysis {
  rows: PasteRowReport[];
  counts: PasteCounts;
  /** Identifies this exact outcome; a run is accepted only if a fresh analysis matches it. */
  fingerprint: string;
  problems: string[];
  elapsedMs: number;
}

export type SqlKind = "read" | "dml" | "ddl" | "transaction" | "other";

export interface SqlStatement {
  text: string;
  /** Byte offset (UTF-8) of the statement in the script. */
  start: number;
  /** Byte offset of its first word, after the comments that lead it. */
  body: number;
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

/** A script saved for one connection, stored exactly as written. */
export interface SavedScript {
  id: number;
  connectionId: string;
  name: string;
  sql: string;
  createdAt: number;
  updatedAt: number;
}

export type ExportFormat = "csv" | "json" | "xlsx";

export interface TableColumns {
  columns: ColumnMeta[];
  primaryKey: string[];
  /** Planner statistics, not an exact count. */
  estimatedRows: number | null;
}

/** The statements that would create a table or a view as it is now. */
export interface TableDefinition {
  sql: string;
  /** The server's own statement, text put together from the catalogue, or the text in a SQLite file. */
  source: "server" | "catalogue" | "file";
}

/** A change to one table or view, from a form of the explorer's table menu. */
export type TableChange =
  | { action: "rename"; table: TableRef; kind: ObjectKind; newName: string }
  | { action: "empty"; table: TableRef }
  | { action: "drop"; table: TableRef; kind: ObjectKind };

/** A view to create, or to give another query, from the view form. */
export interface ViewSpec {
  /** Where the view is, and its name. */
  table: TableRef;
  /** The view's query only: what follows `AS`. */
  select: string;
  /** The view exists and gets this query instead of its own. */
  replace: boolean;
}

/** The first rows of a query that was only tried, with how long that took. */
export interface QueryPreview extends ResultPreview {
  elapsedMs: number;
}

/** An account as the server names it: a MySQL user at a host, or a PostgreSQL role. */
export interface AccountRef {
  name: string;
  /** MySQL only. */
  host?: string | null;
}

/** An account in the list of "Users and privileges". */
export interface Account {
  name: string;
  host: string | null;
  /** The account this connection is logged in as. */
  you: boolean;
  /** One of the server's own accounts: shown, not changed. */
  system: boolean;
  /** MySQL: the account is locked. PostgreSQL: the role cannot log in. */
  locked: boolean;
  superuser: boolean;
}

export interface RoleAttributes {
  login: boolean;
  superuser: boolean;
  createDatabase: boolean;
  createRole: boolean;
  replication: boolean;
  bypassRls: boolean;
  /** -1: no limit. */
  connectionLimit: number;
  validUntil: string | null;
}

export interface DatabaseAccess {
  name: string;
  owner: boolean;
  connect: boolean;
  create: boolean;
  temporary: boolean;
}

/** What one account may do, as far as this version reads it. */
export interface AccountDetails {
  /** MySQL: `SHOW GRANTS`, one statement each. */
  grants: string[];
  plugin: string | null;
  passwordExpired: boolean;
  /** PostgreSQL. */
  attributes: RoleAttributes | null;
  memberOf: string[];
  members: string[];
  databases: DatabaseAccess[];
}

/** What a new user is for: everything in one database, reading one database, or nothing yet. */
export type UserTemplate = "app" | "readOnly" | "none";

/** A user to create. The password travels to the backend only. */
export interface UserSpec {
  account: AccountRef;
  template: UserTemplate;
  database: string | null;
  password: string;
}

/** One statement of a user form, in the order it runs. */
export interface UserStep {
  sql: string;
  /** PostgreSQL: the database the statement runs inside; `null`: the server as a whole. */
  database: string | null;
}

/** A database to create. Only the fields of the connection's engine are sent. */
export interface NewDatabase {
  name: string;
  charset?: string | null;
  collation?: string | null;
  owner?: string | null;
  encoding?: string | null;
  template?: string | null;
}

export interface DatabaseOptions {
  charsets: { name: string; defaultCollation: string; description: string }[];
  collations: { name: string; charset: string }[];
  defaultCharset: string | null;
  defaultCollation: string | null;
  owners: string[];
  currentUser: string | null;
  templates: string[];
  encodings: string[];
  defaultEncoding: string | null;
}

/** Changes to a database. Only what differs from the database as it is now is sent. */
export interface AlterDatabase {
  /** The database as the server lists it now. */
  name: string;
  charset?: string | null;
  collation?: string | null;
  newName?: string | null;
  owner?: string | null;
}

export interface DatabaseDetails {
  name: string;
  charset: string | null;
  collation: string | null;
  owner: string | null;
  encoding: string | null;
  /** The server's own database: shown, not changed. */
  protected: boolean;
  /** Tables and views it holds; `null` when they could not be counted. */
  tables: number | null;
}

/** MySQL server maintenance offered in a connection's menu. */
export type FlushKind = "privileges" | "tables" | "logs" | "status";

/** A `.sql` file chosen in the native Open dialog. */
export interface SqlFile {
  name: string;
  text: string;
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

export type BackupFormat = "sql" | "custom";

/** When a backup runs by itself, in local time. `days`: 1 = Monday … 7 = Sunday. */
export type BackupSchedule =
  | { kind: "off" }
  | { kind: "daily"; hour: number; minute: number }
  | { kind: "weekly"; days: number[]; hour: number; minute: number };

export interface DumpTool {
  path: string;
  client: "mysql" | "mariadb" | "postgres";
  version: string;
  major: number;
}

export interface BackupJob {
  id: number;
  connectionId: string;
  database: string;
  destination: string;
  format: BackupFormat;
  compress: boolean;
  schedule: BackupSchedule;
  /** Scheduled backups to keep; 0 keeps all. */
  keep: number;
  updatedAt: number;
}

export interface BackupRun {
  id: number;
  startedAt: number;
  finishedAt: number;
  connectionId: string;
  connectionName: string;
  database: string;
  scheduled: boolean;
  status: "ok" | "failed" | "stopped";
  file: string | null;
  bytes: number | null;
  tool: string;
  message: string;
  /** When the retention rule deleted the file. */
  removedAt: number | null;
}

/** What restoring a backup into a new database would do, before anything runs. */
export interface RestorePlan {
  /** The exact `CREATE DATABASE` statement. */
  statement: string;
  /** The program that loads the backup, with its version: `mysql 9.3.0`. */
  client: string;
  clientPath: string;
  /** A failed or stopped load leaves the new database empty (PostgreSQL) or not (MySQL). */
  allOrNothing: boolean;
}

export interface BackupAgent {
  supported: boolean;
  needed: boolean;
  installed: boolean;
  problem: string | null;
}

export interface BackupPlan {
  serverVersion: string;
  choice: { tool: DumpTool | null; problem: string | null; warning: string | null };
  job: BackupJob | null;
  defaultDestination: string;
  runs: BackupRun[];
  agent: BackupAgent;
  /** Local time `YYYY-MM-DDTHH:MM`, only when the schedule will really run. */
  nextRun: string | null;
}

export interface BackupSettings {
  destination: string;
  format: BackupFormat;
  compress: boolean;
  schedule: BackupSchedule;
  keep: number;
}

/** A newer version found by the last check. */
export interface UpdateInfo {
  version: string;
  currentVersion: string;
  /** RFC 3339, when the release says. */
  date: string | null;
  notes: string | null;
}

export interface UpdateProgress {
  downloaded: number;
  /** Bytes in all, when the server said. */
  total: number | null;
}

export type TypeParams =
  | { kind: "none" }
  | { kind: "length"; required: boolean; default: number; max: number }
  | { kind: "precision"; defaultPrecision: number; defaultScale: number; max: number };

export interface TypeChoice {
  name: string;
  params: TypeParams;
  family: "integer" | "number" | "text" | "time" | "other";
  /** DEFAULT CURRENT_TIMESTAMP is allowed. */
  timestamp: boolean;
}

export type ColumnDefault = { kind: "none" } | { kind: "null" } | { kind: "value"; value: string } | { kind: "currentTimestamp" };

export interface ColumnSpec {
  name: string;
  type: string;
  length: number | null;
  scale: number | null;
  unsigned: boolean;
  nullable: boolean;
  default: ColumnDefault;
  auto: boolean;
  key: boolean;
  comment: string | null;
}

export interface IndexInfo {
  name: string;
  columns: string[];
  unique: boolean;
  primary: boolean;
}

export type FindMode = "contains" | "equals" | "startsWith";

/** What one table holds of the value looked for. */
export interface TableFind {
  /** The columns it was looked for in. */
  searched: string[];
  /** The columns that hold it, with their rows: only those with one or more. */
  hits: { column: string; rows: number }[];
  /** Rows in which any searched column holds it. */
  rows: number;
  elapsedMs: number;
}

export interface NameHit {
  schema: string | null;
  table: string;
  kind: ObjectKind;
  /** `null`: the table's own name matched. */
  column: string | null;
  dataType: string | null;
}

export interface ColumnDefinition {
  name: string;
  columnType: string;
  nullable: boolean;
  default: string | null;
  defaultExpression: boolean;
  extra: string;
  comment: string | null;
  charset: string | null;
  collation: string | null;
  generated: boolean;
}

export type FkAction = "noAction" | "restrict" | "cascade" | "setNull";

export interface ForeignKeyInfo {
  name: string;
  columns: string[];
  references: TableRef;
  refColumns: string[];
  onDelete: FkAction | null;
  onUpdate: FkAction | null;
}

export interface NewType {
  type: string;
  length: number | null;
  scale: number | null;
  unsigned: boolean;
}

export type TableEdit =
  | { kind: "addColumn"; column: ColumnSpec }
  | { kind: "dropColumn"; name: string }
  | { kind: "renameColumn"; from: string; to: string }
  | { kind: "addIndex"; name: string | null; columns: string[]; unique: boolean }
  | { kind: "dropIndex"; name: string }
  | { kind: "changeColumn"; name: string; type?: NewType; nullable?: boolean; default?: ColumnDefault; comment?: string }
  | { kind: "addForeignKey"; name: string | null; columns: string[]; references: TableRef; refColumns: string[]; onDelete: FkAction; onUpdate: FkAction }
  | { kind: "dropForeignKey"; name: string };

export interface AlterTable {
  table: TableRef;
  edits: TableEdit[];
}

export interface NewTable {
  table: TableRef;
  columns: ColumnSpec[];
  comment: string | null;
}

/** One session on a server, as the server reports it. */
export interface ServerProcess {
  id: number;
  user: string | null;
  host: string | null;
  database: string | null;
  /** MySQL's COMMAND, PostgreSQL's state. */
  command: string | null;
  seconds: number | null;
  /** MySQL's STATE, PostgreSQL's wait event. */
  state: string | null;
  query: string | null;
  /** PostgreSQL's application_name. */
  client: string | null;
  /** The session that read the list. */
  own: boolean;
  idle: boolean;
}

export type EndProcess = "query" | "connection";

export interface AppError {
  kind: "invalid" | "not_found" | "database" | "store" | "secret" | "unsupported" | "conflict" | "io" | "unknown";
  message: string;
}
