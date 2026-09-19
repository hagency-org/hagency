// Build-time behavior evidence from the existing implementation; no live state.
import { readFileSync, writeFileSync } from 'node:fs';
import { resourceAllocationBudget } from '../../lib/resource-allocation-budget.js';
const base = {preset:{id:'medium',ceiling:{tokens:1000,period:'monthly'}},seatId:'shared',commitments:[]};
const active = (id,presetId,allocatedTokens,seatId='shared') => ({id,presetId,allocatedTokens,seatId,state:'active'});
const commitments = [active('other-pool','high',4000), active('other-account','private',9000,'private'),
  active('active','medium',100), {...active('reserved','medium',200),state:'pending',fulfillment:{phase:'planned'}},
  {...active('ended','medium',10000),state:'ended'}, {...active('failed','medium',10000),state:'pending',fulfillment:{phase:'failed'}},
  {...active('unapproved','medium',null),state:'pending'}, {...active('completed','medium',500),state:'pending',fulfillment:{phase:'complete'}}];
const cases = [
  ['empty',base], ['active-and-reserved',{...base,commitments}],
  ['retry-reservation',{...base,commitments,excludeEngagementId:'reserved'}],
  ['retry-other-pool',{...base,commitments,excludeEngagementId:'other-pool'}],
  ['unknown-ceiling',{...base,preset:{id:'medium'}}],
  ['zero-ceiling',{...base,preset:{id:'medium',ceiling:{tokens:0,period:'monthly'}}}],
  ['overcommitted',{...base,commitments:[active('too-much','medium',2000)]}],
];
for (const quotaTokens of [null,0,100,10000]) for (const period of ['daily','monthly']) for (const forAutoJoin of [false,true])
  cases.push([`quota-${quotaTokens}-${period}-automatic-${forAutoJoin}`,{...base,commitments,declaration:{quotaTokens,period},forAutoJoin}]);
cases.push(['automatic-undeclared',{...base,declaration:{},forAutoJoin:true}],['automatic-no-declaration',{...base,forAutoJoin:true}]);
cases.push(
  ['null-versus-missing-period',{...base,preset:{id:'medium',ceiling:{tokens:1000,period:null}},declaration:{quotaTokens:100}}],
  ['missing-versus-null-period',{...base,preset:{id:'medium',ceiling:{tokens:1000}},declaration:{quotaTokens:100,period:null}}],
  ['both-null-period',{...base,preset:{id:'medium',ceiling:{tokens:1000,period:null}},declaration:{quotaTokens:100,period:null}}],
  ['both-missing-period',{...base,preset:{id:'medium',ceiling:{tokens:1000}},declaration:{quotaTokens:100}}],
);
const output=JSON.stringify(cases.map(([name,input])=>({name,input,expected:resourceAllocationBudget(input)})),null,2)+'\n';
const path=new URL('../fixtures/allocation.json',import.meta.url);
if(process.argv.includes('--check')) {if(readFileSync(path,'utf8')!==output) throw new Error('Allocation vectors differ from JavaScript');}
else writeFileSync(path,output);
