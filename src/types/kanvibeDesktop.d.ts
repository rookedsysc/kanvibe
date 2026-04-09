interface KanvibeDesktopNotificationPayload {
  title: string;
  body: string;
  taskId?: string;
  locale: string;
}

interface KanvibeDesktopApi {
  isDesktop: boolean;
  showNotification?: (payload: KanvibeDesktopNotificationPayload) => Promise<boolean>;
  [key: string]: any;
}

interface Window {
  kanvibeDesktop?: KanvibeDesktopApi;
}
