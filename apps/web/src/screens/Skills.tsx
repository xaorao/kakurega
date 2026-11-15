import { Lead } from "../components/Lead";
import { t } from "../i18n";
import { Methods } from "./Methods";
import { Mcp } from "./Mcp";
import { GitHub } from "./GitHub";

/** What the assistant can be taught and what it may reach: the owner's methods and packs, and outside tool servers. */
export function Skills() {
  return (
    <section class="stack" aria-labelledby="h-skills">
      <h1 id="h-skills">{t("skills.title")}</h1>
      <Lead text={t("skills.lead")} />
      <Methods />
      <Mcp />
      <GitHub />
    </section>
  );
}
