import { spawn } from 'node:child_process';

// Bounded newline-delimited JSON RPC. Protocol interpretation belongs to the host.
export function rpcProcess({ command, args = [], cwd, env, timeoutMs = 30000, shutdownTimeoutMs = 5000, onRequest, jsonrpc = false } = {}) {
  if (typeof command !== 'string' || !command || !Array.isArray(args) || args.some(arg => typeof arg !== 'string')
    || !Number.isSafeInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > 300000 || !Number.isSafeInteger(shutdownTimeoutMs) || shutdownTimeoutMs < 1 || shutdownTimeoutMs > 30000) throw new Error('invalid RPC process options');
  const child = spawn(command, args, { cwd, env, shell: false, stdio: ['pipe', 'pipe', 'pipe'] });
  let shutdown;
  const terminated = new Promise(resolve => child.once('close', resolve));
  const pending = new Map(), listeners = new Set();
  let sequence = 0, buffer = '', closed = false, diagnostic = '', serverRequests = 0;
  const failure = () => Object.assign(new Error('Native RPC connection closed; effect outcome may be unknown'), { code: 'RPC_CLOSED' });
  const fail = () => {
    if (closed) return;
    closed = true;
    for (const entry of pending.values()) { clearTimeout(entry.timer); entry.reject(failure()); }
    pending.clear();
    for (const listener of listeners) { try { listener({ method: '$connection/closed', params: {} }); } catch { /* Other observers must still be notified. */ } }
  };
  const send = value => {
    if (closed || !child.stdin.writable) throw failure();
    const line = `${JSON.stringify(jsonrpc ? { jsonrpc: '2.0', ...value } : value)}\n`;
    if (Buffer.byteLength(line) > 1024 * 1024 || child.stdin.writableLength > 2 * 1024 * 1024) throw new Error('RPC output exceeds bound');
    child.stdin.write(line);
  };
  const message = value => {
    if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('invalid RPC envelope');
    if (jsonrpc && value.jsonrpc !== '2.0') throw new Error('invalid JSON-RPC version');
    if (typeof value.method === 'string') {
      if (Object.hasOwn(value, 'id')) {
        if (++serverRequests > 128) throw new Error('native request capacity reached');
        // Unknown server requests fail closed. An integration must explicitly handle approvals.
        Promise.resolve().then(() => {
          if (!onRequest) throw new Error('Unsupported native request');
          return onRequest(value.method, value.params);
        }).then(result => send({ id: value.id, result }), () => send({ id: value.id, error: { code: -32601, message: 'Native request was not authorized or supported' } }))
          .catch(fail).finally(() => { serverRequests--; });
      } else for (const listener of listeners) { try { listener(value); } catch { /* Host observers own their errors. */ } }
    } else {
      const entry = pending.get(value.id);
      if (!entry) return;
      pending.delete(value.id); clearTimeout(entry.timer);
      if (value.error) entry.reject(Object.assign(new Error('Native RPC request rejected'), { code: 'RPC_REJECTED', nativeCode: value.error.code }));
      else if (Object.hasOwn(value, 'result')) entry.resolve(value.result);
      else entry.reject(new Error('invalid RPC response'));
    }
  };
  child.stdout.setEncoding('utf8');
  child.stdout.on('data', chunk => {
    buffer += chunk;
    try {
      let end;
      while ((end = buffer.indexOf('\n')) >= 0) {
        const line = buffer.slice(0, end); buffer = buffer.slice(end + 1);
        if (Buffer.byteLength(line) > 1024 * 1024) throw new Error('RPC input exceeds bound');
        if (line.trim()) message(JSON.parse(line));
      }
      if (Buffer.byteLength(buffer) > 1024 * 1024) throw new Error('RPC input exceeds bound');
    } catch { fail(); child.kill(); }
  });
  child.stderr.setEncoding('utf8');
  child.stderr.on('data', chunk => { diagnostic = (diagnostic + chunk).slice(-4096); });
  child.on('error', fail); child.on('exit', fail); child.stdout.on('end', fail); child.stdin.on('error', fail);
  return {
    request(method, params = {}) {
      if (closed) return Promise.reject(failure());
      if (pending.size >= 128) return Promise.reject(new Error('RPC pending request capacity reached'));
      return new Promise((resolve, reject) => {
        const id = ++sequence;
        const timer = setTimeout(() => {
          pending.delete(id);
          reject(Object.assign(new Error('Native RPC deadline exceeded; do not retry an effect'), { code: 'RPC_TIMEOUT' }));
        }, timeoutMs);
        pending.set(id, { resolve, reject, timer });
        try { send({ id, method, params }); } catch (error) { clearTimeout(timer); pending.delete(id); reject(error); }
      });
    },
    notify: (method, params = {}) => send({ method, params }),
    subscribe(listener) { listeners.add(listener); return () => listeners.delete(listener); },
    close() {
      if (shutdown) return shutdown;
      fail(); child.stdin.destroy(); child.kill();
      shutdown = new Promise((resolve, reject) => {
        const deadline = setTimeout(() => {
          child.kill('SIGKILL');
          reject(Object.assign(new Error('Native RPC shutdown deadline exceeded; process closure is unverified'), { code: 'RPC_SHUTDOWN_TIMEOUT' }));
        }, shutdownTimeoutMs);
        terminated.then(() => { clearTimeout(deadline); resolve(); });
      });
      return shutdown;
    },
    // Explicit local diagnostics only; never forwarded into telemetry or task records.
    diagnostic: () => diagnostic
  };
}
