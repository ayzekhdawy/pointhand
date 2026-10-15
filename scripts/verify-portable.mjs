// Load even the test client's SDK from the extracted package, not this checkout.
import assert from 'node:assert/strict';
import {readFileSync,existsSync} from 'node:fs';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {createHash} from 'node:crypto';

const root=resolve(process.argv[2]??'.');
const sdk=resolve(root,'client/node_modules/@modelcontextprotocol/sdk/dist/esm/client');
const {Client}=await import(pathToFileURL(resolve(sdk,'index.js')).href);
const {StdioClientTransport}=await import(pathToFileURL(resolve(sdk,'stdio.js')).href);
const exe=resolve(root,'bin/windows/x64/pointhand-computer-use.exe');
const manifest=JSON.parse(readFileSync(resolve(root,'bin/windows/x64/manifest.json'),'utf8'));
const entry=manifest.find(item=>item.name==='pointhand-computer-use.exe');assert.ok(entry,'host manifest entry');
const bytes=readFileSync(exe);assert.equal(bytes.length,entry.size);assert.equal(createHash('sha256').update(bytes).digest('hex'),entry.sha256);
assert.ok(existsSync(resolve(root,'scripts/lib/paint-ui.mjs')),'portable Paint selector helper');
const env={...process.env,POINTHAND_OVERLAY:'0'};delete env.POINTHAND_HOST;
const client=new Client({name:'pointhand-portable-verifier',version:'1'});
try {
  await client.connect(new StdioClientTransport({command:process.execPath,args:[resolve(root,'client/dist/index.js')],cwd:root,env}));
  const {tools}=await client.listTools();
  for(const name of ['get_system_info','guard_status','emergency_stop'])assert.ok(tools.some(t=>t.name===name),`missing ${name}`);
  const result=await client.callTool({name:'get_system_info',arguments:{}});assert.ok(!result.isError,JSON.stringify(result.content));
  const info=JSON.parse(result.content.find(p=>p.type==='text').text);assert.equal(info.platform,'windows');
  console.log(JSON.stringify({portable_startup:true,manifest_verified:true,tools:tools.length,platform:info.platform}));
} finally {await client.close();}
