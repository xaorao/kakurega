import { createSignal } from "solid-js";

export type Theme = "system" | "light" | "dark";

export function applyTheme(t: Theme) {
  if (t === "system") document.documentElement.removeAttribute("data-theme");
  else document.documentElement.setAttribute("data-theme", t);
}

function storedTheme(): Theme {
  try {
    const v = localStorage.getItem("kk-theme");
    return v === "light" || v === "dark" ? v : "system";
  } catch {
    return "system";
  }
}

const [theme, setTheme] = createSignal<Theme>(storedTheme());
export { theme };

export function chooseTheme(t: Theme) {
  setTheme(t);
  applyTheme(t);
  try {
    if (t === "system") localStorage.removeItem("kk-theme");
    else localStorage.setItem("kk-theme", t);
  } catch {
    /* the choice still applies for this visit */
  }
}

