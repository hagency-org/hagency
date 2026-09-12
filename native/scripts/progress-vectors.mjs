import { readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import path from 'node:path';
import { parseProgressFilter, decideProgressEvent, verbFor, acpUpdateToEvent, acpUpdateFailedCall, buildProgressSummary } from '../../lib/progress-filter.js';
const sources = ['lib/progress-filter.js', 'bin/hagency-progress', 'scripts/hagency-acp-agent.mjs'];
const text = (file) => readFileSync(new URL(`../../${file}`, import.meta.url), 'utf8').replaceAll('\r\n', '\n');
const hashes = Object.fromEntries(sources.map(file => [file, createHash('sha256').update(text(file)).digest('hex')]));
const vectors = [];
const add = (operation, input, expected) => vectors.push({ operation, input, expected });
const rules = [null, {}, {events:[]}, {events:[' start ','done']}, {events:['unknown']}, {events:null}, {tools:{include:[]}}, {tools:{include:['Read','Bash'],exclude:['Bash']}}, {tools:{exclude:null}}, {minIntervalMs:0}, {minIntervalMs:1.5}, {minIntervalMs:5000.5}, {minIntervalMs:120000}, {events:'done'}, {events:['']}, {events:[3]}, {tools:[]}, {tools:null}, {tools:{include:'Read'}}, {tools:{exclude:'Bash'}}, {perGroup:[]}, {perGroup:null}, {minIntervalMs:-1}, {minIntervalMs:'60s'}, [], 'bad', {tools:{exclude:['Bash']},perGroup:{customer:{events:['step']}}}, {events:['done'],perGroup:{customer:{events:['start'],minIntervalMs:10000}}}];
for (const raw of rules) for (const group of [null, 'customer']) add('filter', {raw,group}, parseProgressFilter(raw,{group}));
for (const raw of [null, {events:['done']}, {tools:{exclude:['Bash']}}, {tools:{include:['Read']}}, {events:[]}]) {
  const filter = parseProgressFilter(raw).filter;
  for (const [event,tool] of [['start',null],['SessionStart',null],['Stop',null],['done',null],['PostToolUse','Read'],['PostToolUse','Bash'],['future','PrivateCustomerTool'],['PostToolUse',null],['PostToolUse',42]]) {
    const result = decideProgressEvent({event,tool,filter});
    add('decision', {raw,event,tool}, {report:result.report,kind:result.kind,verb:result.verb ?? null});
  }
}
for (const tool of ['Read','Glob','Grep','Bash','Edit','Write','NotebookEdit','WebFetch','WebSearch','PrivateTool']) add('verb',tool,verbFor(tool));
for (const sessionUpdate of ['tool_call','tool_call_update','agent_message_chunk','plan']) for (const kind of ['read','search','execute','edit','delete','move','fetch','think','other','future']) {
  const input={sessionUpdate,kind,status:'failed',toolCallId:'call-1',title:'SECRET /private/customer/.env',rawInput:{token:'SECRET'},error:'SECRET'};
  add('acp',input,{mapped:acpUpdateToEvent(input),failed:acpUpdateFailedCall(input)});
}
for (const status of ['failed','FAILED','Failed','completed','pending','in_progress',' failed ',null,42]) {
  const input={sessionUpdate:'tool_call_update',status}; add('acp',input,{mapped:acpUpdateToEvent(input),failed:acpUpdateFailedCall(input)});
}
for (const kind of ['start','step','done']) for (const pairs of [[],[['read',1]],[['worked',16]],[['wrote',1],['read',3]]]) for (const failures of [0,1,3]) for (const delivered of [null,0,2]) {
  add('summary',{kind,pairs,failures,delivered},buildProgressSummary({kind,counts:Object.fromEntries(pairs),failures,delivered}));
}

// Execute the actual CLI body with explicit fake IO, clock, environment and
// fetch. No VM source snippet reimplements coalescing, and no real hook runs.
let executable=text('bin/hagency-progress');
for (const line of ["import { readFileSync, writeFileSync, mkdirSync } from 'fs';", "import path from 'path';", "import os from 'os';", "import { buildProgressSummary, decideProgressEvent, parseProgressFilter } from '../lib/progress-filter.js';"]) {
  if (!executable.includes(line)) throw new Error('Review changed progress CLI imports');
  executable=executable.replace(line,'');
}
executable=executable.replace(/^#![^\n]*\n/,'');
const AsyncFunction=Object.getPrototypeOf(async function(){}).constructor;
const execute=new AsyncFunction('readFileSync','writeFileSync','mkdirSync','path','os','buildProgressSummary','decideProgressEvent','parseProgressFilter','process','Date','fetch','AbortSignal',executable);
async function hookSeries(steps) {
  let state={lastSentAt:0,counts:{}};
  const results=[];
  for (const step of steps) {
    let emitted=null;
    const stop={};
    const payload={hook_event_name:step.event,tool_name:step.tool};
    const read=(file)=>file===0 ? JSON.stringify(payload) : String(file).includes('progress-anchor') ? JSON.stringify({replyTo:'input-1',group:'fixture'}) : String(file).endsWith('progress-filter.json') ? 'null' : JSON.stringify(state);
    const process={env:{HAGENCY_AGENT:'fixture',AGENT_TOKEN:'fixture-token'},argv:[],exit(){throw stop;}};
    try { await execute(read,(_file,data)=>{state=JSON.parse(data);},()=>{},path,{homedir:()=>'/fixture/home',tmpdir:()=>'/fixture/tmp'},buildProgressSummary,decideProgressEvent,parseProgressFilter,process,{now:()=>step.now},async(_url,options)=>{emitted=JSON.parse(options.body).summary;return {ok:step.accepted,status:step.accepted?200:503};},{timeout:()=>null}); } catch(error) { if(error!==stop) throw error; }
    results.push({emitted,state:structuredClone(state)});
  }
  return results;
}
const hookCases=[
 [{event:'start',now:1000000,accepted:true},{event:'PostToolUse',tool:'Read',now:1000001,accepted:true},{event:'PostToolUse',tool:'Bash',now:1060000,accepted:true},{event:'PostToolUse',tool:'Write',now:1060001,accepted:true},{event:'PostToolUse',tool:'Read',now:1120000,accepted:true}],
 [{event:'PostToolUse',tool:'Read',now:1000000,accepted:false},{event:'PostToolUse',tool:'Read',now:1000001,accepted:false},{event:'PostToolUse',tool:'Bash',now:1060000,accepted:true}],
 [{event:'start',now:1000000,accepted:true},{event:'PostToolUse',tool:'Read',now:1000001,accepted:true},{event:'Stop',now:1000002,accepted:true}],
];
for(const steps of hookCases) add('hook_series',steps,await hookSeries(steps));
// Exercise the retained ACP emitter's own collection/finish code without a
// runtime, timer or backend. It provides measured correction evidence too.
const acpSource=text('scripts/hagency-acp-agent.mjs');
const from=acpSource.indexOf('function makeProgressEmitter(');
const until=acpSource.indexOf('\nfunction buildNudge(',from);
if(from<0||until<0) throw new Error('Review changed ACP progress emitter boundary');
const factory=new Function('acpUpdateFailedCall','acpUpdateToEvent','buildProgressSummary','decideProgressEvent','parseProgressFilter','process','Date','setInterval','clearInterval',acpSource.slice(from,until)+'\nreturn makeProgressEmitter;');
async function acpFinish(updates,filterDoc=null,delivered=null) {
  let line=null;
  const make=factory(acpUpdateFailedCall,acpUpdateToEvent,buildProgressSummary,decideProgressEvent,parseProgressFilter,{env:{}},{now:()=>1000000},()=>({unref(){}}),()=>{});
  const emitter=make({name:'fixture',messages:[{id:'input-1',from:'human',group:'fixture',ts:1}],filterDoc,cursor:0,runtime:{updatesSince:()=>updates.map(update=>({update})),updateCursor:()=>updates.length},post:async(_url,options)=>{line=options.body.summary;},log:()=>{}});
  await emitter.finish(delivered);return line;
}
for(const delivered of [null,0,1]) {
  const updates=[{sessionUpdate:'tool_call',kind:'read',toolCallId:'c'},{sessionUpdate:'tool_call_update',status:'completed',toolCallId:'c'}];
  add('acp_finish',{updates,delivered},await acpFinish(updates,null,delivered));
}
{
  const updates=[{sessionUpdate:'tool_call',kind:'read',toolCallId:'already-complete',status:'completed'}];
  add('acp_finish',{updates,delivered:null},await acpFinish(updates));
}
const corrections=[];
for(const bad of [null,[],false,'broken']) {
  const input={raw:{perGroup:{customer:bad}},group:'customer'};
  corrections.push({operation:'filter',input,legacy:parseProgressFilter(input.raw,{group:input.group}),native:{error:'selected perGroup rule is not an object'},rationale:'An explicitly malformed selected rule must not widen publication by falling back.'});
}
for(const tool of ['constructor','toString','__proto__']) corrections.push({operation:'verb',input:tool,legacy:{type:typeof verbFor(tool)},native:'worked',rationale:'Inherited JavaScript object properties are not vetted verbs.'});
const malformed={sessionUpdate:'tool_call_update',status:['failed']};
corrections.push({operation:'acp',input:malformed,legacy:{failed:acpUpdateFailedCall(malformed)},native:{failed:false},rationale:'A malformed non-string status is not failure evidence.'});
const flushed=[{event:'PostToolUse',tool:'Read',now:1000000,accepted:true},{event:'Stop',now:1000001,accepted:true}];
corrections.push({operation:'finished_totals',input:flushed,legacy:await hookSeries(flushed),native:'⏳ finished — read',rationale:'Finish describes the whole run even when earlier progress snapshots were accepted.'});
corrections.push({operation:'failed_call',input:[{sessionUpdate:'tool_call',kind:'read',toolCallId:'c'},{sessionUpdate:'tool_call_update',status:'failed',toolCallId:'c'},{sessionUpdate:'tool_call_update',status:'failed',toolCallId:'c'}],legacy:await acpFinish([{sessionUpdate:'tool_call',kind:'read',toolCallId:'c'},{sessionUpdate:'tool_call_update',status:'failed',toolCallId:'c'},{sessionUpdate:'tool_call_update',status:'failed',toolCallId:'c'}]),native:'⏳ finished, but nothing succeeded — 1 failed attempt',rationale:'Count a call failure once and remove that failed attempt from activity totals.'});
const hidden=[{sessionUpdate:'tool_call',kind:'execute',toolCallId:'c'},{sessionUpdate:'tool_call_update',status:'failed',toolCallId:'c'}];
corrections.push({operation:'excluded_failure',input:hidden,legacy:await acpFinish(hidden,{tools:{exclude:['Bash']}}),native:'⏳ finished',rationale:'Failure counting must honor the same tool exclusion as its start.'});
corrections.push({operation:'silent_filter',input:{events:[]},legacy:await acpFinish([],{events:[]}),native:null,rationale:'An ACP finish must not bypass events filtering.'});
const inherited={raw:{events:['done'],perGroup:{}},group:'__proto__'};
corrections.push({operation:'inherited_group',input:inherited,legacy:parseProgressFilter(inherited.raw,{group:inherited.group}),native:{events:['done'],source:'file'},rationale:'Inherited object properties cannot create a permissive group override.'});
const initial=(id,status)=>({sessionUpdate:'tool_call',kind:'read',toolCallId:id,...(status===undefined?{}:{status})});
for(const [updates,delivered,native] of [
  [[initial('a')],null,'finished — 1 attempt unresolved'],
  [[initial('a','pending')],null,'finished — 1 attempt unresolved'],
  [[initial('a','in_progress')],null,'finished — 1 attempt unresolved'],
  [[initial('a','failed')],null,'finished, but nothing succeeded — 1 failed attempt'],
  [[initial('a','failed'),initial('b','pending')],null,'finished — 1 failed attempt, 1 attempt unresolved'],
  [[initial('a','completed'),initial('b','failed'),initial('c','in_progress')],null,'finished — read, 1 failed, 1 attempt unresolved'],
  [[initial('a','pending')],0,'finished — 1 attempt unresolved, but sent nothing'],
]) {
  corrections.push({operation:'acp_outcomes',input:{updates,delivered},legacy:await acpFinish(updates,null,delivered),native:`⏳ ${native}`,rationale:'Initial status is current ACP evidence; pending or unconfirmed attempts cannot be reported as successful completed work.'});
}
const output=JSON.stringify({hashes,vectors,corrections},null,2)+'\n';
const file=new URL('../fixtures/progress.json',import.meta.url);
if(process.argv.includes('--check')) { if(readFileSync(file,'utf8').replaceAll('\r\n','\n')!==output) throw new Error('Progress oracle differs'); }
else writeFileSync(file,output);
console.log(JSON.stringify({vectors:vectors.length,corrections:corrections.length}));
