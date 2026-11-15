const SETUP = "mcp install --client claude-code --print --leased";
const shellWord = (text: string) => (/^[A-Za-z0-9_\/.:@%+=-]+$/.test(text) ? text : `'${text.replace(/'/g, "'\\''")}'`);

/** The setup command for this installation: a second profile or another data folder must be named, or the agent would
 *  be connected to a different Kakurega than the one this page shows. */
export function setupCommand(config?: { profile?: string | null; data_dir?: string | null } | null): string {
  if (config?.profile) return `kaku --profile ${shellWord(config.profile)} ${SETUP}`;
  if (config?.data_dir) return `kaku ${SETUP} --data-dir ${shellWord(config.data_dir)}`;
  return `kaku ${SETUP}`;
}
