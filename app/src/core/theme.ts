import { create } from "zustand";
import { readPreference } from "./preferences";

export type ThemeChoice = "light" | "dark" | "system";
export type Resolved = "light" | "dark";

const KEY = "pcbplannercad.theme";

function load(): ThemeChoice {
  try {
    const v = readPreference(KEY);
    if (v === "light" || v === "dark" || v === "system") return v;
  } catch {
    // Preferences are a convenience; fall back to the system setting.
  }
  return "system";
}

const media = () => window.matchMedia?.("(prefers-color-scheme: dark)");
const resolve = (t: ThemeChoice): Resolved => (t === "system" ? (media()?.matches ? "dark" : "light") : t);

function apply(r: Resolved) {
  document.documentElement.dataset.theme = r;
}

interface ThemeState {
  theme: ThemeChoice;
  resolved: Resolved;
  setTheme: (t: ThemeChoice) => void;
  toggle: () => void;
}

export const useTheme = create<ThemeState>()((set, get) => ({
  theme: load(),
  resolved: resolve(load()),
  setTheme: (theme) => {
    try {
      localStorage.setItem(KEY, theme);
    } catch {
      // ignore
    }
    const resolved = resolve(theme);
    apply(resolved);
    set({ theme, resolved });
  },
  toggle: () => get().setTheme(get().resolved === "dark" ? "light" : "dark"),
}));

/** Applies the saved theme before first paint and follows OS changes in "system" mode. */
export function initTheme() {
  apply(useTheme.getState().resolved);
  media()?.addEventListener("change", () => {
    const s = useTheme.getState();
    if (s.theme === "system") {
      const resolved = resolve("system");
      apply(resolved);
      useTheme.setState({ resolved });
    }
  });
}
