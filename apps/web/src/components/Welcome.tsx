import { createResource, createSignal, For, Show } from "solid-js";
import { api } from "../api";
import { LOCALES, locale, setLocale, t, type Locale } from "../i18n";
import { modelKey } from "../privacy";
import { chooseTheme, theme, type Theme } from "../theme";
import { markWelcomeDone, nextStep, setupSummary, STEPS, type Step } from "../welcome";

/** The first visit. Four steps, each of which can be skipped, and all of which can be skipped at once. Nothing here is required. */
export function Welcome(props: { onDone: () => void }) {
  const [step, setStep] = createSignal<Step>("language");
  const [ready] = createResource(() => api.readiness().catch(() => null));
  const [facts] = createResource(() => api.privacy().catch(() => null));
  let heading: HTMLHeadingElement | undefined;
  const finish = () => {
    markWelcomeDone(typeof localStorage === "undefined" ? undefined : localStorage);
    props.onDone();
  };
  const advance = () => {
    const n = nextStep(step());
    if (n === null) {
      finish();
      location.hash = "#/";
      // the first review starts on the page under the card: the owner is taken to it, with the sample offered first
      queueMicrotask(() => {
        const to = document.querySelector<HTMLElement>("#sample button, #h-new");
        to?.scrollIntoView({ block: "start" });
        (document.querySelector<HTMLElement>("#sample button") ?? document.getElementById("h-new"))?.focus();
      });
      return;
    }
    setStep(n);
    queueMicrotask(() => heading?.focus());
  };
  const summary = () => setupSummary(ready()?.checks);
  return (
    <section class="card stack welcome" role="region" aria-labelledby="h-welcome" onKeyDown={(e) => { if (e.key === "Escape") finish(); }}>
      <p class="muted">{t("welcome.counter", { n: STEPS.indexOf(step()) + 1, of: STEPS.length })}</p>
      <h2 id="h-welcome" tabindex="-1" ref={heading}>{t(`welcome.${step()}.title` as never)}</h2>
      <Show when={step() === "language"}>
        <fieldset class="stack" aria-labelledby="h-welcome">
          <For each={LOCALES}>
            {([code, name]) => (
              <label class="choice"><input type="radio" name="welcome-lang" checked={locale() === code} onChange={() => setLocale(code as Locale)} /> {name}</label>
            )}
          </For>
        </fieldset>
      </Show>
      <Show when={step() === "theme"}>
        <fieldset class="stack" aria-labelledby="h-welcome">
          <For each={["system", "light", "dark"] as Theme[]}>
            {(v) => (
              <label class="choice"><input type="radio" name="welcome-theme" checked={theme() === v} onChange={() => chooseTheme(v)} /> {t(`theme.${v}` as never)}</label>
            )}
          </For>
        </fieldset>
      </Show>
      <Show when={step() === "privacy"}>
        <Show when={facts()} fallback={<p role="status">{t("welcome.privacy.unknown")}</p>}>
          {(f) => (
            <>
              <p role="status">{t(modelKey(f().model), { name: f().model.name ?? f().model.id, host: f().model.host ?? "" })}</p>
              <p class="muted">{t("privacy.model.sent")}</p>
              <p class="muted">{t("welcome.privacy.more")}</p>
            </>
          )}
        </Show>
      </Show>
      <Show when={step() === "setup"}>
        <p role="status">
          <Show when={summary()} fallback={t("welcome.setup.unknown")}>
            {(s) => (s().problems === 0 ? t("welcome.setup.ok") : t("welcome.setup.problems", { n: s().problems }))}
          </Show>
        </p>
        <p class="muted">{t("welcome.setup.note")}</p>
      </Show>
      <div class="actions">
        <button class="primary" type="button" onClick={advance}>{step() === "setup" ? t("welcome.start") : t("welcome.next")}</button>
        <Show when={step() !== "setup"}><button type="button" onClick={advance}>{t("welcome.skip")}</button></Show>
        <button type="button" class="link" onClick={finish}>{t("welcome.skipAll")}</button>
      </div>
    </section>
  );
}
