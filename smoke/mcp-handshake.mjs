#!/usr/bin/env node
// cadcraft MCP 握手冒烟：spawn `cadcraft-cli mcp`，走 newline-delimited JSON-RPC，
// 断言 initialize / tools/list / 一次 render 调用。用途：接进 DSH 之前先证明这层能说上话。
// 用法：node smoke\mcp-handshake.mjs [exe路径]
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, '..');
const exe = process.argv[2] || path.join(root, 'bin', 'cadcraft-cli.exe');

const child = spawn(exe, ['mcp'], { cwd: root, stdio: ['pipe', 'pipe', 'pipe'] });
let buf = '';
const pending = new Map();
let nextId = 1;

child.stdout.on('data', (d) => {
  buf += d.toString('utf8');
  let i;
  while ((i = buf.indexOf('\n')) >= 0) {
    const line = buf.slice(0, i).trim();
    buf = buf.slice(i + 1);
    if (!line) continue;
    let msg;
    try { msg = JSON.parse(line); } catch { console.log('非 JSON 行（忽略）：' + line.slice(0, 120)); continue; }
    if (msg.id !== undefined && pending.has(msg.id)) {
      const { resolve, reject } = pending.get(msg.id);
      pending.delete(msg.id);
      msg.error ? reject(new Error(JSON.stringify(msg.error))) : resolve(msg.result);
    }
  }
});
child.stderr.on('data', (d) => console.log('[stderr] ' + d.toString('utf8').trim()));

function rpc(method, params, notify = false) {
  const id = notify ? undefined : nextId++;
  const payload = { jsonrpc: '2.0', method };
  if (params !== undefined) payload.params = params;
  if (!notify) payload.id = id;
  child.stdin.write(JSON.stringify(payload) + '\n');
  if (notify) return Promise.resolve();
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject });
    setTimeout(() => { if (pending.has(id)) { pending.delete(id); reject(new Error(method + ' 超时')); } }, 60000);
  });
}

const t0 = Date.now();
try {
  const init = await rpc('initialize', {
    protocolVersion: '2024-11-05',
    capabilities: {},
    clientInfo: { name: 'dsh-smoke', version: '0.0.1' },
  });
  console.log('✓ initialize：server=' + JSON.stringify(init.serverInfo || {}) + ' protocol=' + init.protocolVersion);
  await rpc('notifications/initialized', undefined, true);

  const tools = await rpc('tools/list', {});
  const names = (tools.tools || []).map(t => t.name);
  console.log('✓ tools/list：' + names.length + ' 个工具');
  console.log('  ' + names.join(' · '));

  const draw = await rpc('tools/call', { name: 'command_line', arguments: { text: 'rectang 0,0 100,60' } });
  const drawText = JSON.stringify(draw).slice(0, 200);
  console.log('✓ command_line rectang：' + drawText);

  const render = await rpc('tools/call', { name: 'render', arguments: { width: 900, height: 600 } });
  const content = (render.content || []).find(c => c.type === 'image');
  if (!content) throw new Error('render 没返回 image 内容：' + JSON.stringify(render).slice(0, 300));
  const out = path.join(root, 'out', 'mcp-render.png');
  fs.mkdirSync(path.dirname(out), { recursive: true });
  fs.writeFileSync(out, Buffer.from(content.data, 'base64'));
  console.log('✓ render：PNG ' + fs.statSync(out).size + ' B → ' + out + '（' + ((Date.now() - t0) / 1000).toFixed(1) + 's）');
  console.log('MCP-HANDSHAKE OK');
} catch (e) {
  console.log('✗ 失败：' + e.message);
  process.exitCode = 1;
} finally {
  child.stdin.end();
  setTimeout(() => child.kill(), 300);
}
