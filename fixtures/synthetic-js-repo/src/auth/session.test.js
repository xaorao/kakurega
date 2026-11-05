'use strict';

const test = require('node:test');
const assert = require('node:assert');
const session = require('./session');

test('a new login is valid', () => {
  const token = session.login('alice');
  assert.ok(session.validate(token));
});

test('an unknown token is not valid', () => {
  assert.equal(session.validate('nobody.1.0000000000000000'), false);
});

test('other users stay logged in', () => {
  const a = session.login('a');
  const b = session.login('b');
  session.logout(a);
  assert.ok(session.validate(b));
});
