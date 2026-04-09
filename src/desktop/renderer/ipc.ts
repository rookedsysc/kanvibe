import type { DesktopServiceNamespace } from "@/desktop/main/serviceRegistry";

export function invokeDesktop<T>(
  namespace: DesktopServiceNamespace,
  method: string,
  ...args: unknown[]
): Promise<T> {
  return window.kanvibeDesktop!.invoke(namespace, method, args) as Promise<T>;
}
