import type { Metadata } from "next";
import { NextIntlClientProvider, hasLocale } from "next-intl";
import { setRequestLocale, getMessages } from "next-intl/server";
import { notFound } from "next/navigation";
import { Inter } from "next/font/google";
import { routing } from "@/i18n/routing";
import NotificationListener from "@/components/NotificationListener";
import { getNotificationSettings } from "@/app/actions/appSettings";
import "../globals.css";

const inter = Inter({
  variable: "--font-inter",
  subsets: ["latin"],
});

export const metadata: Metadata = {
  title: "KanVibe",
  description: "AI Agent Task Management Kanban",
};

type Props = {
  children: React.ReactNode;
  params: Promise<{ locale: string }>;
};

export default async function LocaleLayout({ children, params }: Props) {
  const { locale } = await params;
  if (!hasLocale(routing.locales, locale)) {
    notFound();
  }

  setRequestLocale(locale);
  const [messages, notificationSettings] = await Promise.all([
    getMessages(),
    getNotificationSettings().catch(() => ({
      isEnabled: true,
      enabledStatuses: ["progress", "pending", "review"],
    })),
  ]);

  return (
    <html lang={locale}>
      <body suppressHydrationWarning className={`${inter.variable} font-sans bg-bg-page text-text-primary antialiased`}>
        <NextIntlClientProvider messages={messages}>
          <NotificationListener
            isNotificationEnabled={notificationSettings.isEnabled}
            enabledStatuses={notificationSettings.enabledStatuses}
          />
          {children}
        </NextIntlClientProvider>
      </body>
    </html>
  );
}

export function generateStaticParams() {
  return routing.locales.map((locale) => ({ locale }));
}
