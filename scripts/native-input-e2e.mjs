// Opt-in interactive check. Drives only our disposable fixture through PointHand MCP.
import {spawn} from 'node:child_process';
import {readFileSync,mkdirSync,writeFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import assert from 'node:assert/strict';
import {Client} from '../client/node_modules/@modelcontextprotocol/sdk/dist/esm/client/index.js';
import {StdioClientTransport} from '../client/node_modules/@modelcontextprotocol/sdk/dist/esm/client/stdio.js';
const root=fileURLToPath(new URL('../',import.meta.url));
const scratch=resolve(root,'.scratch');mkdirSync(scratch,{recursive:true});
const log=resolve(scratch,`input-${Date.now()}.jsonl`);
const fixture=spawn('python',[resolve(root,'tests/e2e/target_app.py'),log],{windowsHide:true,stdio:'ignore'});
const pause=ms=>new Promise(r=>setTimeout(r,ms));
const events=()=>{try{return readFileSync(log,'utf8').trim().split('\n').map(JSON.parse);}catch{return [];}};
const client=new Client({name:'pointhand-native-input-e2e',version:'1'});
try {
 let ready;
 for(let i=0;i<100&&!ready;i++){ready=events().find(e=>e.kind==='ready');if(!ready)await pause(100);}
 assert.ok(ready,'fixture startup');
 await client.connect(new StdioClientTransport({command:process.execPath,args:[resolve(root,'client/dist/index.js')],env:{...process.env,POINTHAND_ALLOW:'python.exe',POINTHAND_OVERLAY:'1'}}));
 const call=async(name,args={})=>{const r=await client.callTool({name,arguments:args});assert.ok(!r.isError,JSON.stringify(r.content));return JSON.parse(r.content.find(p=>p.type==='text').text);};
 const windows=await call('list_windows',{query:'pointhand-e2e-target'});assert.equal(windows.length,1);
 const id=windows[0].id;
 const [cx,cy]=ready.entry;
 await call('click',{x:cx,y:cy});
 await call('type_text',{window_id:id,text:'PointHand 123'});
 await call('press_key',{window_id:id,key:'Escape'});
 const [x,y]=ready.canvas;
 await call('move',{x:x+50,y:y+50});
 await call('click',{x:x+50,y:y+50});
 await call('scroll',{x:x+70,y:y+70,scrollY:240});
 await call('drag',{from_x:x+80,from_y:y+80,to_x:x+180,to_y:y+110});
 await pause(200);
 const received=events();
 assert.ok(received.some(e=>e.kind==='text'&&e.value==='PointHand 123'),'Unicode input delivered');
 for(const kind of ['click','release','wheel','motion'])assert.ok(received.some(e=>e.kind===kind),`${kind} delivered`);
 const overlay=await call('overlay_status');const guard=await call('guard_status');
 writeFileSync(resolve(scratch,'native-input-e2e.json'),JSON.stringify({passed:true,overlay,guard,event_count:received.length},null,2));
 console.log(JSON.stringify({passed:true,event_count:received.length,overlay,guard}));
} finally {await client.close();fixture.kill();}
