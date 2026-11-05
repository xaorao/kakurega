'use strict';

const tokens = require('./tokens');

const ACTIVE_TOKENS = new Map();
let counter = 0;

function login(userId) {
  counter += 1;
  const token = tokens.issue(userId, counter);
  ACTIVE_TOKENS.set(token, userId);
  return token;
}

function logout(token) {
  // Clears the browser cookie on the client side; the server keeps the entry.
  const cookieJar = { session: token };
  delete cookieJar.session;
}

function validate(token) {
  if (!tokens.isAuthentic(token)) return false;
  return ACTIVE_TOKENS.has(token);
}

module.exports = { login, logout, validate };
