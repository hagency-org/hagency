import { readFileSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import vm from 'node:vm';
const source=readFileSync(new URL('../../lib/agent-ops-client-auth.js',import.meta.url),'utf8');
const start=source.indexOf('function canonicalize(value) {');
const end=source.indexOf('export function agentOpsBodyDigest(');
if(start<0||end<=start)throw new Error('Review the changed JavaScript encoder');
const canonicalize=vm.runInNewContext(source.slice(start,end).replace(/^export /gm,'')+'\ncanonicalAgentOpsJson');
const values=[0,-0,1,0.25,-0.25,0.1+0.2,1e-6,1e-7,1e20,1e21,1e23,Number.MIN_VALUE,Number.MAX_VALUE,Number.MAX_SAFE_INTEGER,1000000000000000100];
let seed=0x8fc76a971812f20bn;
const view=new DataView(new ArrayBuffer(8));
for(let i=0;i<256;i++){
  seed=BigInt.asUintN(64,seed*6364136223846793005n+1442695040888963407n);
  view.setBigUint64(0,seed);const value=view.getFloat64(0);
  if(Number.isFinite(value))values.push(value);
}
const vectors=values.map((value,index)=>{
  const input={z:[value,{score:value}],a:{'10':value,'2':-value,'01':value}};
  if(index===0) {input['\uE000']=0.25;input['😀']=1e-7;input['小白']={score:0.75};}
  const canonical=canonicalize(input);
  return {input,canonical,sha256:createHash('sha256').update(canonical).digest('hex')};
});
const path=new URL('../fixtures/payloads.json',import.meta.url);
const output=JSON.stringify(vectors,null,2)+'\n';
if(process.argv.includes('--check')){
  if(readFileSync(path,'utf8')!==output)throw new Error('Payload vectors differ from JavaScript');
}else writeFileSync(path,output);
console.log(JSON.stringify({vectors:vectors.length}));
