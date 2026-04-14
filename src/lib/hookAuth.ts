export function buildCurlAuthHeader(authToken?: string): string {
  if (!authToken) {
    return "";
  }

  return `  -H "X-Kanvibe-Token: ${authToken}" \\\n+`;
}

export function buildFetchAuthHeaders(authToken?: string): string {
  if (!authToken) {
    return '{ "Content-Type": "application/json" }';
  }

  return `{ "Content-Type": "application/json", "X-Kanvibe-Token": "${authToken}" }`;
}
