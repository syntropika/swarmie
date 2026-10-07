import test from "node:test";
import assert from "node:assert/strict";
import { Client, HttpTransport, RpcError, ProtocolError, decodeResponse } from "../dist/client.js";
import { stdioClient } from "../dist/stdio.js";
import { createServer } from "node:http";
import { once } from "node:events";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

test("decodes null results and preserves structured errors",()=>{
  assert.equal(decodeResponse({jsonrpc:"2.0",id:"a",result:null},"a"),null);
  assert.throws(()=>decodeResponse({jsonrpc:"2.0",id:"b",result:{}},"a"),ProtocolError);
  assert.throws(()=>decodeResponse({jsonrpc:"2.0",id:"a",error:{code:-32602,message:"invalid",data:{field:"cwd"}}},"a"),error=>error instanceof RpcError && error.code===-32602 && error.data.field==="cwd");
});
test("HTTP enforces identity, byte limit and cancellation",async()=>{
  const server=createServer(async(req,res)=>{let body="";for await(const chunk of req)body+=chunk;const request=JSON.parse(body);
    if(request.method==="slow"){setTimeout(()=>res.end("{}"),100);return;}
    res.setHeader("content-type","application/json");res.end(JSON.stringify({jsonrpc:"2.0",id:request.method==="wrong"?"wrong":request.id,result:request.method==="large"?"x".repeat(2048):null}));
  });server.listen(0,"127.0.0.1");await once(server,"listening");const url=`http://127.0.0.1:${server.address().port}`;
  try { const client=Client.http(url,{timeoutMs:20,maxResponseBytes:1024});assert.equal(await client.callRaw("null"),null);await assert.rejects(client.callRaw("wrong"),ProtocolError);await assert.rejects(client.callRaw("large"),ProtocolError);await assert.rejects(client.callRaw("slow")); }
  finally {server.closeAllConnections();await new Promise(resolve=>server.close(resolve));}
});
test("event polling rejects unordered transcript and has a deadline",async()=>{
 const client=new Client({request:async()=>[{sessionId:"s",sequence:2,type:"text.delta",data:{text:"a"}},{sessionId:"s",sequence:1,type:"text.delta",data:{text:"b"}}]});
 await assert.rejects(async()=>{for await(const event of client.events("s")){}},ProtocolError);
 const empty=new Client({request:async()=>[]});await assert.rejects(empty.waitForTurn("s",{timeoutMs:20,pollIntervalMs:2}),/deadline/);
});
async function conformance(client){
 const initialized=await client.initialize();assert.ok(initialized);
 const schema=await client.schema();assert.ok(Object.keys(schema["x-methods"]).includes("session/create"));
 assert.ok((await client.discover()).some(harness=>harness.harness==="mock" && harness.available));
 await assert.rejects(client.callRaw("not/a/method"),error=>error instanceof RpcError && error.code===-32601);
 const session=await client.spawn({harness:"mock",cwd:process.env.MPAI_TEST_CWD??process.cwd()});
 try {assert.equal((await client.prompt(session.id,[{type:"text",data:"Reply"}])).accepted,true);
 const events=await client.waitForTurn(session.id,{afterSequence:session.sequence,timeoutMs:3000,pollIntervalMs:5});
 assert.equal(events.filter(event=>event.type==="text.delta").map(event=>event.data.text).join(""),"client-ready");assert.ok(events.some(event=>event.type==="turn.ended"));assert.ok(events.every((e,i)=>i===0||e.sequence>events[i-1].sequence));
 const replay=await client.call("session/transcript",{sessionId:session.id,limit:4096});assert.ok(replay.some(e=>e.type==="text.delta"));
 }finally{await client.close(session.id);}
}
test("live HTTP daemon conformance",{skip:!process.env.MPAI_TEST_URL},async()=>{
 await conformance(Client.http(process.env.MPAI_TEST_URL,{token:process.env.MPAI_TEST_TOKEN}));
 await assert.rejects(Client.http(process.env.MPAI_TEST_URL,{token:"incorrect"}).initialize(),ProtocolError);
});
test("live owned stdio daemon conformance",{skip:!process.env.MPAI_TEST_BINARY},async()=>{
 const state=await mkdtemp(join(tmpdir(),"mpai-ts-client-"));const owned=stdioClient(process.env.MPAI_TEST_BINARY,["--state-dir",state,"serve","--mock-script",process.env.MPAI_TEST_SCRIPT]);
 try{await conformance(owned.client);}finally{owned.close();await rm(state,{recursive:true,force:true});}
});
test("owned stdio deadlines terminate a nonresponsive process",async()=>{
 const owned=stdioClient(process.execPath,["-e","process.stdin.resume()"],20);
 try{await assert.rejects(owned.client.initialize(),/timed out/);await assert.rejects(owned.client.initialize(),/closed/);}finally{owned.close();}
});
