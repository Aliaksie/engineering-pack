import test from 'node:test';
import assert from 'node:assert/strict';
import { rpcProcess } from '../platform/index.mjs';

const program = `
let buffer='';process.stdin.setEncoding('utf8');process.stdin.on('data',chunk=>{buffer+=chunk;let end;
while((end=buffer.indexOf('\\n'))>=0){const m=JSON.parse(buffer.slice(0,end));buffer=buffer.slice(end+1);
const send=v=>process.stdout.write(JSON.stringify(v)+'\\n');
if(m.method==='pid')send({id:m.id,result:process.pid});
if(m.method==='echo'){send({method:'fixture/event',params:{observed:true}});send({id:m.id,result:m.params});}
if(m.method==='approval'){send({id:'server-request',method:'item/commandExecution/requestApproval',params:{}});globalThis.waiting=m.id;}
if(m.id==='server-request'){send({id:globalThis.waiting,result:m});}
if(m.method==='malformed')process.stdout.write('invalid JSON\\n');
if(m.method==='oversized')process.stdout.write('x'.repeat(1024*1024+1));
if(m.method==='reject')send({id:m.id,error:{code:-32001,message:'not echoed'}});
}});`;
const fixture = (t, options = {}) => {
  const client = rpcProcess({ command: process.execPath, args: ['-e', program], timeoutMs: 3000, ...options });
  t.after(() => client.close()); return client;
};
test('stdio RPC matches responses, delivers notifications and rejects unsupported native approval requests', async t => {
  const client = fixture(t), messages = [];
  client.subscribe(message => messages.push(message));
  const result = await client.request('echo', { key: 'bounded' });
  assert.deepEqual(result, { key: 'bounded' }); assert.equal(messages[0].method, 'fixture/event');
  const approval = await client.request('approval');
  assert.equal(approval.error.code, -32601); assert.equal(Object.hasOwn(approval, 'result'), false);
  await assert.rejects(client.request('reject'), error => error.code === 'RPC_REJECTED' && error.nativeCode === -32001);
});
test('stdio native request handling is explicit and request timeouts never retry effects', async t => {
  const handled = [], client = fixture(t, { onRequest: async (method, params) => { handled.push({ method, params }); return { decision: 'decline' }; } });
  assert.deepEqual((await client.request('approval')).result, { decision: 'decline' });
  assert.equal(handled.length, 1);
  const timed = fixture(t, { timeoutMs: 100 });
  await assert.rejects(timed.request('no-response'), error => error.code === 'RPC_TIMEOUT');
  assert.equal((await timed.request('echo', { stillOpen: true })).stillOpen, true);
});
test('malformed or oversized native streams close the connection and reject all pending requests', async t => {
  for (const method of ['malformed', 'oversized']) {
    const client = fixture(t), other = client.request('no-response');
    const results = await Promise.allSettled([other, client.request(method)]);
    assert.ok(results.every(result => result.status === 'rejected' && result.reason.code === 'RPC_CLOSED'));
    await assert.rejects(client.request('echo'), error => error.code === 'RPC_CLOSED');
  }
});


test('owned RPC shutdown resolves only after exit, rejects pending work and is repeatable', async t => {
  const client = fixture(t);
  const pid = await client.request('pid');
  const pending = assert.rejects(client.request('no-response'), error => error.code === 'RPC_CLOSED');
  const closing = client.close();
  assert.equal(client.close(), closing);
  await closing; await pending;
  assert.throws(() => process.kill(pid, 0), error => error.code === 'ESRCH');
  await client.close();
});


test('shutdown deadline reports an unverified exit instead of claiming successful closure', { skip: process.platform === 'win32' }, async () => {
  // Windows terminates on kill(); POSIX processes can deliberately ignore SIGTERM.
  const client = rpcProcess({ command: process.execPath, args: ['-e', `process.on('SIGTERM', () => {});setInterval(() => {}, 1000);${program}`], shutdownTimeoutMs: 50 });
  await client.request('pid');
  await assert.rejects(client.close(), error => error.code === 'RPC_SHUTDOWN_TIMEOUT');
});
