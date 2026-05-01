import enMessages from "../../../../messages/en.json";
import koMessages from "../../../../messages/ko.json";
import zhMessages from "../../../../messages/zh.json";

export const DEFAULT_LOCALE = "ko";
export const SUPPORTED_LOCALES = ["ko", "en", "zh"] as const;

export type SupportedLocale = (typeof SUPPORTED_LOCALES)[number];

export const messagesByLocale: Record<SupportedLocale, Record<string, unknown>> = {
  en: enMessages,
  ko: koMessages,
  zh: zhMessages,
};

export function isSupportedLocale(locale: string | undefined): locale is SupportedLocale {
  return !!locale && SUPPORTED_LOCALES.includes(locale as SupportedLocale);
}

export function getSafeLocale(locale: string | undefined): SupportedLocale {
  return isSupportedLocale(locale) ? locale : DEFAULT_LOCALE;
}
