// Small operator for PointHand MCP; all app interactions go through this client.
import {Client} from '../client/node_modules/@modelcontextprotocol/sdk/dist/esm/client/index.js';
import {StdioClientTransport} from '../client/node_modules/@modelcontextprotocol/sdk/dist/esm/client/stdio.js';
import {fileURLToPath} from 'node:url';
import {writeFileSync,mkdirSync} from 'node:fs';
const [id,operation='state',payload='{}']=process.argv.slice(2);
const window={id:Number(id)};
const client=new Client({name:'pointhand-image-operator',version:'1'});
await client.connect(new StdioClientTransport({command:process.execPath,args:[fileURLToPath(new URL('../client/dist/index.js',import.meta.url))],env:{...process.env}}));
const call=async(name,args)=>{const result=await client.callTool({name,arguments:args});if(result.isError)throw new Error(JSON.stringify(result.content));return result;};
try{
 if(operation!=='state'){
  const args=JSON.parse(payload);
  if(args.elementName){
   const state=await call('get_window_state',{window,include_screenshot:false,include_text:true});
   const tree=JSON.parse(state.content.find(p=>p.type==='text').text).accessibility.tree;
   const lines=tree.split('\n').filter(line=>line.includes('"'+args.elementName+'"')&&(!args.role||line.includes('] '+args.role+' ')));
   if(lines.length!==1)throw new Error('Expected one element: '+JSON.stringify(lines));
   args.element_index=Number(lines[0].match(/\[(\d+)\]/)[1]);delete args.elementName;delete args.role;
  }
  await call(operation,{window,...args});
 }
 const state=await call('get_window_state',{window,include_text:true,scale:.75});
 mkdirSync(new URL('../.scratch/',import.meta.url),{recursive:true});
 for(const part of state.content){if(part.type==='text'){writeFileSync(new URL('../.scratch/paint-current.json',import.meta.url),part.text);console.log(JSON.parse(part.text).accessibility.tree);}else if(part.type==='image')writeFileSync(new URL('../.scratch/paint-current.png',import.meta.url),Buffer.from(part.data,'base64'));}
}finally{await client.close();}
