import { describe, expect, it } from "vitest";
import type { Project } from "@/entities/Project";
import { matchesProjectSearch } from "../projectSearch";

const project = { name: "kanvibe", repoPath: "/code/my-project", sshHost: "devbox" } as Project;

describe("project fuzzy search", () => {
  it.each(["kvb", "cd/mpr", "dvbx", " KVB ", "", "   "])("matches %j", (query) => {
    expect(matchesProjectSearch(project, query)).toBe(true);
  });
  it.each(["bvk", "zzz", "kanvibee", "kvbdvbx"])("rejects %j", (query) => {
    expect(matchesProjectSearch(project, query)).toBe(false);
  });
  it("handles local hosts", () => {
    expect(matchesProjectSearch({ ...project, sshHost: null }, "dvbx")).toBe(false);
  });
});
