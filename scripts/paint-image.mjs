// Execute a stroke plan exclusively through PointHand MCP. No bitmap paste/open.
import {Client} from '../client/node_modules/@modelcontextprotocol/sdk/dist/esm/client/index.js';
import {StdioClientTransport} from '../client/node_modules/@modelcontextprotocol/sdk/dist/esm/client/stdio.js';
import {fileURLToPath} from 'node:url';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
const [id,planPath,originX,originY,startColor='0',endColor]=process.argv.slice(2);
const plan=JSON.parse(readFileSync(planPath,'utf8'));const window={id:Number(id)};
const origin=[Number(originX),Number(originY)];
if(!Number.isSafeInteger(window.id)||!origin.every(Number.isSafeInteger))throw new Error('Supply an observed window and canvas origin');
const client=new Client({name:'pointhand-image-painter',version:'1'});
await client.connect(new StdioClientTransport({command:process.execPath,args:[fileURLToPath(new URL('../client/dist/index.js',import.meta.url))],env:{...process.env}}));
const call=async(name,args={})=>{const result=await client.callTool({name,arguments:{window,...args}});if(result.isError)throw new Error(JSON.stringify(result.content));return result;};
const state=async(shot=false)=>{const r=await call('get_window_state',{include_text:true,include_screenshot:shot,scale:.75});const data=JSON.parse(r.content.find(p=>p.type==='text').text);if(shot)for(const part of r.content)if(part.type==='image')writeFileSync(new URL('../.scratch/mona-lisa-progress.png',import.meta.url),Buffer.from(part.data,'base64'));return data;};
const index=(tree,name,role)=>{const lines=tree.split('\n').filter(line=>line.includes('] '+role+' ')&&line.includes('"'+name+'"'));if(lines.length!==1)throw new Error('Expected one '+name+': '+JSON.stringify(lines));return Number(lines[0].match(/\[(\d+)\]/)[1]);};
async function chooseColor(hex){
 let data=await state();
 await call('click',{element_index:index(data.accessibility.tree,'Renkleri düzenle','Button')});
 data=await state();
 await call('set_value',{element_index:index(data.accessibility.tree,'RGB onaltılık','Edit'),value:hex});
 data=await state();
 if(!data.accessibility.tree.toLowerCase().includes('value="'+hex.toLowerCase()+'"'))throw new Error('RGB value did not update');
 await call('click',{element_index:index(data.accessibility.tree,'Tamam','Button')});
}
mkdirSync(new URL('../.scratch/',import.meta.url),{recursive:true});
const started=performance.now();let completed=0;
try{
 const initial=await state();
 if(!initial.accessibility.tree.includes(plan.canvas.join(' × ')+'px'))throw new Error('Canvas dimensions do not match plan');
 if(!initial.accessibility.tree.includes('value="'+plan.pen_size+' piksel"'))throw new Error('Set the pencil size before drawing');
 for(let color=Number(startColor);color<(endColor===undefined?plan.colors.length:Number(endColor));color++){
  const group=plan.colors[color];await chooseColor(group.hex);
  for(let offset=0;offset<group.strokes.length;offset+=500){
   const strokes=group.strokes.slice(offset,offset+500).map(p=>p.map((value,index)=>value+origin[index%2]));
   const result=await call('draw_strokes',{strokes});const data=JSON.parse(result.content.find(p=>p.type==='text').text);
   completed+=data.strokes;
   writeFileSync(new URL('../.scratch/mona-lisa-checkpoint.json',import.meta.url),JSON.stringify({window:window.id,origin,plan:planPath,color,nextOffset:offset+strokes.length,completed,elapsed_seconds:(performance.now()-started)/1000}));
  }
  await state(true);
  console.log(JSON.stringify({color:color+1,total_colors:plan.colors.length,hex:group.hex,strokes:completed,seconds:Math.round((performance.now()-started)/1000)}));
 }
 const overlay=await call('overlay_status');console.log(overlay.content.find(p=>p.type==='text').text);
 console.log('PointHand drawing pass complete');
}finally{await client.close();}
