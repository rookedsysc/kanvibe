import { useEffect, useState } from "react";

function isWindowCurrentlyActive(): boolean {
  return document.visibilityState !== "hidden" && document.hasFocus();
}

/**
 * 창이 뒤에 가려져 있으면 화면이 바뀌어도 볼 사람이 없으므로 폴링을 멈춘다.
 * 창을 여러 개 띄우는 사용 방식이라 창 수만큼 곱해지는 비용이고,
 * 원격에서는 SSH ControlMaster lease를 점유해 실제 git 작업과 경합한다.
 *
 * Electron은 창이 다른 창에 가려진 것만으로는 `visibilitychange`를 내지 않아 포커스도 함께 본다.
 *
 * 첫 값도 실제 창 상태에서 읽는다. 활성으로 가정해 두면 뒤에 가려진 창이 마운트되는 첫 렌더에서만
 * 활성으로 보여, 포커스가 왔을 때만 해야 하는 일이 백그라운드에서도 한 번 실행된다.
 */
export function useIsWindowActive(): boolean {
  const [isWindowActive, setIsWindowActive] = useState(isWindowCurrentlyActive);

  useEffect(() => {
    const syncWindowActive = () => {
      setIsWindowActive(isWindowCurrentlyActive());
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
