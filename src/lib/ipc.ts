import { Channel, invoke } from "@tauri-apps/api/core";

import type {
  Account,
  AccountDetails,
  AccountRef,
  AgentEvent,
  AiProvider,
  AlterDatabase,
  AppError,
  ApplyResult,
  AuditEntry,
  BackupAgent,
  BackupFormat,
  BackupJob,
  BackupPlan,
  BackupRun,
  BackupSettings,
  CellContent,
  ChangeSet,
  ChatRequest,
  CliStatus,
  ConnectionProfile,
  ConnectionView,
  DatabaseDetails,
  DatabaseOptions,
  DbObject,
  ExportFormat,
  FileReadOptions,
  BatchOutcome,
  Comparison,
  CopyBatch,
  CopyPreview,
  FileTable,
  FlushKind,
  HistoryEntry,
  ModelChoice,
  Namespace,
  NewDatabase,
  ObjectKind,
  PasswordChange,
  PasteAnalysis,
  OnProblem,
  ParamValue,
  PasteRequest,
  Plan,
  PickedFile,
  QueryPreview,
  RestorePlan,
  RowCount,
  RowFilter,
  RowPage,
  RunOutcome,
  SavedScript,
  ServerInfo,
  ServerProcess,
  TypeChoice,
  NewTable,
  IndexInfo,
  ColumnDefinition,
  FindMode,
  NameHit,
  TableFind,
  ForeignKeyInfo,
  AlterTable,
  EndProcess,
  SortSpec,
  SqlFile,
  SqlStatement,
  Statement,
  TableChange,
  TableColumns,
  TableDefinition,
  TableRef,
  UniqueKey,
  UpdateInfo,
  UpdateProgress,
  UserSpec,
  UserStep,
  ViewSpec,
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
  fetchRows: (id: string, table: TableRef, page: { offset: number; limit: number; sort: SortSpec | null; filters: RowFilter[]; matchAny: boolean }) =>
    invoke<RowPage>("fetch_rows", { id, table, page }),
  /** Counts on the server; `cancelJob(job)` interrupts it (the call then fails with `conflict`). */
  countRows: (id: string, table: TableRef, filters: RowFilter[], matchAny: boolean, job: string) =>
    invoke<RowCount>("count_rows", { id, table, filters, matchAny, job }),
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
  /** Reads the whole table on the server; `cancelJob(job)` interrupts it (the call then fails with `conflict`). */
  findInTable: (id: string, table: TableRef, text: string, mode: FindMode, job: string) =>
    invoke<TableFind>("find_in_table", { id, table, text, mode, job }),
  findNames: (id: string, database: string, schema: string | null, text: string) =>
    invoke<NameHit[]>("find_names", { id, database, schema, text }),
  describeTable: (id: string, table: TableRef) => invoke<TableColumns>("describe_table", { id, table }),
  analyzeSql: (id: string, sql: string) => invoke<SqlStatement[]>("analyze_sql", { id, sql }),
  runSql: (
    id: string,
    tab: string,
    database: string | null,
    sql: string,
    options: { maxRows: number; changeLimit: number | null },
    params?: Record<string, ParamValue>,
  ) => invoke<RunOutcome>("run_sql", { id, tab, database, sql, options, params: params ?? null }),
  /** The named parameters (`:name`) of a script, each once, in order. */
  sqlParameters: (id: string, sql: string) => invoke<string[]>("sql_parameters", { id, sql }),
  /** EXPLAIN one statement on a connection of its own; `analyze` runs it (reads only). `cancelJob(job)` stops it. */
  explainSql: (id: string, database: string | null, sql: string, params: Record<string, ParamValue> | null, analyze: boolean, job: string) =>
    invoke<Plan>("explain_sql", { id, database, sql, params, analyze, job }),
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
  /** Shows the native Open dialog for a SQLite file and gives its path. `null` when it was cancelled. */
  pickDatabaseFile: () => invoke<string | null>("pick_database_file"),
  readTableFile: (token: string, options: FileReadOptions) => invoke<FileTable>("read_table_file", { token, options }),
  /** Rows `offset..offset + limit` of a file too large for one run, as read by `readTableFile`. */
  tableFileRows: (token: string, options: FileReadOptions, offset: number, limit: number) =>
    invoke<string[][]>("table_file_rows", { token, options, offset, limit }),
  /** One batch of a large import, in one transaction; `cancelJob(job)` stops it. */
  importBatch: (id: string, table: TableRef, request: PasteRequest, onProblem: OnProblem, job: string) =>
    invoke<BatchOutcome>("import_batch", { id, table, request, onProblem, job }),
  /** Structure compare of database (or PostgreSQL schema) A with B, of the same engine. Reads only. */
  compareStructure: (aId: string, aDatabase: string, aSchema: string | null, bId: string, bDatabase: string, bSchema: string | null) =>
    invoke<Comparison>("compare_structure", { aId, aDatabase, aSchema, bId, bDatabase, bSchema }),
  /** The first `limit` rows of `from`, analysed against `to` as a paste. Writes nothing. */
  copyPreview: (sourceId: string, from: TableRef, columns: string[], targetId: string, to: TableRef, request: PasteRequest, limit: number) =>
    invoke<CopyPreview>("copy_preview", { sourceId, from, columns, targetId, to, request, limit }),
  /** Rows `offset..offset + limit` of `from`, written to `to` as a batch; `cancelJob(job)` stops it. */
  copyRows: (
    sourceId: string,
    from: TableRef,
    columns: string[],
    targetId: string,
    to: TableRef,
    request: PasteRequest,
    onProblem: OnProblem,
    offset: number,
    limit: number,
    job: string,
  ) => invoke<CopyBatch>("copy_rows", { sourceId, from, columns, targetId, to, request, onProblem, offset, limit, job }),
  databaseOptions: (id: string) => invoke<DatabaseOptions>("database_options", { id }),
  /** The exact statement `createDatabase` would run. Runs nothing. */
  planCreateDatabase: (id: string, spec: NewDatabase) => invoke<string>("plan_create_database", { id, spec }),
  createDatabase: (id: string, spec: NewDatabase) => invoke<string>("create_database", { id, spec }),
  databaseDetails: (id: string, name: string) => invoke<DatabaseDetails>("database_details", { id, name }),
  /** The exact statements `alterDatabase` would run, in order. Runs nothing. */
  planAlterDatabase: (id: string, spec: AlterDatabase) => invoke<string[]>("plan_alter_database", { id, spec }),
  alterDatabase: (id: string, spec: AlterDatabase) => invoke<string[]>("alter_database", { id, spec }),
  planDropDatabase: (id: string, name: string) => invoke<string>("plan_drop_database", { id, name }),
  /** `typed` is what the user typed to confirm; the backend checks it against `name`. */
  dropDatabase: (id: string, name: string, typed: string) => invoke<string>("drop_database", { id, name, typed }),
  tableDefinition: (id: string, table: TableRef, kind: ObjectKind) => invoke<TableDefinition>("table_definition", { id, table, kind }),
  /** The exact statement `applyTableChange` would run. Runs nothing. */
  planTableChange: (id: string, change: TableChange) => invoke<string>("plan_table_change", { id, change }),
  /** `typed` is what the user typed to confirm emptying or dropping; the backend checks it against the name. */
  applyTableChange: (id: string, change: TableChange, typed: string | null) =>
    invoke<string>("apply_table_change", { id, change, typed }),
  /** The query of a view as the server keeps it now. */
  viewSelect: (id: string, table: TableRef) => invoke<string>("view_select", { id, table }),
  /** The exact statements `saveView` would run, in order. Runs nothing. */
  planView: (id: string, spec: ViewSpec) => invoke<string[]>("plan_view", { id, spec }),
  saveView: (id: string, spec: ViewSpec) => invoke<string[]>("save_view", { id, spec }),
  /** The first rows of a view's query, read on a connection that cannot write. */
  previewViewSelect: (id: string, database: string, select: string) =>
    invoke<QueryPreview>("preview_view_select", { id, database, select }),
  listAccounts: (id: string) => invoke<Account[]>("list_accounts", { id }),
  accountDetails: (id: string, account: AccountRef) => invoke<AccountDetails>("account_details", { id, account }),
  /** The statements `createUser` would run. `reveal`: with the password instead of the mask. Runs nothing. */
  planUser: (id: string, spec: UserSpec, reveal: boolean) => invoke<UserStep[]>("plan_user", { id, spec, reveal }),
  /** All of it or none. Answers with the statements that ran, the password masked. */
  createUser: (id: string, spec: UserSpec) => invoke<UserStep[]>("create_user", { id, spec }),
  planPassword: (id: string, account: AccountRef, password: string, reveal: boolean) =>
    invoke<string>("plan_password", { id, account, password, reveal }),
  changePassword: (id: string, account: AccountRef, password: string) =>
    invoke<string>("change_password", { id, account, password }),
  planDropUser: (id: string, account: AccountRef) => invoke<string>("plan_drop_user", { id, account }),
  /** `typed` is what the user typed to confirm; the backend checks it against the account's name. */
  dropUser: (id: string, account: AccountRef, typed: string) => invoke<string>("drop_user", { id, account, typed }),
  planFlush: (id: string, what: FlushKind) => invoke<string>("plan_flush", { id, what }),
  flushServer: (id: string, what: FlushKind) => invoke<string>("flush_server", { id, what }),
  /** Shows the native Open dialog for a SQL file and gives its text. `null` when it was cancelled. */
  pickSqlFile: () => invoke<SqlFile | null>("pick_sql_file"),
  columnTypes: (id: string) => invoke<TypeChoice[]>("column_types", { id }),
  /** The statements `createTable` would run, in order. Runs nothing. */
  planCreateTable: (id: string, spec: NewTable) => invoke<string[]>("plan_create_table", { id, spec }),
  createTable: (id: string, spec: NewTable) => invoke<string[]>("create_table", { id, spec }),
  columnDefinitions: (id: string, table: TableRef) => invoke<ColumnDefinition[]>("column_definitions", { id, table }),
  foreignKeys: (id: string, table: TableRef) => invoke<ForeignKeyInfo[]>("foreign_keys", { id, table }),
  tableIndexes: (id: string, table: TableRef) => invoke<IndexInfo[]>("table_indexes", { id, table }),
  /** The statements `alterTable` would run. Runs nothing. */
  planAlterTable: (id: string, spec: AlterTable) => invoke<string[]>("plan_alter_table", { id, spec }),
  /** `confirm`: the table's name as typed, needed when a column is dropped. */
  alterTable: (id: string, spec: AlterTable, confirm: string | null) => invoke<string[]>("alter_table", { id, spec, confirm }),
  listProcesses: (id: string) => invoke<ServerProcess[]>("list_processes", { id }),
  /** The statement `endProcess` would run. Runs nothing. */
  planEndProcess: (id: string, process: number, what: EndProcess) => invoke<string>("plan_end_process", { id, process, what }),
  endProcess: (id: string, process: number, what: EndProcess) => invoke<string>("end_process", { id, process, what }),
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
  /** What the backup dialog of a database shows: the dump tool, the saved settings, the history. */
  backupPlan: (id: string, database: string) => invoke<BackupPlan>("backup_plan", { id, database }),
  /** Shows the native dialog for a folder. `null` when it was cancelled. */
  pickBackupFolder: () => invoke<string | null>("pick_backup_folder"),
  /** Saves the settings and installs or removes the background runner to match every schedule. */
  saveBackupJob: (id: string, database: string, settings: BackupSettings) =>
    invoke<{ job: BackupJob; agent: BackupAgent; nextRun: string | null }>("save_backup_job", { id, database, settings }),
  /** Backs the database up now. Resolves with the run, kept or failed; `cancelJob(job)` stops it. */
  runBackup: (
    id: string,
    database: string,
    target: { destination: string; format: BackupFormat; compress: boolean },
    job: string,
    onProgress: (bytes: number) => void,
  ) => {
    const channel = new Channel<number>();
    channel.onmessage = onProgress;
    return invoke<BackupRun>("run_backup", { id, database, target, job, onProgress: channel });
  },
  listBackupRuns: (id: string | null, database: string | null, limit: number) =>
    invoke<BackupRun[]>("list_backup_runs", { id, database, limit }),
  /** Shows the file of a run in Finder. */
  revealBackup: (run: number) => invoke<void>("reveal_backup", { run }),
  /** What restoring this backup into a new database of that name would do. Runs nothing. */
  restorePlan: (id: string, run: number, database: string) => invoke<RestorePlan>("restore_plan", { id, run, database }),
  /** Creates the database and loads the backup into it. `cancelJob(job)` stops the load. */
  restoreBackup: (id: string, run: number, database: string, job: string, onProgress: (bytes: number) => void) => {
    const channel = new Channel<number>();
    channel.onmessage = onProgress;
    return invoke<{ database: string; statement: string; durationMs: number }>("restore_backup", { id, run, database, job, onProgress: channel });
  },
  /** Asks the release manifest for a newer version. `null`: this is the latest. */
  checkUpdate: () => invoke<UpdateInfo | null>("check_update"),
  /** Downloads, verifies and installs what the last check found. The app keeps running. */
  installUpdate: (onProgress: (progress: UpdateProgress) => void) => {
    const channel = new Channel<UpdateProgress>();
    channel.onmessage = onProgress;
    return invoke<void>("install_update", { onProgress: channel });
  },
  restartApp: () => invoke<void>("restart_app"),
};
