import { createInterface } from 'node:readline';
const mode = process.argv[2];
const lines = createInterface({input:process.stdin});
lines.on('line', line => {
  const {id,method,params} = JSON.parse(line);
  if(mode==='malformed') { console.log('not JSON'); return; }
  if(mode==='slow') { return; }
  if(method==='close') { console.log(JSON.stringify({id,ok:true,result:{bye:true}})); lines.close(); process.stdin.destroy(); return; }
  if(method==='fail') console.log(JSON.stringify({id,ok:false,error:'rejected input'}));
  else console.log(JSON.stringify({id,ok:true,result:{method,params}}));
});
