// Optional offline reference check against the exact read-only Palpo checkout.
// No installed/deployed database, homeserver, model or real credential is used.
import assert from 'node:assert/strict';
import { execFileSync, spawn } from 'node:child_process';
import { once } from 'node:events';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const root = resolve(process.argv[2] ?? 'missing-pinned-palpo-checkout');
const pin = 'c7c400e04ab05479a63c30f14679ec0180457d85';
assert.equal(execFileSync('git', ['rev-parse','HEAD'], {cwd:root,encoding:'utf8'}).trim(), pin);
execFileSync('git',['diff','--exit-code','HEAD','--','web-admin/server.mjs','web-admin/lib'],{cwd:root,stdio:'pipe'});
const load = path => import(pathToFileURL(resolve(root,path)));
const { Store } = await load('web-admin/lib/store.mjs');
const { Service, Palpo } = await load('web-admin/lib/service.mjs');
const { createApp } = await load('web-admin/server.mjs');
const fleetId = 'hf_0123456789abcdef0123456789abcdef';
const token = 'synthetic-machine-token-DO-NOT-USE';
const store = new Store(':memory:');
const palpo = new Palpo('http://127.0.0.1:1', () => { throw new Error('Homeserver IO is forbidden in this fixture'); });
const service = new Service({store,palpo,serverName:'example.test',transportOrigin:'http://127.0.0.1',relayOrigin:'http://127.0.0.1'});
const server = createApp({service,publicOrigin:'http://127.0.0.1',startAccountWorker:false});
server.listen(0,'127.0.0.1'); await once(server,'listening');
service.transportOrigin = `http://127.0.0.1:${server.address().port}`;
const fleet = store.state.fleets[fleetId] = {
  id:fleetId,state:'pending_connection',installation:'installed',transport:{mode:'outbound',token,generation:31,sequence:0},
  registration:{namespaces:{users:[]}},
};
service.outbound.transaction(fleet,'tx-reference',{events:[{event_id:'$fixture',content:{body:'opaque'}}],ephemeral:[{type:'m.typing'}],extension:{fraction:0.125}});
service.outbound.enqueue(fleet,'work','request','request-reference',{requestId:'r1',requestedTokens:100000});
store.state.requests[`${fleetId}:r1`] = {
  id:`${fleetId}:r1`,fleetId,requestId:'r1',sourceEventId:'$request',
  payload:{role:'coding',requestedTokens:100000,targetProjectId:'p1',targetRoomId:'!target:example.test',sourceRoomId:'!reception:example.test'},
};
store.save();
try {
  const child = spawn('cargo',['run','--offline','-p','hagency-palpo','--example','palpo-reference','--',`${service.transportOrigin}/api/fleet/v2/${fleetId}`],{stdio:'inherit'});
  const deadline = setTimeout(() => child.kill('SIGKILL'),120000);
  const [code] = await once(child,'exit'); clearTimeout(deadline); assert.equal(code,0);
  assert.equal(service.outbound.usage(fleet).pending,0);
  assert.equal(fleet.transport.sequence,1);
  assert.equal(store.state.requests[`${fleetId}:r1`].outboundStatus.observedAt,'2020-01-01T00:00:00.000Z');
  assert.equal(fleet.connection,undefined); assert.equal(fleet.state,'pending_connection');
  console.log(`Pinned Palpo ${pin}: actual createApp/Outbound protocol accepted, no connection proof fabricated`);
} finally {
  server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); store.close();
}
