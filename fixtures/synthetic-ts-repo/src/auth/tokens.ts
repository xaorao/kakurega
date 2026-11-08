import { createHmac } from 'node:crypto';

const SECRET: string = 'not-a-real-secret-for-tests';

export function issue(userId: string, counter: number): string {
  const body = `${userId}.${counter}`;
  const mac = createHmac('sha256', SECRET).update(body).digest('hex').slice(0, 16);
  return `${body}.${mac}`;
}

export function isAuthentic(token: unknown): boolean {
  const parts = String(token).split('.');
  if (parts.length !== 3) return false;
  const expected = issue(parts[0], Number(parts[1]));
  return expected === token;
}
