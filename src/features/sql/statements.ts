// Which statement of a script the cursor is in. The splitting itself is the backend's
// (`db::script::split`, which knows each dialect's strings and comments); this only places
// its answer in the editor's text.

/** One statement in the editor's text, in UTF-16 offsets. */
export interface Span {
  /**
   * Where its text starts, the comments above it included. A `--` comment that follows the
   * previous statement's semicolon on the same line is left to that statement.
   */
  start: number;
  /** Where the statement proper starts: its first word. */
  body: number;
  /** Where its text ends (exclusive), before the semicolon. */
  end: number;
}

/**
 * Where the backend's statements are in `script`. The backend counts bytes of UTF-8, the
 * editor counts UTF-16 units: the two differ as soon as the script holds Thai text.
 */
export function spans(script: string, statements: { start: number; body: number; text: string }[]): Span[] {
  let bytes = 0;
  let at = 0;
  const seek = (target: number) => {
    while (bytes < target && at < script.length) {
      const point = script.codePointAt(at)!;
      bytes += point < 0x80 ? 1 : point < 0x800 ? 2 : point < 0x10000 ? 3 : 4;
      at += point > 0xffff ? 2 : 1;
    }
    return at;
  };
  const out: Span[] = [];
  for (const s of statements) {
    const from = seek(s.start);
    const body = seek(s.body);
    const previous = out[out.length - 1];
    let start = from;
    if (previous && (script.startsWith("--", from) || script[from] === "#") && !script.slice(previous.end, from).includes("\n")) {
      const lineEnd = script.indexOf("\n", from);
      if (lineEnd >= 0 && lineEnd < body) start = lineEnd + 1;
    }
    out.push({ start, body, end: from + s.text.length });
  }
  return out;
}

/**
 * The statement the cursor belongs to, as an index into `list`; -1 when there is none.
 * From its first word to the end of the line it ends on (its semicolon, a comment after it)
 * it is that statement. On a later line it is the next one, comments above it included.
 */
export function statementAt(script: string, list: Span[], offset: number): number {
  let i = -1;
  while (i + 1 < list.length && list[i + 1]!.body <= offset) i++;
  if (i < 0) return list.length > 0 ? 0 : -1;
  if (offset <= list[i]!.end || i + 1 === list.length) return i;
  return script.slice(list[i]!.end, offset).includes("\n") ? i + 1 : i;
}
