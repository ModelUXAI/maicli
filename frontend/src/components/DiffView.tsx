// Diff unified dua kolom nomor (referensi 09). Parser murni string dan
// toleran: input cacat tetap dirender sebagai konteks, tidak pernah melempar.

export interface DiffLine {
  kind: "add" | "del" | "context" | "hunk" | "meta";
  oldNo: number | null;
  newNo: number | null;
  text: string;
}

export interface ParsedDiff {
  lines: DiffLine[];
  hunks: number;
  isDiff: boolean;
}

const HUNK_RE = /^@@\s*-(\d+)(?:,\d+)?\s*\+(\d+)(?:,\d+)?\s*@@/;

export function isUnifiedDiff(text: string): boolean {
  return /^---\s+\S+/m.test(text) && /^\+\+\+\s+\S+/m.test(text) && /^@@/m.test(text);
}

export function parseDiff(text: string): ParsedDiff {
  const raw = text.split("\n");
  const lines: DiffLine[] = [];
  let oldNo = 1;
  let newNo = 1;
  let hunks = 0;
  let sawHunk = false;

  for (const line of raw) {
    const hunk = HUNK_RE.exec(line);
    if (hunk) {
      oldNo = Number.parseInt(hunk[1], 10);
      newNo = Number.parseInt(hunk[2], 10);
      hunks += 1;
      sawHunk = true;
      lines.push({ kind: "hunk", oldNo: null, newNo: null, text: line });
      continue;
    }
    if (
      line.startsWith("diff --git") ||
      line.startsWith("index ") ||
      line.startsWith("--- ") ||
      line.startsWith("+++ ") ||
      line.startsWith("new file mode") ||
      line.startsWith("deleted file mode") ||
      line.startsWith("similarity index") ||
      line.startsWith("rename ")
    ) {
      lines.push({ kind: "meta", oldNo: null, newNo: null, text: line });
      continue;
    }
    if (line.startsWith("\\ No newline")) {
      lines.push({ kind: "meta", oldNo: null, newNo: null, text: line });
      continue;
    }
    if (line.startsWith("+")) {
      lines.push({ kind: "add", oldNo: null, newNo: newNo++, text: line.slice(1) });
      continue;
    }
    if (line.startsWith("-")) {
      lines.push({ kind: "del", oldNo: oldNo++, newNo: null, text: line.slice(1) });
      continue;
    }
    if (line.startsWith(" ")) {
      lines.push({ kind: "context", oldNo: oldNo++, newNo: newNo++, text: line.slice(1) });
      continue;
    }
    lines.push({ kind: "context", oldNo: oldNo++, newNo: newNo++, text: line });
  }

  return { lines, hunks, isDiff: sawHunk };
}

export function DiffView({ text }: { text: string }) {
  const parsed = parseDiff(text);
  const visible = parsed.lines.filter((line) => line.kind !== "meta");

  return (
    <div className="overflow-x-auto">
      <table className="w-full border-collapse font-mono text-code-inline">
        <tbody>
          {visible.map((line, index) => (
            <tr
              key={`${index}-${line.text.slice(0, 24)}`}
              className={
                line.kind === "add"
                  ? "diff-add"
                  : line.kind === "del"
                    ? "diff-del"
                    : line.kind === "hunk"
                      ? "bg-surface-container-high/70 text-secondary"
                      : ""
              }
            >
              <td className="w-10 select-none border-r border-outline-variant/40 pr-1 text-right align-top text-outline-variant">
                {line.oldNo ?? "—"}
              </td>
              <td className="w-10 select-none border-r border-outline-variant/40 pr-1 text-right align-top text-outline-variant">
                {line.newNo ?? "—"}
              </td>
              <td className="w-6 select-none text-center align-top">
                {line.kind === "add" ? "+" : line.kind === "del" ? "-" : " "}
              </td>
              <td className="whitespace-pre-wrap break-all align-top">{line.text}</td>
            </tr>
          ))}
        </tbody>
      </table>
      {parsed.hunks > 0 ? (
        <div className="border-t border-outline-variant px-sm py-0.5 text-label-sm text-outline">
          ╰─ END OF HUNK {parsed.hunks}
        </div>
      ) : null}
    </div>
  );
}
