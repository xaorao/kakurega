// Whether the last message the model received is word for word what was typed.

export function sameWords(messages: readonly { role: string; content: string }[], typed: string): boolean {
  const last = messages[messages.length - 1];
  return last !== undefined && last.role === "user" && last.content === typed;
}
