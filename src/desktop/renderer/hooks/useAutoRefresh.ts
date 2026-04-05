import { useEffect } from "react";
import { useRouter } from "@/desktop/renderer/navigation";

let boardHasMountedBefore = false;

export function useAutoRefresh() {
  const router = useRouter();

  useEffect(() => {
    if (boardHasMountedBefore) {
      router.refresh();
    }
    boardHasMountedBefore = true;
  }, [router]);
}
