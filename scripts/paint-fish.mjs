// Live test: select an observed blank Paint window before running.
// Usage: node scripts/paint-fish.mjs <observed-window-id> <pixel-scale> <border-offset>
import { Client } from '../client/node_modules/@modelcontextprotocol/sdk/dist/esm/client/index.js';
import { StdioClientTransport } from '../client/node_modules/@modelcontextprotocol/sdk/dist/esm/client/stdio.js';
import { fileURLToPath } from 'node:url';
import { writeFileSync, mkdirSync } from 'node:fs';
const [id, scale='1', offset='0'] = process.argv.slice(2);
if (!id || !Number.isSafeInteger(Number(id)) || !(Number(scale)>0) || !Number.isFinite(Number(offset))) throw new Error('Supply an observed Paint id and valid scale/offset');
const window = {id:Number(id)};
const ellipse=(cx,cy,rx,ry)=>Array.from({length:49},(_,i)=>[cx+rx*Math.cos(i*Math.PI/24),cy+ry*Math.sin(i*Math.PI/24)]);
const strokes=[ellipse(770,470,65,38),[[705,470],[675,440],[680,470],[675,500],[705,470]],[[752,433],[770,406],[790,434]],[[755,507],[770,530],[786,507]],ellipse(808,458,4,4),[[807,480],[799,487]],[[746,454],[758,470],[746,484]]];
const transport=new StdioClientTransport({command:process.execPath,args:[fileURLToPath(new URL('../client/dist/index.js',import.meta.url))],env:{...process.env}});
const client=new Client({name:'pointhand-paint-acceptance',version:'1.0'});
await client.connect(transport);
try {
 const state = await client.callTool({name:'get_window_state',arguments:{window,include_screenshot:false,include_text:true}});
 if(state.isError) throw new Error(JSON.stringify(state.content));
 const tree = JSON.parse(state.content.find(p=>p.type==='text').text).accessibility.tree;
 const pencil = tree.split('\n').find(line=>/"(Çizgi kalemi|Pencil)"/.test(line));
 if(!pencil) throw new Error('Pencil was not found in the PointHand accessibility tree');
 const element_index = Number(pencil.match(/\[(\d+)\]/)[1]);
 const selected = await client.callTool({name:'click',arguments:{window,element_index}});
 if(selected.isError) throw new Error(JSON.stringify(selected.content));
 // Allow native HWND/GDI initialization before measuring frame timing.
 await new Promise(resolve=>setTimeout(resolve,200));
 for (const stroke of strokes) {
  const result=await client.callTool({name:'draw',arguments:{window,points:stroke.map(p=>p.map(v=>Math.round(v*Number(scale)+Number(offset)))),step:3}});
  if(result.isError) throw new Error(JSON.stringify(result.content));
 }
 const result=await client.callTool({name:'get_window_state',arguments:{window,scale:0.75}});
 if(result.isError) throw new Error(JSON.stringify(result.content));
 mkdirSync(new URL('../.scratch/',import.meta.url),{recursive:true});
 for(const part of result.content) if(part.type==='image') writeFileSync(new URL('../.scratch/paint-fish.png',import.meta.url),Buffer.from(part.data,'base64'));
 const overlayResult=await client.callTool({name:'overlay_status',arguments:{}});
 if(overlayResult.isError) throw new Error(JSON.stringify(overlayResult.content));
 const overlay=JSON.parse(overlayResult.content.find(p=>p.type==='text').text);
 writeFileSync(new URL('../.scratch/native-overlay-status.json',import.meta.url),JSON.stringify(overlay,null,2));
 if(overlay.enabled && (!overlay.ready || overlay.error || overlay.cursor_frames===0)) throw new Error('Native overlay did not render: '+JSON.stringify(overlay));
 console.log(JSON.stringify(overlay));
 console.log('Seven strokes completed through MCP and Rust; .scratch/paint-fish.png saved');
} finally {await client.close();}
