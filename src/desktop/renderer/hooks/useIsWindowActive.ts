import { useEffect, useState } from "react";

/**
 * 창이 뒤에 가려져 있으면 화면이 바뀌어도 볼 사람이 없으므로 폴링을 멈춘다.
 * 창을 여러 개 띄우는 사용 방식이라 창 수만큼 곱해지는 비용이고,
 * 원격에서는 SSH ControlMaster lease를 점유해 실제 git 작업과 경합한다.
 *
 * Electron은 창이 다른 창에 가려진 것만으로는 `visibilitychange`를 내지 않아 포커스도 함께 본다.
 */
export function useIsWindowActive(): boolean {
  const [isWindowActive, setIsWindowActive] = useState(true);

  useEffect(() => {
    const syncWindowActive = () => {
      setIsWindowActive(document.visibilityState !== "hidden" && document.hasFocus());
    };

    syncWindowActive();
    document.addEventListener("visibilitychange", syncWindowActive);
    window.addEventListener("focus", syncWindowActive);
    window.addEventListener("blur", syncWindowActive);

    return () => {
      document.removeEventListener("visibilitychange", syncWindowActive);
      window.removeEventListener("focus", syncWindowActive);
      window.removeEventListener("blur", syncWindowActive);
    };
  }, []);

  return isWindowActive;
}
