import { createI18n } from "vue-i18n"
import de from "@/locales/de.json"
import en from "@/locales/en.json"

export const messages = {
  en,
  de,
} as const

export const i18n = createI18n({
  legacy: false,
  locale: "en",
  fallbackLocale: "en",
  messages,
})

export type Locale = keyof typeof messages
