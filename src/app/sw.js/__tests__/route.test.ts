import { describe, it, expect, vi, beforeEach } from "vitest";
import { GET } from "../route";

describe("Service Worker Route Handler", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("should return Service Worker script as JavaScript response", async () => {
    // Given
    // When
    const response = await GET();

    // Then
    expect(response.status).toBe(200);
    expect(response.headers.get("Content-Type")).toBe("application/javascript");
    expect(response.headers.get("Cache-Control")).toBe("public, max-age=0");
  });

  it("should include notificationclick event listener in service worker code", async () => {
    // Given
    // When
    const response = await GET();
    const code = await response.text();

    // Then
    expect(code).toContain("self.addEventListener('notificationclick'");
    expect(code).toContain("event.notification.close()");
    expect(code).toContain("clients.openWindow");
  });

  it("should include install and activate event listeners", async () => {
    // Given
    // When
    const response = await GET();
    const code = await response.text();

    // Then
    expect(code).toContain("self.addEventListener('install'");
    expect(code).toContain("self.addEventListener('activate'");
    expect(code).toContain("self.skipWaiting()");
    expect(code).toContain("clients.claim()");
  });

  it("service worker code should extract taskId and locale from notification data", async () => {
    // Given
    // When
    const response = await GET();
    const code = await response.text();

    // Then
    expect(code).toContain("event.notification.data?.taskId");
    expect(code).toContain("event.notification.data?.locale");
  });

  it("service worker code should generate absolute URL using self.location.origin", async () => {
    // Given
    // When
    const response = await GET();
    const code = await response.text();

    // Then
    // Check for URL construction logic
    expect(code).toContain("new URL(relativePath, self.location.origin)");
    expect(code).toContain("const absoluteUrl");
    expect(code).toContain("const relativePath");
  });

  it("service worker code should warn when taskId is missing", async () => {
    // Given
    // When
    const response = await GET();
    const code = await response.text();

    // Then
    expect(code).toContain("console.warn('[SW] taskId not found");
  });

  it("service worker code should use matchAll to find existing windows", async () => {
    // Given
    // When
    const response = await GET();
    const code = await response.text();

    // Then
    expect(code).toContain("clients.matchAll");
    expect(code).toContain("type: 'window'");
  });

  it("service worker code should focus existing window if found", async () => {
    // Given
    // When
    const response = await GET();
    const code = await response.text();

    // Then
    expect(code).toContain("client.focus()");
    expect(code).toContain("client.url.includes");
  });

  it("service worker code should handle clients.openWindow result", async () => {
    // Given
    // When
    const response = await GET();
    const code = await response.text();

    // Then
    // Should have error handling for openWindow
    expect(code).toContain("clients.openWindow(absoluteUrl)");
    expect(code).toContain(".then((result)");
    expect(code).toContain(".catch((err)");
  });

  it("service worker code should include logging for debugging", async () => {
    // Given
    // When
    const response = await GET();
    const code = await response.text();

    // Then
    expect(code).toContain("console.log('[SW]");
    expect(code).toContain("console.error('[SW]");
    expect(code).toContain("[SW] Notification clicked");
    expect(code).toContain("[SW] Opening URL");
    expect(code).toContain("[SW] Found");
    expect(code).toContain("[SW] Window opened");
  });

  it("service worker code should use event.waitUntil", async () => {
    // Given
    // When
    const response = await GET();
    const code = await response.text();

    // Then
    expect(code).toContain("event.waitUntil");
  });

  it("should handle try-catch for error handling", async () => {
    // Given
    // When
    const response = await GET();
    const code = await response.text();

    // Then
    expect(code).toContain("try");
    expect(code).toContain("catch (err)");
    expect(code).toContain("console.error('[SW] Error");
  });
});
