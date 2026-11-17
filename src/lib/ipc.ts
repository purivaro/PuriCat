import { Channel, invoke } from "@tauri-apps/api/core";

import type {
  AgentEvent,
  AiProvider,
  AppError,
  ApplyResult,
  AuditEntry,
  CellContent,
  ChangeSet,
  ChatRequest,
  CliStatus,
  ConnectionProfile,
  ConnectionView,
  DbObject,
  ExportFormat,
  FileReadOptions,
  FileTable,
  HistoryEntry,
  ModelChoice,
  Namespace,
  PasswordChange,
  PasteAnalysis,
  PickedFile,
  RowFilter,
  PasteRequest,
  RowPage,
  SavedScript,
  RunOutcome,
  ServerInfo,
  SortSpec,
  SqlStatement,
  Statement,
  TableColumns,
  TableRef,
  UniqueKey,
} from "./types";

/** Backend errors arrive as `{ kind, message }`; anything else is normalised to that shape. */
export function toAppError(e: unknown): AppError {
  if (e && typeof e === "object" && "message" in e && "kind" in e) return e as AppError;
  return { kind: "unknown", message: typeof e === "string" ? e : String((e as Error)?.message ?? e) };
}

export function errorMessage(e: unknown): string {
  return toAppError(e).message;
}

export const ipc = {
  listConnections: () => invoke<ConnectionView[]>("list_connections"),
  saveConnection: (profile: ConnectionProfile, password: PasswordChange) =>
    invoke<{ connection: ConnectionView; sessionClosed: boolean }>("save_connection", { profile, password }),
  duplicateConnection: (id: string, name: string) =>
    invoke<ConnectionView>("duplicate_connection", { id, name }),
  deleteConnection: (id: string) => invoke<void>("delete_connection", { id }),
  testConnection: (profile: ConnectionProfile, password: PasswordChange) =>
    invoke<ServerInfo>("test_connection", { profile, password }),
  openConnection: (id: string) => invoke<ServerInfo>("open_connection", { id }),
  closeConnection: (id: string) => invoke<void>("close_connection", { id }),
  listDatabases: (id: string) => invoke<Namespace[]>("list_databases", { id }),
  listSchemas: (id: string, database: string) => invoke<Namespace[]>("list_schemas", { id, database }),
  listObjects: (id: string, database: string, schema: string | null) =>
    invoke<DbObject[]>("list_objects", { id, database, schema }),
  fetchRows: (id: string, table: TableRef, page: { offset: number; limit: number; sort: SortSpec | null; filters: RowFilter[] }) =>
    invoke<RowPage>("fetch_rows", { id, table, page }),
  fetchCell: (id: string, table: TableRef, key: { column: string; value: string }[], column: string) =>
    invoke<CellContent>("fetch_cell", { id, table, key, column }),
  planChanges: (id: string, table: TableRef, changes: ChangeSet) =>
    invoke<Statement[]>("plan_changes", { id, table, changes }),
  applyChanges: (id: string, table: TableRef, changes: ChangeSet) =>
    invoke<ApplyResult>("apply_changes", { id, table, changes }),
  uniqueKeys: (id: string, table: TableRef) => invoke<UniqueKey[]>("unique_keys", { id, table }),
  analyzePaste: (id: string, table: TableRef, request: PasteRequest) =>
    invoke<PasteAnalysis>("analyze_paste", { id, table, request }),
  /** Runs a previewed paste. `onProgress` receives the number of statements done so far. */
  applyPaste: (
    id: string,
    table: TableRef,
    request: PasteRequest,
    expected: string,
    job: string,
    onProgress: (done: number) => void,
  ) => {
    const channel = new Channel<number>();
    channel.onmessage = onProgress;
    return invoke<ApplyResult>("apply_paste", { id, table, request, expected, job, onProgress: channel });
  },
  cancelJob: (job: string) => invoke<void>("cancel_job", { job }),
  describeTable: (id: string, table: TableRef) => invoke<TableColumns>("describe_table", { id, table }),
  analyzeSql: (id: string, sql: string) => invoke<SqlStatement[]>("analyze_sql", { id, sql }),
  runSql: (
    id: string,
    tab: string,
    database: string | null,
    sql: string,
    options: { maxRows: number; changeLimit: number | null },
  ) => invoke<RunOutcome>("run_sql", { id, tab, database, sql, options }),
  cancelSql: (tab: string) => invoke<void>("cancel_sql", { tab }),
  closeConsole: (tab: string) => invoke<void>("close_console", { tab }),
  listHistory: (id: string, limit: number) => invoke<HistoryEntry[]>("list_history", { id, limit }),
  clearHistory: (id: string) => invoke<void>("clear_history", { id }),
  listScripts: (id: string) => invoke<SavedScript[]>("list_scripts", { id }),
  getScript: (script: number) => invoke<SavedScript>("get_script", { script }),
  createScript: (id: string, name: string, sql: string) => invoke<SavedScript>("create_script", { id, name, sql }),
  updateScript: (script: number, sql: string) => invoke<SavedScript>("update_script", { script, sql }),
  renameScript: (script: number, name: string) => invoke<SavedScript>("rename_script", { script, name }),
  deleteScript: (script: number) => invoke<void>("delete_script", { script }),
  exportResult: (tab: string, result: number, format: ExportFormat, path: string) =>
    invoke<number>("export_result", { tab, result, format, path }),
  /** Shows the native Open dialog (CSV, Excel). `null` when it was cancelled. */
  pickTableFile: () => invoke<PickedFile | null>("pick_table_file"),
  readTableFile: (token: string, options: FileReadOptions) => invoke<FileTable>("read_table_file", { token, options }),
  getSettings: () => invoke<Record<string, string>>("get_settings"),
  setSetting: (key: string, value: string) => invoke<void>("set_setting", { key, value }),
  detectAiClis: () => invoke<CliStatus[]>("detect_ai_clis"),
  agentModels: (provider: AiProvider) => invoke<ModelChoice[]>("agent_models", { provider }),
  /** The context text for a chat: schema names and types, plus the active query if given. */
  agentContext: (id: string, database: string, schema: string | null, sql: string | null) =>
    invoke<string>("agent_context", { id, database, schema, sql }),
  /** One chat turn. Resolves with the whole answer; `cancelJob(job)` stops it. */
  agentSend: (job: string, request: ChatRequest, onEvent: (event: AgentEvent) => void) => {
    const channel = new Channel<AgentEvent>();
    channel.onmessage = onEvent;
    return invoke<string>("agent_send", { job, request, onEvent: channel });
  },
  /** The user approved a proposal: PuriCat runs it. `liftLimit` runs what the row limit held back. */
  agentProposalRun: (id: string, liftLimit: boolean) => invoke<RunOutcome>("agent_proposal_run", { id, liftLimit }),
  agentProposalReject: (id: string) => invoke<void>("agent_proposal_reject", { id }),
  agentAudit: (limit: number) => invoke<AuditEntry[]>("agent_audit", { limit }),
  clearAgentAudit: () => invoke<void>("clear_agent_audit"),
};
