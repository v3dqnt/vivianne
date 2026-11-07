import { test } from 'node:test'
import assert from 'node:assert/strict'
import { hasSecrets, maskSecrets } from './secretMask.ts'

test('keys in common formats are hidden but still tell you which kind they were', () => {
  const ant = 'sk-ant-api03-AbCdEfGhIjKlMnOpQrStUvWxYz0123456789'
  assert.equal(maskSecrets(`use ${ant} please`), 'use sk-ant-••••••••  please'.replace('  ', ' '))
  assert.match(maskSecrets('token ghp_abcdefghijklmnopqrstuvwxyz0123456789'), /^token ghp_••••••••$/)
  assert.match(maskSecrets('AKIAIOSFODNN7EXAMPLE'), /^AKIA••••••••$/)
  assert.match(maskSecrets('Authorization: Bearer abcdefghijklmnopqrstuvwxyz123456'), /^Authorization: Bearer ••••••••$/)
})

test('assignments hide the value and keep the name', () => {
  assert.equal(maskSecrets('DB_PASSWORD=hunter2hunter2'), 'DB_PASSWORD=••••••••')
  assert.equal(maskSecrets('{"api_key": "abcd1234efgh5678"}'), '{"api_key": "••••••••"}')
})

test('private key blocks are hidden whole', () => {
  const pem = '-----BEGIN RSA PRIVATE KEY-----\nMIIEow\nabc\n-----END RSA PRIVATE KEY-----'
  assert.equal(maskSecrets(`here:\n${pem}\nthanks`), 'here:\n••••••••\nthanks')
})

test('ordinary text is untouched', () => {
  const plain = 'Fix the token counter in src/tokens.ts, the password field is fine, key=short'
  assert.equal(maskSecrets(plain), plain)
  assert.equal(hasSecrets(plain), false)
  assert.equal(hasSecrets('sk-ant-api03-AbCdEfGhIjKlMnOpQrStUvWxYz0123456789'), true)
})
