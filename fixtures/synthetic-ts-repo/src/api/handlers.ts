import { login, logout, validate } from '../auth/session.ts';
import { retryDelay } from '../util/backoff.ts';

type Body = { user?: unknown; token?: string } | null;

export function handleLogin(body: Body): { status: number; token?: string } {
  if (!body || typeof body.user !== 'string') return { status: 400 };
  return { status: 200, token: login(body.user) };
}

export function handleLogout(body: { token: string }): { status: number } {
  logout(body.token);
  return { status: 204 };
}

export function handleOrders(token: string): { status: number; orders?: unknown[]; retryAfterMs?: number } {
  if (!validate(token)) return { status: 401 };
  return { status: 200, orders: [], retryAfterMs: retryDelay(1) };
}
