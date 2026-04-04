interface KanvibeDesktopNotificationPayload {
  title: string;
  body: string;
  taskId?: string;
  locale: string;
}

interface KanvibeDesktopApi {
  isDesktop: boolean;
  showNotification: (payload: KanvibeDesktopNotificationPayload) => Promise<boolean>;
}

interface Window {
  kanvibeDesktop?: KanvibeDesktopApi;
}
