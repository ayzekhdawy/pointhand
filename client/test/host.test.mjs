import { test } from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { ComputerUseHost } from '../dist/host.js';
const fixture = fileURLToPath(new URL('./fixtures/host.mjs',import.meta.url));
const create = (mode='normal') => new ComputerUseHost(process.execPath,[fixture,mode]);
test('host responses are correlated and explicit errors do not kill the connection',async () => {
  const host=create();try {
    const result=await Promise.all([host.call('one',{value:1}),host.call('two',{value:2})]);
    assert.equal(result[0].params.value,1);assert.equal(result[1].params.value,2);
    await assert.rejects(host.call('fail'),/rejected input/);
    assert.equal((await host.call('still_alive')).method,'still_alive');
  } finally {await host.close();}
});
test('spawn failure rejects requests without an unhandled error',async () => {
  const host=new ComputerUseHost(fileURLToPath(new URL('./no-such-host.exe',import.meta.url)));
  await assert.rejects(host.call('ping'),/ENOENT/);await host.close();
});
test('timeout terminates the host and rejects queued and later mutations',async () => {
  const host=create('slow');
  const results=await Promise.allSettled([host.call('first',{},100),host.call('second',{},5000)]);
  assert.equal(results[0].status,'rejected');assert.equal(results[1].status,'rejected');
  assert.match(results[1].reason.message,/host terminated/);
  await assert.rejects(host.call('late'),/host terminated/);await host.close();
});
test('invalid protocol data fails promptly',async () => {
  const host=create('malformed');await assert.rejects(host.call('ping'),/invalid JSON/);await host.close();
});
test('close is idempotent and further calls are rejected',async () => {
  const host=create();await host.call('ping');await Promise.all([host.close(),host.close()]);
  await assert.rejects(host.call('late'),/closing|closed/);
});

test('stop helper failure does not corrupt the host protocol',async () => {
  const host=create();try {
    await host.call('ping');
    // Node is our mock executable and rejects the native-only --stop-pid flag.
    await assert.rejects(host.requestStop(),/stop helper failed/);
    assert.equal((await host.call('still_alive')).method,'still_alive');
  } finally {await host.close();}
});

test('emergency stop cannot spawn after closing the host',async () => {
  const host=create();await host.call('ping');await host.close();
  await assert.rejects(host.requestStop(),/closing|closed/);
});
