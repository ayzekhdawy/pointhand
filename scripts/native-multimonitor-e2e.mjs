// Interactive: move only the disposable fixture through PointHand MCP.
// Requires an extended second display; never changes display configuration.
import {spawn} from 'node:child_process';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import assert from 'node:assert/strict';
import {Client} from '../client/node_modules/@modelcontextprotocol/sdk/dist/esm/client/index.js';
import {StdioClientTransport} from '../client/node_modules/@modelcontextprotocol/sdk/dist/esm/client/stdio.js';

const root=fileURLToPath(new URL('../',import.meta.url));
const out=resolve(root,'.scratch');mkdirSync(out,{recursive:true});
const log=resolve(out,`multimonitor-${Date.now()}.jsonl`);
const fixture=spawn('python',[resolve(root,'tests/e2e/target_app.py'),log],{windowsHide:true,stdio:'ignore'});
const client=new Client({name:'pointhand-multimonitor-e2e',version:'1'});
const pause=ms=>new Promise(r=>setTimeout(r,ms));
const events=()=>{try{return readFileSync(log,'utf8').trim().split('\n').map(JSON.parse);}catch{return [];}};
async function waitFor(read,label){for(let i=0;i<100;i++){const value=read();if(value)return value;await pause(50);}throw new Error(label);}
try {
  await waitFor(()=>events().find(e=>e.kind==='ready'),'fixture startup failed');
  await client.connect(new StdioClientTransport({command:process.execPath,args:[resolve(root,'client/dist/index.js')],env:{...process.env,POINTHAND_ALLOW:'python.exe',POINTHAND_OVERLAY:'1',POINTHAND_OVERLAY_CAPTURE:'1'}}));
  const call=async(name,args={})=>{const r=await client.callTool({name,arguments:args});assert.ok(!r.isError,JSON.stringify(r.content));return JSON.parse(r.content.find(p=>p.type==='text').text);};
  const info=await call('get_system_info');
  const primary=info.monitors.find(m=>m.primary),secondary=info.monitors.find(m=>!m.primary);
  assert.ok(primary&&secondary,'connect an extended second display first');
  const windows=await call('list_windows',{query:'pointhand-e2e-target'});assert.equal(windows.length,1);
  const id=windows[0].id;const before=await call('get_window',{window_id:id});
  const [wl,wt]=before.rect;const [ml,mt]=secondary.work_rect;
  // Windows can refuse programmatic activation of a freshly spawned process.
  // A real click on this visible fixture's title bar activates it normally;
  // the allow-list refuses the click if a different app covers that point.
  // Keep activation away from the drag origin: two rapid title-bar presses at
  // the same point would be interpreted as a maximize double-click.
  await call('click',{x:wl+80,y:wt+15});
  await call('drag',{window_id:id,from_x:200,from_y:15,to_x:ml+300-wl,to_y:mt+115-wt});
  const moved=await call('get_window',{window_id:id});
  const [l,t,r,b]=moved.rect;const center=[(l+r)/2,(t+b)/2];const [sl,st,sr,sb]=secondary.rect;
  assert.ok(center[0]>=sl&&center[0]<sr&&center[1]>=st&&center[1]<sb,`fixture did not reach the second display: ${JSON.stringify({before:before.rect,after:moved.rect,target:secondary.rect})}`);
  if(secondary.dpi)assert.equal(moved.dpi,secondary.dpi.x,'window DPI must match the target monitor');
  const snapshotAt=events().length;await call('press_key',{window_id:id,key:'F9'});
  const ready=await waitFor(()=>events().slice(snapshotAt).find(e=>e.kind==='ready'),'fixture geometry refresh failed');
  await call('click',{window_id:id,x:ready.entry[0]-l,y:ready.entry[1]-t});
  await call('type_text',{window_id:id,text:'PointHand secondary'});
  const [cx,cy]=ready.canvas;const clickAt=events().length;
  const target=[cx+80,cy+70];
  await call('click',{window_id:id,x:target[0]-l,y:target[1]-t});
  const pressed=await waitFor(()=>events().slice(clickAt).find(e=>e.kind==='press'),'second-display click missing');
  assert.equal(pressed.x,target[0],'click missed the requested physical X pixel');
  assert.equal(pressed.y,target[1],'click missed the requested physical Y pixel');
  const wheelAt=events().length;await call('scroll',{window_id:id,x:target[0]-l,y:target[1]-t,scrollY:240});
  await waitFor(()=>events().slice(wheelAt).filter(e=>e.kind==='wheel').length>=2,'second-display wheel missing');
  assert.equal(events().slice(wheelAt).filter(e=>e.kind==='wheel').reduce((sum,e)=>sum+e.delta,0),-240);
  const drawAt=events().length;
  await call('draw',{window_id:id,points:[[cx+60-l,cy+120-t],[cx+220-l,cy+150-t],[cx+300-l,cy+100-t]],step:4});
  const released=await waitFor(()=>events().slice(drawAt).find(e=>e.kind==='release'),'second-display stroke release missing');
  assert.equal(released.x,cx+300,'stroke must end at the requested physical X pixel');
  assert.equal(released.y,cy+100,'stroke must end at the requested physical Y pixel');
  assert.ok(events().slice(drawAt).some(e=>e.kind==='motion'),'second-display stroke motion missing');
  assert.ok(events().some(e=>e.kind==='text'&&e.value==='PointHand secondary'),'text on second display missing');
  const state=await client.callTool({name:'get_window_state',arguments:{window_id:id,include_text:false,include_screenshot:true,scale:1}});assert.ok(!state.isError);
  for(const part of state.content)if(part.type==='image')writeFileSync(resolve(out,'multimonitor-fixture.png'),Buffer.from(part.data,'base64'));
  const overlay=await call('overlay_status');assert.equal(overlay.ready,true);assert.equal(overlay.error,null);
  const report={passed:true,monitors:info.monitors,virtual_screen:info.virtual_screen,window_before:before.rect,window_after:moved.rect,dpi_before:before.dpi,dpi_after:moved.dpi,mixed_dpi:primary.dpi?.x!==secondary.dpi?.x,click_error_px:[pressed.x-target[0],pressed.y-target[1]],scroll_delta:-240,drawing:true,overlay};
  writeFileSync(resolve(out,'native-multimonitor-e2e.json'),JSON.stringify(report,null,2));
  writeFileSync(resolve(out,`native-multimonitor-${secondary.rect[0]<0?'left':'right'}.json`),JSON.stringify(report,null,2));
  console.log(JSON.stringify(report));
} finally {await client.close();fixture.kill();}
