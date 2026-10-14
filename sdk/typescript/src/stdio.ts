import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { randomUUID } from "node:crypto";
import { Client,decodeResponse,ProtocolError,type Transport } from "./client.js";
export class StdioTransport implements Transport {
  private process:ChildProcessWithoutNullStreams;private sequence=0;private prefix=randomUUID();private buffer="";private closed=false;
  private pending=new Map<string,{resolve:(value:unknown)=>void;reject:(error:unknown)=>void;cleanup:()=>void}>();
  constructor(command:string,args:string[]=[],private timeoutMs=30000) {
    if(!Number.isFinite(timeoutMs)||timeoutMs<=0)throw new RangeError("timeoutMs must be positive");
    this.process=spawn(command,args,{stdio:["pipe","pipe","pipe"]});this.process.stdout.setEncoding("utf8");
    this.process.stdout.on("data",(chunk:string)=>{this.buffer+=chunk;if(this.buffer.length>16*1024*1024){this.fail(new ProtocolError("Runtime stdio frame exceeded 16 MiB"));this.process.kill();return;}let split:number;while((split=this.buffer.indexOf("\n"))>=0){const line=this.buffer.slice(0,split);this.buffer=this.buffer.slice(split+1);if(!line.trim())continue;try{const response=JSON.parse(line);if(!("id" in response))continue;const pending=this.pending.get(response.id);if(!pending){this.fail(new ProtocolError("Unexpected stdio response identity"));continue;}this.pending.delete(response.id);pending.cleanup();try{pending.resolve(decodeResponse(response,response.id));}catch(error){pending.reject(error);}}catch(error){this.fail(error);}}});
    this.process.stderr.on("data",()=>{});this.process.on("error",error=>this.fail(error));this.process.on("exit",()=>{this.closed=true;this.fail(new ProtocolError("Runtime process exited"));});
  }
  request(method:string,params:unknown,signal?:AbortSignal):Promise<unknown> {
    if(this.closed)return Promise.reject(new ProtocolError("Runtime process is closed"));if(signal?.aborted)return Promise.reject(signal.reason);
    const id=`${this.prefix}-${++this.sequence}`;
    return new Promise((resolve,reject)=>{let expired=false;const cancel=()=>{expired=true;this.pending.delete(id);reject(signal?.reason ?? new ProtocolError("Runtime stdio request timed out"));};const timer=setTimeout(()=>{cancel();this.close();},this.timeoutMs);const abort=()=>{clearTimeout(timer);cancel();this.close();};signal?.addEventListener("abort",abort,{once:true});const cleanup=()=>{clearTimeout(timer);signal?.removeEventListener("abort",abort);};this.pending.set(id,{resolve,reject,cleanup});this.process.stdin.write(JSON.stringify({jsonrpc:"2.0",id,method,params})+"\n",error=>{if(error && !expired){this.pending.delete(id);cleanup();reject(error);}});});
  }
  private fail(error:unknown){for(const pending of this.pending.values()){pending.cleanup();pending.reject(error);}this.pending.clear();}
  close(){if(this.closed)return;this.closed=true;this.process.stdin.end();this.process.kill();setTimeout(()=>{if(this.process.exitCode===null)this.process.kill("SIGKILL");},2000).unref();this.fail(new ProtocolError("Runtime process closed"));}
}
export function stdioClient(command:string,args:string[]=[],timeoutMs=30000){const transport=new StdioTransport(command,args,timeoutMs);return {client:new Client(transport),close:()=>transport.close()};}
