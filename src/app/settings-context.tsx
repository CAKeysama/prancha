import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { DEFAULT_SETTINGS, resolveTheme, sanitizeSettings, type SettingKey, type Settings } from "../domain/settings";
import { api, errorMessage } from "../services/api";

interface SettingsApi {
  settings: Settings;
  loaded: boolean;
  loadError: string | null;
  update: <K extends SettingKey>(key: K, value: Settings[K]) => Promise<void>;
  effectiveTheme: "dark" | "light";
}

const Ctx = createContext<SettingsApi | null>(null);

export function useSettings(): SettingsApi {
  const v = useContext(Ctx);
  if (!v) throw new Error("useSettings fora do SettingsProvider");
  return v;
}

function systemPrefersDark(): boolean {
  return typeof window.matchMedia === "function" ? window.matchMedia("(prefers-color-scheme: dark)").matches : true;
}

export function SettingsProvider({ children }: { children: ReactNode }) {
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [loaded, setLoaded] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [systemDark, setSystemDark] = useState(systemPrefersDark);
  const latest = useRef(settings);

  useEffect(() => {
    latest.current = settings;
  }, [settings]);

  useEffect(() => {
    let alive = true;
    api
      .getSettings()
      .then((raw) => {
        if (!alive) return;
        setSettings(sanitizeSettings(raw));
        setLoaded(true);
      })
      .catch((e) => {
        if (!alive) return;
        setLoadError(errorMessage(e));
        setLoaded(true);
      });
    return () => {
      alive = false;
    };
  }, []);

  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => setSystemDark(mq.matches);
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, []);

  const pref = settings["appearance.theme"];
  const effectiveTheme = resolveTheme(pref, systemDark);

  // Aplica o tema imediatamente e espelha a preferência para o próximo início (sem flash).
  useEffect(() => {
    document.documentElement.dataset.theme = effectiveTheme;
    try {
      localStorage.setItem("prancha.theme", pref);
    } catch {
      /* armazenamento indisponível */
    }
    try {
      void getCurrentWindow()
        .setTheme(pref === "system" ? null : pref)
        .catch(() => undefined);
    } catch {
      /* fora do Tauri */
    }
  }, [effectiveTheme, pref]);

  const update = useCallback(async <K extends SettingKey>(key: K, value: Settings[K]) => {
    const previous = latest.current[key];
    setSettings((s) => ({ ...s, [key]: value }));
    try {
      await api.setSetting(key, value);
    } catch (e) {
      setSettings((s) => ({ ...s, [key]: previous }));
      throw e;
    }
  }, []);

  const value = useMemo(
    () => ({ settings, loaded, loadError, update, effectiveTheme }),
    [settings, loaded, loadError, update, effectiveTheme],
  );
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}
