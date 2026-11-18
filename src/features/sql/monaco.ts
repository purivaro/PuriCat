// Monaco set-up for the SQL editor: only the core editor and the SQL grammars are loaded,
// and this whole module is imported lazily, so the editor costs nothing until a query tab opens.

// Editor features are opt-in in this build of Monaco: take what a SQL editor needs.
import "monaco-editor/features/codicon/register";
import "monaco-editor/features/suggest/register";
import "monaco-editor/features/snippet/register";
import "monaco-editor/features/find/register";
import "monaco-editor/features/bracketMatching/register";
import "monaco-editor/features/comment/register";
import "monaco-editor/features/linesOperations/register";
import "monaco-editor/features/clipboard/register";
import "monaco-editor/features/cursorUndo/register";
import "monaco-editor/features/multicursor/register";
import "monaco-editor/features/wordOperations/register";
import "monaco-editor/features/wordHighlighter/register";
import "monaco-editor/features/indentation/register";
import "monaco-editor/features/folding/register";
import "monaco-editor/languages/definitions/sql/register";
import "monaco-editor/languages/definitions/mysql/register";
import "monaco-editor/languages/definitions/pgsql/register";

import * as monaco from "monaco-editor/editor/editor.api";
import EditorWorker from "monaco-editor/editor/editor.worker?worker";

import type { Engine } from "../../lib/types";

(self as unknown as { MonacoEnvironment: monaco.Environment }).MonacoEnvironment = {
  getWorker: () => new EditorWorker(),
};

export const LANGUAGE: Record<Engine, string> = { mysql: "mysql", postgres: "pgsql", sqlserver: "sql" };

let swatch: CanvasRenderingContext2D | null | undefined;

/**
 * A design token as `#rrggbb` / `#rrggbbaa`, the only form Monaco's themes accept. The
 * browser does the parsing, so any CSS colour works: the production build rewrites tokens
 * to their shortest spelling (`#ffffff` becomes `#fff`), which Monaco rejects.
 */
function css(name: string): string {
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  if (/^#[0-9a-f]{6}([0-9a-f]{2})?$/i.test(value)) return value;
  swatch ??= document.createElement("canvas").getContext("2d", { willReadFrequently: true });
  if (!swatch) return value;
  swatch.clearRect(0, 0, 1, 1);
  swatch.fillStyle = value;
  swatch.fillRect(0, 0, 1, 1);
  const [r, g, b, a] = swatch.getImageData(0, 0, 1, 1).data;
  const hex = (n = 0) => n.toString(16).padStart(2, "0");
  return `#${hex(r)}${hex(g)}${hex(b)}${a === 255 ? "" : hex(a)}`;
}

/** Rebuild the editor theme from the app's design tokens (call again when the theme flips). */
export function applyTheme() {
  const dark = document.documentElement.dataset.theme === "dark";
  monaco.editor.defineTheme("puricat", {
    base: dark ? "vs-dark" : "vs",
    inherit: true,
    rules: [
      { token: "keyword", foreground: dark ? "ffa45c" : "c2500a", fontStyle: "bold" },
      { token: "operator.sql", foreground: dark ? "a9a9ae" : "5f6368" },
      { token: "string", foreground: dark ? "5bc27a" : "188038" },
      { token: "number", foreground: dark ? "74a9f7" : "1a6fd8" },
      { token: "comment", foreground: dark ? "7b7b82" : "8a8f98", fontStyle: "italic" },
    ],
    colors: {
      "editor.background": css("--surface"),
      "editor.foreground": css("--fg"),
      "editorLineNumber.foreground": css("--fg-subtle"),
      "editorLineNumber.activeForeground": css("--fg-muted"),
      "editor.lineHighlightBackground": css("--sunken"),
      "editor.selectionBackground": dark ? "#ff8a3044" : "#ff7a1833",
      "editorCursor.foreground": css("--accent"),
      "editorWidget.background": css("--raised"),
      "editorWidget.border": css("--border"),
      "editorSuggestWidget.selectedBackground": dark ? "#ff8a3033" : "#fff1e6",
      "editorIndentGuide.background1": css("--border"),
    },
  });
  monaco.editor.setTheme("puricat");
}

export interface Schema {
  /** Table and view names the editor may suggest. */
  tables: () => string[];
  /** Column names of one table, fetched on demand. */
  columns: (table: string) => Promise<string[]>;
}

/** Completion sources per editor model; the providers below are registered once. */
const schemas = new Map<string, Schema>();

const KEYWORDS =
  "SELECT FROM WHERE AND OR NOT IN IS NULL LIKE BETWEEN EXISTS AS ON JOIN LEFT RIGHT INNER OUTER CROSS GROUP BY ORDER HAVING LIMIT OFFSET DISTINCT UNION ALL INSERT INTO VALUES UPDATE SET DELETE CREATE TABLE VIEW INDEX ALTER ADD COLUMN DROP TRUNCATE PRIMARY KEY FOREIGN REFERENCES DEFAULT UNIQUE CASE WHEN THEN ELSE END ASC DESC COUNT SUM AVG MIN MAX COALESCE CAST WITH RETURNING BEGIN COMMIT ROLLBACK EXPLAIN SHOW DESCRIBE"
    .split(" ");

let registered = false;
function registerCompletion() {
  if (registered) return;
  registered = true;
  for (const language of new Set(Object.values(LANGUAGE))) {
    monaco.languages.registerCompletionItemProvider(language, {
      triggerCharacters: ["."],
      provideCompletionItems: async (model, position) => {
        const word = model.getWordUntilPosition(position);
        const range = new monaco.Range(position.lineNumber, word.startColumn, position.lineNumber, word.endColumn);
        const schema = schemas.get(model.uri.toString());
        const tables = schema?.tables() ?? [];
        const before = model.getLineContent(position.lineNumber).slice(0, word.startColumn - 1);
        const item = (label: string, kind: monaco.languages.CompletionItemKind, detail?: string) => ({
          label,
          kind,
          detail,
          insertText: label,
          range,
        });

        // `table.` -> that table's columns only.
        const qualifier = /([A-Za-z_][\w$]*)\.$/.exec(before)?.[1];
        if (qualifier && schema) {
          const table = tables.find((t) => t.toLowerCase() === qualifier.toLowerCase());
          const columns = table ? await schema.columns(table) : [];
          return { suggestions: columns.map((c) => item(c, monaco.languages.CompletionItemKind.Field, table)) };
        }

        // Otherwise: tables, columns of the tables already mentioned in the text, keywords.
        const text = model.getValue();
        const mentioned = tables.filter((t) => new RegExp(`\\b${t.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\b`, "i").test(text)).slice(0, 6);
        const columns = schema ? (await Promise.all(mentioned.map((t) => schema.columns(t).then((cs) => cs.map((c) => [c, t] as const))))).flat() : [];
        const seen = new Set<string>();
        return {
          suggestions: [
            ...tables.map((t) => item(t, monaco.languages.CompletionItemKind.Struct, "table")),
            ...columns.filter(([c]) => !seen.has(c) && seen.add(c)).map(([c, t]) => item(c, monaco.languages.CompletionItemKind.Field, t)),
            ...KEYWORDS.map((k) => item(k, monaco.languages.CompletionItemKind.Keyword)),
          ],
        };
      },
    });
  }
}

export interface EditorOptions {
  container: HTMLElement;
  engine: Engine;
  value: string;
  schema: Schema;
  onChange: (value: string) => void;
  onRun: () => void;
}

export function createEditor(o: EditorOptions): monaco.editor.IStandaloneCodeEditor {
  registerCompletion();
  applyTheme();
  const model = monaco.editor.createModel(o.value, LANGUAGE[o.engine]);
  schemas.set(model.uri.toString(), o.schema);
  const editor = monaco.editor.create(o.container, {
    model,
    theme: "puricat",
    automaticLayout: true,
    fontFamily: '"JetBrains Mono Variable", "Noto Sans Thai Variable", ui-monospace, Menlo, monospace',
    fontLigatures: false,
    fontSize: 13,
    lineHeight: 21,
    minimap: { enabled: false },
    scrollBeyondLastLine: false,
    renderLineHighlight: "line",
    padding: { top: 10, bottom: 10 },
    lineNumbersMinChars: 3,
    tabSize: 2,
    wordWrap: "on",
    contextmenu: false, // the browser-style menu offers actions the app does not back
    quickSuggestions: true,
    scrollbar: { verticalScrollbarSize: 10, horizontalScrollbarSize: 10 },
  });
  editor.onDidChangeModelContent(() => o.onChange(editor.getValue()));
  // An action, not `addCommand`: a command's key binding is shared by every editor in the
  // page, and the last editor created answers for all of them. An action belongs to its editor.
  const run = editor.addAction({ id: "puricat.run", label: "Run", keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter], run: o.onRun });
  editor.onDidDispose(() => {
    run.dispose();
    schemas.delete(model.uri.toString());
    model.dispose();
  });
  return editor;
}

/** The selected text, or the whole script when nothing is selected. */
export function textToRun(editor: monaco.editor.IStandaloneCodeEditor): string {
  const selection = editor.getSelection();
  const selected = selection && !selection.isEmpty() ? editor.getModel()?.getValueInRange(selection) : "";
  return selected?.trim() ? selected : editor.getValue();
}

/** Replace the selection (or everything) through the undo stack. */
export function replaceText(editor: monaco.editor.IStandaloneCodeEditor, text: string, whole: boolean) {
  const model = editor.getModel();
  if (!model) return;
  const selection = editor.getSelection();
  const range = whole || !selection || selection.isEmpty() ? model.getFullModelRange() : selection;
  editor.executeEdits("puricat", [{ range, text, forceMoveMarkers: true }]);
  editor.focus();
}
