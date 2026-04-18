import { useTranslations } from "next-intl";
import { useRouter } from "@/desktop/renderer/navigation";

export default function NotFoundRoute() {
  const t = useTranslations("common");
  const router = useRouter();

  return (
    <div className="min-h-screen flex items-center justify-center bg-bg-page px-4">
      <div className="flex max-w-md flex-col items-center gap-4 text-center">
        <div className="rounded-full border border-border-default bg-bg-surface px-3 py-1 text-xs font-medium text-text-muted">
          404
        </div>
        <div>
          <h1 className="text-xl font-semibold text-text-primary">{t("notFoundTitle")}</h1>
          <p className="mt-2 text-sm text-text-secondary">{t("notFoundDescription")}</p>
        </div>
        <button
          type="button"
          onClick={() => router.back()}
          className="rounded-md border border-border-default bg-bg-surface px-4 py-2 text-sm text-text-secondary transition-colors hover:border-brand-primary hover:text-text-primary"
        >
          {t("goBack")}
        </button>
      </div>
    </div>
  );
}
