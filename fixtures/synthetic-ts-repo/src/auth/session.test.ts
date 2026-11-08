import test from 'node:test';
import assert from 'node:assert';
import { login, logout, validate } from './session.ts';

test('a new login is valid', () => {
  const token = login('alice');
  assert.ok(validate(token));
});

test('an unknown token is not valid', () => {
  assert.equal(validate('nobody.1.0000000000000000'), false);
});

test('other users stay logged in', () => {
  const a = login('a');
  const b = login('b');
  logout(a);
  assert.ok(validate(b));
});
