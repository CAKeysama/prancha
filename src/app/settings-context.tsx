import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { DEFAULT_SETTINGS, resolveTheme, sanitizeSettings, type SettingKey, type Settings } from "../domain/settings";
import { api, errorMessage } from "../services/api";
import { sound } from "../services/sound";

interface SettingsApi {
  settings: Settings;
  loaded: boolean;
  loadError: string | null;
  update: <K extends SettingKey>(key: K, value: Settings[K]) => Promise<void>;
  /** A pessoa já escolheu este valor alguma vez (está salvo), em vez de usar o padrão. */
  isSaved: (key: SettingKey) => boolean;
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

function prefersReducedMotion(): boolean {
  return typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/** Guarda a preferência para o `theme-boot.js` aplicar antes da primeira pintura. */
function remember(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* armazenamento indisponível */
  }
}

export function SettingsProvider({ children }: { children: ReactNode }) {
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [loaded, setLoaded] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [systemDark, setSystemDark] = useState(systemPrefersDark);
  const [saved, setSaved] = useState<ReadonlySet<string>>(() => new Set());
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
        setSaved(new Set(Object.keys(raw)));
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
  // Depois do primeiro desenho, a troca vira um esmaecimento curto da janela inteira, sem nada pulando de cor em tempos diferentes.
  // Antes de carregar, vale o que o `theme-boot.js` aplicou: os padrões não podem piscar por cima.
  const firstTheme = useRef(true);
  useEffect(() => {
    if (!loaded) return;
    const root = document.documentElement;
    const apply = () => {
      root.dataset.theme = effectiveTheme;
    };
    const animate = !firstTheme.current && root.dataset.theme !== effectiveTheme && typeof document.startViewTransition === "function" && !prefersReducedMotion();
    firstTheme.current = false;
    if (animate) document.startViewTransition(apply);
    else apply();
    remember("prancha.theme", pref);
    try {
      void getCurrentWindow()
        .setTheme(pref === "system" ? null : pref)
        .catch(() => undefined);
    } catch {
      /* fora do Tauri */
    }
  }, [loaded, effectiveTheme, pref]);

  const font = settings["appearance.font"];
  useEffect(() => {
    if (!loaded) return;
    document.documentElement.dataset.font = font;
    remember("prancha.font", font);
  }, [loaded, font]);

  // O serviço de som só toca o que as preferências deixam.
  const soundOn = settings["sound.enabled"];
  const volume = settings["sound.volume"];
  useEffect(() => {
    sound.configure({ enabled: loaded && soundOn, volume: volume / 100 });
  }, [loaded, soundOn, volume]);

  const update = useCallback(async <K extends SettingKey>(key: K, value: Settings[K]) => {
    const previous = latest.current[key];
    setSettings((s) => ({ ...s, [key]: value }));
    setSaved((s) => (s.has(key) ? s : new Set(s).add(key)));
    try {
      await api.setSetting(key, value);
    } catch (e) {
      setSettings((s) => ({ ...s, [key]: previous }));
      throw e;
    }
  }, []);

  const isSaved = useCallback((key: SettingKey) => saved.has(key), [saved]);
  const value = useMemo(
    () => ({ settings, loaded, loadError, update, isSaved, effectiveTheme }),
    [settings, loaded, loadError, update, isSaved, effectiveTheme],
  );
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}
