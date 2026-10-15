// Interactive manual test: move a physical mouse only after the drawing begins.
// Uses PointHand MCP exclusively; requires Python with Tkinter.
import {Client} from '../client/node_modules/@modelcontextprotocol/sdk/dist/esm/client/index.js';
import {StdioClientTransport} from '../client/node_modules/@modelcontextprotocol/sdk/dist/esm/client/stdio.js';
import {spawn} from 'node:child_process';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {resolve} from 'node:path';
import assert from 'node:assert/strict';
import {fileURLToPath} from 'node:url';
const root=fileURLToPath(new URL('../',import.meta.url));
const out=resolve(root,'.scratch');mkdirSync(out,{recursive:true});
const log=resolve(out,`physical-${Date.now()}.jsonl`);
const fixture=spawn('python',[resolve(root,'tests/e2e/target_app.py'),log],{stdio:'ignore',windowsHide:true});
const c=new Client({name:'pointhand-physical-interruption-test',version:'1'});
const pause=ms=>new Promise(r=>setTimeout(r,ms));
const events=()=>{try{return readFileSync(log,'utf8').trim().split('\n').map(JSON.parse);}catch{return [];}};
async function waitFor(read){for(let i=0;i<100;i++){const v=read();if(v)return v;await pause(50);}throw new Error('Fixture did not become ready');}
try {
 await pause(5000);
 await waitFor(()=>events().find(e=>e.kind==='ready'));
 await c.connect(new StdioClientTransport({command:process.execPath,args:[resolve(root,'client/dist/index.js')],env:{...process.env,POINTHAND_ALLOW:'python.exe',POINTHAND_OVERLAY:'1',POINTHAND_OVERLAY_CAPTURE:'1'}}));
 const call=async(name,args={})=>{const r=await c.callTool({name,arguments:args});assert.ok(!r.isError,JSON.stringify(r.content));return JSON.parse(r.content.find(p=>p.type==='text').text);};
 const windows=await call('list_windows',{query:'pointhand-e2e-target'});assert.equal(windows.length,1);const id=windows[0].id;
 const info=await call('get_system_info');const secondary=info.monitors.find(m=>!m.primary);
 const [wl,wt]=windows[0].rect;
 await call('click',{x:wl+80,y:wt+15});
 if(secondary){const [ml,mt]=secondary.work_rect;await call('drag',{window_id:id,from_x:200,from_y:15,to_x:ml+300-wl,to_y:mt+115-wt});}
 const moved=await call('get_window',{window_id:id});const [l,t]=moved.rect;
 const at=events().length;await call('press_key',{window_id:id,key:'F9'});const ready=await waitFor(()=>events().slice(at).find(e=>e.kind==='ready'));
 await call('click',{window_id:id,x:ready.entry[0]-l,y:ready.entry[1]-t});
 await call('type_text',{window_id:id,text:'Çizim başlayınca oynat'});
 const before=await call('guard_status');const [x,y]=ready.canvas;const start=events().length;
 const response=await c.callTool({name:'draw_strokes',arguments:{window_id:id,strokes:Array.from({length:20},()=>[x+60-l,y+80-t,x+260-l,y+130-t]),hold_ms:500,settle_ms:50}});
 assert.equal(response.isError,true,'No physical intervention was observed during the drawing');
 assert.match(JSON.stringify(response.content),/user_active/,'Expected genuine physical-input interruption');
 await pause(150);const status=await call('guard_status');
 assert.ok(status.physical_events>before.physical_events,'Physical hook must see a real input event');
 const delivered=events().slice(start);assert.ok(delivered.some(e=>e.kind==='press'),'The drawing must actually hold the button');
 assert.ok(delivered.some(e=>e.kind==='release'),'Interrupted drawing must release the button');
 assert.ok(delivered.some(e=>e.kind==='button_state'&&!e.left_down),'Windows must report the left button released');
 const report={passed:true,physical_events:status.physical_events-before.physical_events,interrupted:true,left_button_released:true,monitor:secondary?.rect??null};
 writeFileSync(resolve(out,'manual-physical-e2e.json'),JSON.stringify(report,null,2));console.log(JSON.stringify(report));
} finally {await c.close();fixture.kill();}
