// Execute a stroke plan exclusively through PointHand MCP. No bitmap paste/open.
import {Client} from '../client/node_modules/@modelcontextprotocol/sdk/dist/esm/client/index.js';
import {StdioClientTransport} from '../client/node_modules/@modelcontextprotocol/sdk/dist/esm/client/stdio.js';
import {fileURLToPath} from 'node:url';
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {paintControl,verifyPaintSetup,validateSelectors} from './lib/paint-ui.mjs';
const argv=process.argv.slice(2);const selectorsAt=argv.indexOf('--selectors');let selectors={};
if(selectorsAt>=0){if(!argv[selectorsAt+1])throw new Error('--selectors needs a JSON file');selectors=validateSelectors(JSON.parse(readFileSync(argv[selectorsAt+1],'utf8')));argv.splice(selectorsAt,2);}
const [id,planPath,originX,originY,startColor='0',endColor]=argv;
if(argv.length<4)throw new Error('Usage: node scripts/paint-image.mjs <window-id> <plan.json> <canvas-x> <canvas-y> [start-color] [end-color] [--selectors names.json]');
const plan=JSON.parse(readFileSync(planPath,'utf8'));const window={id:Number(id)};
const origin=[Number(originX),Number(originY)];
if(!Number.isSafeInteger(window.id)||window.id<=0||!origin.every(v=>Number.isSafeInteger(v)&&v>=0))throw new Error('Supply an observed window and canvas origin');
const client=new Client({name:'pointhand-image-painter',version:'1'});
await client.connect(new StdioClientTransport({command:process.execPath,args:[fileURLToPath(new URL('../client/dist/index.js',import.meta.url))],env:{...process.env}}));
const call=async(name,args={})=>{const result=await client.callTool({name,arguments:{window,...args}});if(result.isError)throw new Error(JSON.stringify(result.content));return result;};
const state=async(shot=false)=>{const r=await call('get_window_state',{include_text:true,include_screenshot:shot,scale:.75});const data=JSON.parse(r.content.find(p=>p.type==='text').text);if(shot)for(const part of r.content)if(part.type==='image')writeFileSync(new URL('../.scratch/mona-lisa-progress.png',import.meta.url),Buffer.from(part.data,'base64'));return data;};
async function chooseColor(hex){
 let data=await state();
 await call('click',{element_index:paintControl(data.accessibility.tree,'editColors',selectors).index});
 data=await state();
 await call('set_value',{element_index:paintControl(data.accessibility.tree,'hex',selectors).index,value:hex});
 data=await state();
 if(paintControl(data.accessibility.tree,'hex',selectors).value.replace(/^#/, '').toLowerCase()!==hex.replace(/^#/, '').toLowerCase())throw new Error('RGB value did not update');
 await call('click',{element_index:paintControl(data.accessibility.tree,'confirm',selectors).index});
}
mkdirSync(new URL('../.scratch/',import.meta.url),{recursive:true});
const started=performance.now();let completed=0;
try{
 const initial=await state();
 verifyPaintSetup(initial.accessibility.tree,plan);
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
