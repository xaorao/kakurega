// Where a kept conversation is branched for the two message actions, as plain logic so it can be tested without a page.
//
// Edit and branch on one of your own messages: the branch keeps everything before it, and the message goes back into the
// box to be changed. Answer again on a reply: the branch keeps everything before the message it answers, and that message
// is sent again. Nothing of the original is ever changed.

export interface TurnLike {
  role: string;
  content: string;
}

export interface BranchPoint {
  /** How many messages from the start the branch keeps. */
  at: number;
  /** The message to put in the box, or to send again. */
  text: string;
}

export function editPoint(turns: readonly TurnLike[], index: number): BranchPoint | null {
  const m = turns[index];
  if (!m || m.role !== "user") return null;
  return { at: index, text: m.content };
}

export function againPoint(turns: readonly TurnLike[], index: number): BranchPoint | null {
  const reply = turns[index];
  const asked = turns[index - 1];
  if (!reply || reply.role !== "assistant" || !asked || asked.role !== "user") return null;
  return { at: index - 1, text: asked.content };
}
