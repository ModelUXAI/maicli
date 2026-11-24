import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { DiffView, isUnifiedDiff, parseDiff } from "./DiffView";

const SAMPLE = `diff --git a/src/main.ts b/src/main.ts
index 83db48f..bf269f4 100644
--- a/src/main.ts
+++ b/src/main.ts
@@ -1,3 +1,3 @@
 const a = 1;
-const b = 2;
+const b = 3;
 const c = 4;
`;

describe("DiffView", () => {
  it("mengenali diff unified", () => {
    expect(isUnifiedDiff(SAMPLE)).toBe(true);
    expect(isUnifiedDiff("halo dunia")).toBe(false);
  });

  it("baris + dan - punya dua gutter nomor dan kelas warna benar", () => {
    const { container } = render(<DiffView text={SAMPLE} />);
    const rows = [...container.querySelectorAll("tr")];
    const added = rows.find((row) => row.textContent?.includes("const b = 3"));
    const removed = rows.find((row) => row.textContent?.includes("const b = 2"));

    expect(added).toBeDefined();
    expect(removed).toBeDefined();

    const addedCells = [...(added?.querySelectorAll("td") ?? [])];
    expect(addedCells).toHaveLength(4);
    expect(addedCells[0].textContent).toBe("—");
    expect(addedCells[1].textContent).toBe("2");
    expect(addedCells[2].textContent).toBe("+");
    expect(added?.className).toContain("diff-add");

    const removedCells = [...(removed?.querySelectorAll("td") ?? [])];
    expect(removedCells[0].textContent).toBe("2");
    expect(removedCells[1].textContent).toBe("—");
    expect(removedCells[2].textContent).toBe("-");
    expect(removed?.className).toContain("diff-del");
  });

  it("hunk header memakai kelas surface-secondary", () => {
    const { container } = render(<DiffView text={SAMPLE} />);
    const hunk = [...container.querySelectorAll("tr")].find((row) =>
      row.textContent?.includes("@@ -1,3 +1,3 @@"),
    );
    expect(hunk?.className).toContain("text-secondary");
    expect(screen.getByText(/END OF HUNK 1/)).toBeInTheDocument();
  });

  it("input diff cacat tidak melempar", () => {
    const broken = "@@ tanpa angka @@\n+baris tambah tanpa header\n-barisl\nacak-acakan";
    expect(() => parseDiff(broken)).not.toThrow();
    expect(() => render(<DiffView text={broken} />)).not.toThrow();
    expect(() => parseDiff("")).not.toThrow();
    expect(() => parseDiff("--- a\n+++ b\n@@\n")).not.toThrow();
  });

  it("baris di luar hunk dirender sebagai konteks dengan nomor urut", () => {
    const parsed = parseDiff("baris satu\nbaris dua");
    expect(parsed.lines).toHaveLength(2);
    expect(parsed.lines[0]).toMatchObject({ kind: "context", oldNo: 1, newNo: 1 });
    expect(parsed.lines[1]).toMatchObject({ kind: "context", oldNo: 2, newNo: 2 });
  });
});
