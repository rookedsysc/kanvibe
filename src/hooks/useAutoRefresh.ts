"use client";

import { useEffect, useRef } from "react";
import { useRouter } from "@/i18n/navigation";

const RECONNECT_DELAY_MS = 3000;

/** Board 컴포넌트가 이전에 마운트된 적이 있는지 추적하는 모듈 레벨 플래그 */
let boardHasMountedBefore = false;

/**
 * WebSocket을 통한 보드 자동 새로고침 + 뒤로가기 시 최신 데이터 로드.
 * 보드 페이지에서 호출하면 Hook API 변경 사항이 실시간으로 반영된다.
 */
export function useAutoRefresh() {
  const router = useRouter();
  const wsRef = useRef<WebSocket | null>(null);
  const reconnectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  /** 뒤로가기 등으로 Board가 재마운트되면 라우터 캐시를 무효화하여 최신 데이터를 로드한다 */
  useEffect(() => {
    if (boardHasMountedBefore) {
      router.refresh();
    }
    boardHasMountedBefore = true;
  }, [router]);

  useEffect(() => {
    function connect() {
      const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
      const port = process.env.NEXT_PUBLIC_WS_PORT
        ? parseInt(process.env.NEXT_PUBLIC_WS_PORT, 10)
        : parseInt(window.location.port || "4885", 10) + 2;
      const wsUrl = `${protocol}//${window.location.hostname}:${port}/api/board/events`;

      const ws = new WebSocket(wsUrl);
      wsRef.current = ws;

      ws.onmessage = (event) => {
        try {
          const data = JSON.parse(event.data);
          if (data.type === "board-updated") {
            router.refresh();
          }
        } catch {
          /* 파싱 실패 무시 */
        }
      };

      ws.onclose = () => {
        wsRef.current = null;
        reconnectTimerRef.current = setTimeout(connect, RECONNECT_DELAY_MS);
      };

      ws.onerror = () => {
        ws.close();
      };
    }

    connect();

    return () => {
      if (reconnectTimerRef.current) {
        clearTimeout(reconnectTimerRef.current);
      }
      wsRef.current?.close();
    };
  }, [router]);
}
