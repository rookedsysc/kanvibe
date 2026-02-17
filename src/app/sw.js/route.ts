// Service Worker route handler
// Next.js Route Handler로 /sw.js를 제공한다

export const runtime = "nodejs";

export async function GET() {
  const swCode = `
// Service Worker: 브라우저 알림 클릭 핸들러
console.log('[SW] Service Worker script loaded');

self.addEventListener('install', (event) => {
  console.log('[SW] Service Worker installing...');
  self.skipWaiting();
});

self.addEventListener('activate', (event) => {
  console.log('[SW] Service Worker activating...');
  event.waitUntil(clients.claim());
});

self.addEventListener('notificationclick', (event) => {
  console.log('[SW] Notification clicked:', event.notification.data);
  
  const taskId = event.notification.data?.taskId;
  const locale = event.notification.data?.locale || 'ko';

  event.notification.close();

  if (!taskId) {
    console.warn('[SW] taskId not found in notification data');
    return;
  }

  const relativePath = \`/\${locale}/task/\${taskId}\`;
  
  try {
    const absoluteUrl = new URL(relativePath, self.location.origin).href;
    console.log('[SW] Opening URL:', absoluteUrl);

    event.waitUntil(
      clients.matchAll({ type: 'window' }).then((clientList) => {
        console.log('[SW] Found', clientList.length, 'open windows');
        
        // 이미 열려있는 같은 URL의 창이 있으면 포커스
        for (const client of clientList) {
          if (client.url.includes(\`/task/\${taskId}\`)) {
            console.log('[SW] Found matching window, focusing:', client.url);
            return client.focus();
          }
        }
        
        // 없으면 새 창 열기
        console.log('[SW] No matching window, opening new one');
        return clients.openWindow(absoluteUrl).then((result) => {
          if (result) {
            console.log('[SW] Window opened successfully:', result.url);
          } else {
            console.error('[SW] Failed to open window (result is null)');
          }
          return result;
        }).catch((err) => {
          console.error('[SW] Error opening window:', err);
        });
      }).catch((err) => {
        console.error('[SW] Error matching clients:', err);
      })
    );
  } catch (err) {
    console.error('[SW] Error in notification click handler:', err);
  }
});
`;

  return new Response(swCode, {
    headers: {
      "Content-Type": "application/javascript",
      "Cache-Control": "public, max-age=0",
    },
  });
}
