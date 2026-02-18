"use client";

import { useActionState } from "react";
import { useTranslations } from "next-intl";
import { loginAction } from "@/app/actions/auth";

export default function LoginForm() {
  const t = useTranslations("login");
  const [state, formAction, isPending] = useActionState(
    async (_prevState: { error: string } | null, formData: FormData) => {
      const result = await loginAction(formData);
      return result ?? null;
    },
    null
  );

  return (
    <div className="min-h-screen flex items-center justify-center bg-bg-page">
      <div className="w-full max-w-sm p-8 bg-bg-surface rounded-xl border border-border-default shadow-sm">
        <h1 className="text-2xl font-bold text-text-primary mb-6 text-center">
          KanVibe
        </h1>

        <form action={formAction} className="space-y-4">
          <div>
            <label
              htmlFor="username"
              className="block text-sm text-text-secondary mb-1"
            >
              {t("username")}
            </label>
            <input
              id="username"
              name="username"
              type="text"
              required
              className="w-full px-3 py-2 bg-bg-page border border-border-default rounded-md text-text-primary focus:outline-none focus:border-brand-primary transition-colors"
              placeholder={t("usernamePlaceholder")}
            />
          </div>

          <div>
            <label
              htmlFor="password"
              className="block text-sm text-text-secondary mb-1"
            >
              {t("password")}
            </label>
            <input
              id="password"
              name="password"
              type="password"
              required
              className="w-full px-3 py-2 bg-bg-page border border-border-default rounded-md text-text-primary focus:outline-none focus:border-brand-primary transition-colors"
              placeholder={t("passwordPlaceholder")}
            />
          </div>

          {state?.error && (
            <p className="text-status-error text-sm">{state.error}</p>
          )}

          <button
            type="submit"
            disabled={isPending}
            className="w-full py-2 bg-brand-primary hover:bg-brand-hover disabled:opacity-50 text-text-inverse rounded-md font-medium transition-colors"
          >
            {isPending ? t("loggingIn") : t("submit")}
          </button>
        </form>
      </div>
    </div>
  );
}
