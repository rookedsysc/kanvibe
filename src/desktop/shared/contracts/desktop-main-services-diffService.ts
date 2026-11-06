

export interface DiffFile {
  path: string;
  originalPath?: string;
  status: "added" | "modified" | "deleted" | "renamed";
  additions: number;
  deletions: number;
}
