import { t } from "../i18n";
import { Methods } from "./Methods";
import { Mcp } from "./Mcp";

/** What the assistant can be taught and what it may reach: the owner's methods and packs, and outside tool servers. */
export function Skills() {
  return (
    <section class="stack wide" aria-labelledby="h-skills">
      <h1 id="h-skills">{t("skills.title")}</h1>
      <p class="lead">{t("skills.lead")}</p>
      <Methods />
      <Mcp />
    </section>
  );
}
