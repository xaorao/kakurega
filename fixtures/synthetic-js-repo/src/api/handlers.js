'use strict';

const session = require('../auth/session');
const { retryDelay } = require('../util/backoff');

function handleLogin(body) {
  if (!body || typeof body.user !== 'string') return { status: 400 };
  return { status: 200, token: session.login(body.user) };
}

function handleLogout(body) {
  session.logout(body.token);
  return { status: 204 };
}

function handleOrders(token) {
  if (!session.validate(token)) return { status: 401 };
  return { status: 200, orders: [], retryAfterMs: retryDelay(1) };
}

module.exports = { handleLogin, handleLogout, handleOrders };
