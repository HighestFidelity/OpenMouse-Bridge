import { test } from 'node:test';
import assert from 'node:assert/strict';
import { loopbackHost, requestAllowed } from './request-gate.mjs';

test('only loopback hosts are served, so DNS rebinding is refused', () => {
  for (const host of ['127.0.0.1:17846', 'localhost:17846', '[::1]:17846', 'localhost']) {
    assert.ok(loopbackHost(host), host);
  }
  for (const host of [undefined, '', 'evil.example:17846', '127.0.0.1.evil.example', 'localhost:0', 'localhost:99999', '127.0.0.1:']) {
    assert.ok(!loopbackHost(host), String(host));
  }
});

test('a web page from another origin is refused, a tool with no Origin is not', () => {
  const host = '127.0.0.1:17846';
  assert.ok(requestAllowed({ host, origin: 'https://control.openmouse.app' }));
  assert.ok(requestAllowed({ host }));
  assert.ok(!requestAllowed({ host, origin: 'https://evil.example' }));
  assert.ok(!requestAllowed({ host, origin: 'null' }));
  assert.ok(!requestAllowed({ host: 'evil.example', origin: 'https://control.openmouse.app' }));
});

test('the HID WebSocket needs an allowed Origin', () => {
  const host = '127.0.0.1:17846';
  assert.ok(!requestAllowed({ host }, { requireOrigin: true }));
  assert.ok(requestAllowed({ host, origin: 'https://openmouse.app' }, { requireOrigin: true }));
});
