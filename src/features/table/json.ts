// JSON laid out for reading, without reading it as values.

/**
 * A JSON object or array with one member per line, two spaces per level. Only the white
 * space between tokens changes: every string and number is copied exactly as it is written,
 * so a number too large or too precise for JavaScript (a 20-digit id, `1.10`) is not
 * rounded on the way. `null` when the text is not a JSON object or array.
 */
export function formatJson(text: string): string | null {
  const first = text.trimStart()[0];
  if (first !== "{" && first !== "[") return null;
  try {
    // Only to know that the text is well formed; its result is not used.
    JSON.parse(text);
  } catch {
    return null;
  }

  const isSpace = (c: string | undefined) => c === " " || c === "\n" || c === "\r" || c === "\t";
  let out = "";
  let depth = 0;
  const line = () => `\n${"  ".repeat(depth)}`;
  for (let i = 0; i < text.length; i++) {
    const c = text[i]!;
    if (c === '"') {
      // A string, copied whole: an escaped quote does not end it.
      let end = i + 1;
      while (text[end] !== '"') end += text[end] === "\\" ? 2 : 1;
      out += text.slice(i, end + 1);
      i = end;
    } else if (c === "{" || c === "[") {
      let next = i + 1;
      while (isSpace(text[next])) next++;
      if (text[next] === "}" || text[next] === "]") {
        // An empty object or array stays on its line.
        out += c + text[next];
        i = next;
      } else {
        depth++;
        out += c + line();
      }
    } else if (c === "}" || c === "]") {
      depth--;
      out += line() + c;
    } else if (c === ",") {
      out += c + line();
    } else if (c === ":") {
      out += ": ";
    } else if (!isSpace(c)) {
      out += c;
    }
  }
  return out;
}
