
export interface ReleaseUpdate {
  version: string;
  tagName: string;
  name: string;
  body: string;
  htmlUrl: string;
  publishedAt: string | null;
}

export interface ReleaseUpdateCheckResult {
  currentVersion: string;
  isUpdateAvailable: boolean;
  release: ReleaseUpdate | null;
  error?: string;
}
