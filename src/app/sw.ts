// Service Worker route handler
// Next.js Route Handler로 /sw.js를 제공한다

export const runtime = "nodejs";

export async function GET() {
  const swCode = `
// Service Worker: 브라우저 알림 클릭 핸들러
self.addEventListener('notificationclick', (event) => {
  // 알림 데이터에서 taskId 추출
  const taskId = event.notification.data?.taskId;
  const locale = event.notification.data?.locale || 'ko';

  // 알림 닫기
  event.notification.close();

  if (!taskId) {
    console.warn('[SW] taskId not found in notification data');
    return;
  }

  // redirect URL 생성
  const redirectUrl = \`/\${locale}/task/\${taskId}\`;

  // 기존 창이 있으면 포커스, 없으면 새 창 열기
  event.waitUntil(
    clients.matchAll({ type: 'window' }).then((clientList) => {
      // 이미 열려있는 같은 URL의 창이 있으면 포커스
      for (const client of clientList) {
        if (client.url.includes(\`/task/\${taskId}\`) && 'focus' in client) {
          return client.focus();
        }
      }
      // 없으면 새 창 열기
      if (clients.openWindow) {
        return clients.openWindow(redirectUrl);
      }
    })
  );
});
`;

  return new Response(swCode, {
    headers: {
      "Content-Type": "application/javascript",
      "Cache-Control": "public, max-age=3600",
    },
  });
}
