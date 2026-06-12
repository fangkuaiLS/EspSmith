import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import zh from "./locales/zh.json";
import en from "./locales/en.json";

// 从 localStorage 读取用户语言偏好，默认中文
const savedLang = localStorage.getItem("espsmith-lang");
const defaultLang = savedLang || "zh";

i18n.use(initReactI18next).init({
  resources: {
    zh: { translation: zh },
    en: { translation: en },
  },
  lng: defaultLang,
  fallbackLng: "zh",
  interpolation: {
    escapeValue: false, // React 已经安全处理了 XSS
    prefix: '{',
    suffix: '}',
  },
});

// 语言切换工具函数
export function switchLanguage(lang: "zh" | "en") {
  i18n.changeLanguage(lang);
  localStorage.setItem("espsmith-lang", lang);
}

export function getCurrentLanguage(): "zh" | "en" {
  return (i18n.language?.startsWith("zh") ? "zh" : "en") as "zh" | "en";
}

/**
 * Translate backend-returned i18n strings.
 * Backend sends strings in format: `i18n:key|param1=value1|param2=value2`
 * If the string doesn't start with `i18n:`, it's returned as-is.
 */
export function translateBackendString(str: string): string {
  if (!str.startsWith("i18n:")) return str;
  const content = str.slice(5);
  const pipeIdx = content.indexOf("|");
  if (pipeIdx < 0) {
    return i18n.t(content);
  }
  const key = content.slice(0, pipeIdx);
  const params: Record<string, string> = {};
  let remaining = content.slice(pipeIdx + 1);
  while (remaining.length > 0) {
    const eqIdx = remaining.indexOf("=");
    if (eqIdx < 0) break;
    const paramName = remaining.slice(0, eqIdx);
    remaining = remaining.slice(eqIdx + 1);
    const nextPipe = remaining.indexOf("|");
    if (nextPipe < 0) {
      params[paramName] = remaining;
      break;
    } else {
      params[paramName] = remaining.slice(0, nextPipe);
      remaining = remaining.slice(nextPipe + 1);
    }
  }
  return i18n.t(key, params);
}

export default i18n;