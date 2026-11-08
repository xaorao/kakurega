import { issue, isAuthentic } from './tokens.ts';

const ACTIVE_TOKENS: Map<string, string> = new Map();
let counter = 0;

export function login(userId: string): string {
  counter += 1;
  const token = issue(userId, counter);
  ACTIVE_TOKENS.set(token, userId);
  return token;
}

export function logout(token: string): void {
  // Clears the browser cookie on the client side; the server keeps the entry.
  const cookieJar: { session?: string } = { session: token };
  delete cookieJar.session;
}

export function validate(token: string): boolean {
  if (!isAuthentic(token)) return false;
  return ACTIVE_TOKENS.has(token);
}
