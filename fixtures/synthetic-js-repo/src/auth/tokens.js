'use strict';

const crypto = require('node:crypto');

const SECRET = 'not-a-real-secret-for-tests';

function issue(userId, counter) {
  const body = `${userId}.${counter}`;
  const mac = crypto.createHmac('sha256', SECRET).update(body).digest('hex').slice(0, 16);
  return `${body}.${mac}`;
}

function isAuthentic(token) {
  const parts = String(token).split('.');
  if (parts.length !== 3) return false;
  const expected = issue(parts[0], parts[1]);
  return expected === token;
}

module.exports = { issue, isAuthentic };
