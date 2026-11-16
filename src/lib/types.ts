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

export interface PasteAnalysis {
  rows: { status: PasteRowStatus; message: string | null; changed: string[] }[];
  counts: PasteCounts;
  problems: string[];
  elapsedMs: number;
}

export type AiProvider = "claude" | "codex";

export interface CliStatus {
  provider: AiProvider;
  installed: boolean;
  path: string | null;
  version: string | null;
  authenticated: boolean | null;
}

export interface AppError {
  kind: "invalid" | "not_found" | "database" | "store" | "secret" | "unsupported" | "conflict" | "io" | "unknown";
  message: string;
}
