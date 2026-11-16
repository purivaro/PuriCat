import { Channel, invoke } from "@tauri-apps/api/core";

import type {
  AppError,
  ApplyResult,
  ChangeSet,
  CliStatus,
  ConnectionProfile,
  ConnectionView,
  DbObject,
  Namespace,
  PasteAnalysis,
  PasteCounts,
  PasteRequest,
  PasswordChange,
  RowPage,
  ServerInfo,
  SortSpec,
  Statement,
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
  fetchRows: (id: string, table: TableRef, page: { offset: number; limit: number; sort: SortSpec | null }) =>
    invoke<RowPage>("fetch_rows", { id, table, page }),
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
    expected: PasteCounts,
    job: string,
    onProgress: (done: number) => void,
  ) => {
    const channel = new Channel<number>();
    channel.onmessage = onProgress;
    return invoke<ApplyResult>("apply_paste", { id, table, request, expected, job, onProgress: channel });
  },
  cancelJob: (job: string) => invoke<void>("cancel_job", { job }),
  getSettings: () => invoke<Record<string, string>>("get_settings"),
  setSetting: (key: string, value: string) => invoke<void>("set_setting", { key, value }),
  detectAiClis: () => invoke<CliStatus[]>("detect_ai_clis"),
};
