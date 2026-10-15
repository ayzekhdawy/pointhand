// Real Rust binary regression tests. No mouse, clipboard or keyboard mutations.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { once } from 'node:events';
import { fileURLToPath } from 'node:url';
const exe=fileURLToPath(new URL('../bin/windows/x64/pointhand-computer-use.exe',import.meta.url));
function session(){
 const child=spawn(exe,[],{stdio:['pipe','pipe','pipe'],windowsHide:true,env:{...process.env,POINTHAND_OVERLAY:'0'}});
 const waiters=new Map();let next=1;
 createInterface({input:child.stdout}).on('line',line=>{const response=JSON.parse(line);const waiter=waiters.get(response.id);if(waiter){waiters.delete(response.id);clearTimeout(waiter.timer);waiter.resolve(response);}});
 const fail=error=>{for(const waiter of waiters.values()){clearTimeout(waiter.timer);waiter.reject(error);}waiters.clear();};
 child.on('error',fail);child.on('close',()=>fail(new Error('host exited')));
 function raw(line,id){return new Promise((resolve,reject)=>{const timer=setTimeout(()=>{waiters.delete(id);child.kill();reject(new Error('host response timeout'));},5000);waiters.set(id,{resolve,reject,timer});child.stdin.write(line+'\n');});}
 const call=(method,params={})=>{const id=next++;return raw(JSON.stringify({id,method,params}),id);};
 const close=async()=>{const exited=once(child,'close');await call('close');child.stdin.end();assert.equal((await exited)[0],0);};
 return {child,raw,call,close};
}
test('malformed JSON gets escaped correctly and the host continues',async()=>{
 const host=session();try{
  const bad=await host.raw('{"id":1,"method":"ping","params":{"quote":"bad" broken}}',0);
  assert.equal(bad.ok,false);assert.match(bad.error,/invalid json/);
  const ping=await host.call('ping');assert.equal(ping.result.engine,'rust');assert.equal(ping.result.overlay,'rust');
 }finally{await host.close();}
});
test('native stop event interrupts a long wait and blocks later actions',async()=>{
 const host=session();try{
  await host.call('ping');const start=performance.now();
  const waiting=host.call('batch',{actions:[{action:'wait',seconds:10},{action:'wait',seconds:0}]});
  await new Promise(resolve=>setTimeout(resolve,150));
  const stopper=spawn(exe,['--stop-pid',String(host.child.pid)],{windowsHide:true,stdio:'ignore'});
  assert.equal((await once(stopper,'close'))[0],0);
  const response=await waiting;assert.equal(response.result.length,1);assert.match(response.result[0].error,/stopped/);
  assert.ok(performance.now()-start<2000,'wait should cancel promptly');
  assert.match((await host.call('list_windows')).error,/stopped/);
 }finally{await host.close();}
});
test('invalid stroke requests fail before desktop input',async()=>{
 const host=session();try{
  assert.match((await host.call('draw',{points:[[Number.MAX_SAFE_INTEGER,0],[0,0]]})).error,/out of range/);
  assert.match((await host.call('draw',{points:Array.from({length:4097},()=>[0,0])})).error,/4096/);
  assert.match((await host.call('clipboard_write',{})).error,/missing text/);
  const status=await host.call('overlay_status');assert.equal(status.result.enabled,false);assert.equal(status.result.ready,false);
  const batch=await host.call('batch',{actions:[{action:'unknown'},{action:'wait',seconds:0}]});assert.equal(batch.result.length,1);assert.equal(batch.result[0].ok,false);
 }finally{await host.close();}
});
test('EOF exits cleanly',async()=>{
 const host=session();await host.call('ping');const exited=once(host.child,'close');host.child.stdin.end();assert.equal((await exited)[0],0);
});
