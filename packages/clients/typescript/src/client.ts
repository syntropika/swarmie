import type { Methods, RuntimeEvent, SessionSnapshot, InputSpawnOptions, InputPromptContent, JsonValue } from "./generated.js";
export * from "./generated.js";
export class RpcError extends Error {
  constructor(public readonly code:number,message:string,public readonly data?:unknown){super(message);this.name="RpcError";}
}
export class ProtocolError extends Error { constructor(message:string){super(message);this.name="ProtocolError";} }
export type ClientOptions={token?:string;timeoutMs?:number;maxResponseBytes?:number};
export type EventOptions={afterSequence?:number;timeoutMs?:number;pollIntervalMs?:number;signal?:AbortSignal};
export interface Transport { request(method:string,params:unknown,signal?:AbortSignal):Promise<unknown>; }
function positive(value:number,label:string) {if(!Number.isFinite(value) || value<=0)throw new RangeError(`${label} must be positive`);return value;}
export function decodeResponse(value:unknown,id:string):unknown {
  if(!value || typeof value !== "object")throw new ProtocolError("Invalid JSON-RPC response");
  const message=value as {jsonrpc?:unknown;id?:unknown;error?:{code?:unknown;message?:unknown;data?:unknown};result?:unknown};
  if(message.jsonrpc!=="2.0" || message.id!==id)throw new ProtocolError("JSON-RPC response identity mismatch");
  if(message.error){if(typeof message.error.code!=="number" || typeof message.error.message!=="string")throw new ProtocolError("Malformed JSON-RPC error");throw new RpcError(message.error.code,message.error.message,message.error.data);}
  if(!("result" in message))throw new ProtocolError("JSON-RPC response has no result");
  return message.result;
}
export class HttpTransport implements Transport {
  private sequence=0;private prefix=crypto.randomUUID();private endpoint:URL;
  constructor(url:string,private options:ClientOptions={}) {
    this.endpoint=new URL(url);if(!["http:","https:"].includes(this.endpoint.protocol))throw new ProtocolError("Use an HTTP or HTTPS endpoint");
    if(this.endpoint.pathname==="/")this.endpoint.pathname="/rpc";
    positive(options.timeoutMs ?? 30000,"timeoutMs");positive(options.maxResponseBytes ?? 16*1024*1024,"maxResponseBytes");
  }
  async request(method:string,params:unknown,signal?:AbortSignal):Promise<unknown> {
    const id=`${this.prefix}-${++this.sequence}`;
    const timeout=AbortSignal.timeout(this.options.timeoutMs ?? 30000);const cancellation=signal ? AbortSignal.any([signal,timeout]) : timeout;
    const response=await fetch(this.endpoint,{method:"POST",redirect:"error",headers:{"Content-Type":"application/json",...(this.options.token ? {Authorization:`Bearer ${this.options.token}`}:{})},body:JSON.stringify({jsonrpc:"2.0",id,method,params}),signal:cancellation});
    if(!response.ok)throw new ProtocolError(`Runtime HTTP status ${response.status}`);
    const reader=response.body?.getReader();if(!reader)throw new ProtocolError("Runtime response has no body");
    const chunks:Uint8Array[]=[];let length=0;const limit=this.options.maxResponseBytes ?? 16*1024*1024;
    for(;;){const {done,value}=await reader.read();if(done)break;length+=value.length;if(length>limit){await reader.cancel();throw new ProtocolError("Runtime response exceeded the configured byte limit");}chunks.push(value);}
    const bytes=new Uint8Array(length);let offset=0;for(const chunk of chunks){bytes.set(chunk,offset);offset+=chunk.length;}
    let value:unknown;try{value=JSON.parse(new TextDecoder().decode(bytes));}catch{throw new ProtocolError("Runtime response is not JSON");}
    return decodeResponse(value,id);
  }
}
export class Client {
  constructor(public readonly transport:Transport){}
  static http(url:string,options:ClientOptions={}) {return new Client(new HttpTransport(url,options));}
  call<M extends keyof Methods>(method:M,params:Methods[M]["params"],signal?:AbortSignal):Promise<Methods[M]["result"]> {return this.transport.request(method,params,signal) as Promise<Methods[M]["result"]>;}
  callRaw(method:string,params:unknown={},signal?:AbortSignal):Promise<unknown> {return this.transport.request(method,params,signal);}
  initialize(){return this.call("runtime/initialize",{});}
  schema(){return this.call("runtime/schema",{});}
  discover(){return this.call("runtime/discover",{});}
  spawn(options:InputSpawnOptions):Promise<SessionSnapshot>{return this.call("session/create",options);}
  prompt(sessionId:string,prompt:string|InputPromptContent[]){return this.call("session/prompt",{sessionId,...(typeof prompt==="string" ? {text:prompt}:{content:prompt})});}
  close(sessionId:string){return this.call("session/close",{sessionId});}
  async *events(sessionId:string,options:EventOptions={}):AsyncGenerator<RuntimeEvent> {
    let sequence=options.afterSequence ?? 0;const timeout=positive(options.timeoutMs ?? 600000,"timeoutMs");const interval=positive(options.pollIntervalMs ?? 200,"pollIntervalMs");const end=performance.now()+timeout;
    while(performance.now()<end){options.signal?.throwIfAborted();const signal=AbortSignal.any([AbortSignal.timeout(Math.max(1,Math.ceil(end-performance.now()))),...(options.signal ? [options.signal]:[])]);const events=await this.call("session/transcript",{sessionId,afterSequence:sequence,limit:256},signal);
      for(const event of events){if(event.sequence<=sequence || event.sessionId!==sessionId)throw new ProtocolError("Runtime event ordering or session identity mismatch");sequence=event.sequence;yield event;}
      if(events.length<256)await new Promise<void>((resolve,reject)=>{const timer=setTimeout(done,Math.min(interval,end-performance.now()));function done(){options.signal?.removeEventListener("abort",cancel);resolve();}function cancel(){clearTimeout(timer);options.signal?.removeEventListener("abort",cancel);reject(options.signal?.reason);}options.signal?.addEventListener("abort",cancel,{once:true});});
    }
    throw new ProtocolError("Event polling deadline exceeded");
  }
  async waitForTurn(sessionId:string,options:EventOptions={}):Promise<RuntimeEvent[]> {
    const events:RuntimeEvent[]=[];for await(const event of this.events(sessionId,options)){events.push(event);if(event.type==="turn.ended")return events;if(event.type==="session.ended")throw new ProtocolError("Session ended before the turn completed");}return events;
  }
}
