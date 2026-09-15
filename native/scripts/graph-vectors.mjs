import { readFile, writeFile } from 'node:fs/promises';
import { createTaskGraphStore, evaluateCondition } from '../../lib/task-graph.js';

// Execute the existing policy. No copied evaluator and no live agents/services.
const store = () => createTaskGraphStore({now:()=> '2026-09-10T00:00:00.000Z',dispatchMessage:()=>({id:'fixture_durable_message'})});
const node = (extra={}) => ({assignee:'agent',description:'Verify dependency policy',...extra});
const definition = graph => ({label:graph.label,nodes:Object.values(graph.nodes).map(n=>({id:n.id,assignee:n.assignee,description:n.description,depends_on:n.depends_on,condition:n.condition}))});
const progress = graph => Object.fromEntries(Object.values(graph.nodes).map(n=>[n.id,{status:n.status,result:n.result,error:n.error}]));
const cases=[];
const operands=[null,false,true,0,-0,0.25,1e-7,'','yes',[],{},[1],{score:0.5}];
for (const [i,result] of operands.entries()) {
  for (const [name,condition] of Object.entries({truth:{dep:'dep'},eq:{eq:result},neq:{neq:result},in:{in:[result]},op_eq:{op:'eq',value:result},op_in:{op:'in',value:[result]}})) {
    cases.push({name:`operand_${i}_${name}`,result,condition,status:'complete'});
  }
}
for (const status of ['pending','dispatched','active','complete','failed','skipped','cancelled']) {
  cases.push({name:`status_${status}`,result:true,condition:{eq:true},status});
}
for (const [name,result,condition] of [
  ['missing_eq_null',{}, {path:'absent',eq:null}],
  ['missing_eq_missing',{}, {path:'absent',op:'eq'}],
  ['null_eq_missing',null,{op:'eq'}],
  ['missing_neq_null',{}, {path:'absent',neq:null}],
  ['blocked_proto',{safe:{value:true}}, {path:'safe.__proto__',eq:null}],
  ['blocked_constructor',{constructor:true}, {path:'constructor'}],
  ['blocked_prototype',{safe:{prototype:1}}, {path:'safe.prototype',op:'eq'}],
  ['nested_fraction',{safe:{value:0.25}}, {path:'safe.value',eq:0.25}],
  ['field_alias',{safe:2}, {field:'safe',eq:2}],
  ['array_index',[0,0.25], {path:'1',eq:0.25}],
  ['array_leading_zero',[1,2], {path:'01',eq:2}],
  ['array_length',[1,2], {path:'length',eq:2}],
  ['unicode_length','😀', {path:'length',eq:2}],
  ['unicode_codeunit_length','😀', {path:'0.length',eq:1}],
  ['unicode_not_replacement','😀', {path:'0',eq:'�'}],
  ['ascii_codeunit','abc', {path:'1',eq:'b'}],
  ['null_path',null,{path:'safe',eq:null}],
  ['non_array_in',1,{in:1}],
  ['unknown_op',1,{op:'other'}],
  ['precedence',1,{eq:1,neq:1,in:[]}],
  ['empty_condition',false,{}],
]) cases.push({name,result,condition,status:'complete'});
for (const [kind,proto,result] of [['object',Object.prototype,{}],['array',Array.prototype,[]]]) {
  for (const key of Object.getOwnPropertyNames(proto)) {
    if (key==='constructor'||key==='__proto__'||typeof proto[key]!=='function') continue;
    cases.push({name:`${kind}_prototype_${key}`,result,condition:{path:key},status:'complete'});
  }
}
cases.push({name:'own_toString',result:{toString:false},condition:{path:'toString'},status:'complete'});
cases.push({name:'function_not_traversed',result:{},condition:{path:'toString.length',op:'eq'},status:'complete'});
const conditions=cases.map(c=>{
  const s=store();const g=s.createGraph({id:'fixture',owner:'owner',label:c.name,nodes:{dep:node({status:c.status,result:c.result}),next:node({depends_on:['dep'],condition:c.condition})}});
  return {name:c.name,node:definition(g).nodes[1],progress:progress(g),expected:evaluateCondition(g,g.nodes.next)};
});
const plans=[
  ['root_and_wait',{a:node(),b:node({depends_on:['a']})}],
  ['fraction_branch',{a:node({status:'complete',result:{score:0.25}}),yes:node({depends_on:['a'],condition:{path:'score',eq:0.25}}),no:node({depends_on:['a'],condition:{path:'score',eq:0.5}}),after:node({depends_on:['no']})}],
  ['reverse_failure',{z:node({depends_on:['y']}),y:node({depends_on:['x']}),x:node({status:'failed',error:'task failed'})}],
  ['cancelled_dependency',{a:node({status:'cancelled'}),b:node({depends_on:['a']}),c:node()}],
  ['all_terminal',{a:node({status:'complete',result:0.25}),b:node({status:'skipped'})}],
  ['all_failed',{a:node({status:'failed',error:'failed'}),b:node({status:'complete'})}],
  ['condition_wait',{next:node({condition:{dep:'root',eq:true}}),root:node({status:'active'})}],
  ['condition_failed',{next:node({condition:{dep:'root',eq:true}}),root:node({status:'failed',error:'failed'})}],
  ['multiple_results',{first:node({status:'complete',result:0.25}),second:node({status:'complete',result:{summary:'ok'}}),next:node({depends_on:['second','first']})}],
  ['cancel',{a:node({status:'active'}),b:node({depends_on:['a']}),c:node({status:'complete',result:42})},true],
];
const transitions=plans.map(([name,nodes,cancel=false])=>{
  const s=store();const initial=s.createGraph({id:'fixture',owner:'owner',label:name,nodes});
  const next=cancel?s.deleteGraph('fixture'):s.advanceGraph('fixture');
  return {name,cancel,graph:{definition:definition(initial),status:initial.status,progress:progress(initial)},expected:{status:next.status,progress:progress(next),assignments:Object.values(next.nodes).filter(n=>n.status==='dispatched'&&initial.nodes[n.id].status==='pending').map(n=>({node_id:n.id,assignee:n.assignee,description:n.description,dependency_results:n.depends_on.filter(dep=>next.nodes[dep].status==='complete').map(dep=>({node_id:dep,assignee:next.nodes[dep].assignee,result:next.nodes[dep].result}))}))}};
});
const target=new URL('../fixtures/graphs.json',import.meta.url);
const encoded=JSON.stringify({conditions,transitions},null,2)+'\n';
if (process.argv.includes('--check')) {
  if (await readFile(target,'utf8')!==encoded) throw new Error('Graph policy fixture is stale; run node native/scripts/graph-vectors.mjs');
} else await writeFile(target,encoded);
console.log(JSON.stringify({conditions:conditions.length,transitions:transitions.length}));
