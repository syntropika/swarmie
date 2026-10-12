#!/usr/bin/env python3
"""Generate client types and method catalogs from the daemon's canonical schema."""
import argparse,json,pathlib,re,urllib.request
ROOT=pathlib.Path(__file__).resolve().parents[1]
CONTRACTS=ROOT.parent/'contracts'

def ts(schema):
    if schema is True:return 'JsonValue'
    if schema is False:return 'never'
    if '$ref' in schema:return schema['$ref'].rsplit('/',1)[-1]
    if 'const' in schema:return json.dumps(schema['const'])
    if 'enum' in schema:return ' | '.join(json.dumps(v) for v in schema['enum'])
    for op,separator in [('allOf',' & '),('oneOf',' | '),('anyOf',' | ')]:
        if op in schema:
            union='('+separator.join(ts(x) for x in schema[op])+')'
            siblings={k:v for k,v in schema.items() if k!=op}
            return union+' & '+ts(siblings) if 'properties' in siblings or 'type' in siblings else union
    kind=schema.get('type')
    if not kind and 'required' in schema and not schema.get('properties'):
        return '{ '+ '; '.join(json.dumps(k)+': JsonValue' for k in schema['required'])+' }'
    if isinstance(kind,list):return ' | '.join(ts({**schema,'type':k}) for k in kind)
    if kind=='array':return f'Array<{ts(schema.get("items",True))}>'
    if kind=='object' or 'properties' in schema:
        properties=schema.get('properties',{});required=schema.get('required',[])
        result='{ '+ '; '.join(json.dumps(k)+('' if k in required else '?')+': '+ts(v) for k,v in properties.items())+' }'
        if schema.get('additionalProperties') not in [None,False]:result+=' & Record<string, '+ts(schema.get('additionalProperties',True))+'>'
        if not properties and schema.get('additionalProperties') is None:return 'Record<string, JsonValue>'
        return result
    return {'string':'string','integer':'number','number':'number','boolean':'boolean','null':'null'}.get(kind,'JsonValue')

def py(schema):
    if schema is True or schema is False:return 'Any'
    if '$ref' in schema:return schema['$ref'].rsplit('/',1)[-1]
    if 'enum' in schema:return 'Literal['+', '.join(repr(v) for v in schema['enum'])+']'
    if 'const' in schema:return 'Literal['+repr(schema['const'])+']'
    for op in ['oneOf','anyOf']:
        if op in schema:return 'Union['+', '.join(py(x) for x in schema[op])+']'
    if 'allOf' in schema:return 'dict[str, Any]'
    kind=schema.get('type')
    if isinstance(kind,list):return 'Union['+', '.join(py({**schema,'type':k}) for k in kind)+']'
    if kind=='array':return 'list['+py(schema.get('items',True))+']'
    if kind=='object':return 'dict[str, '+py(schema.get('additionalProperties',True))+']'
    return {'string':'str','integer':'int','number':'float','boolean':'bool','null':'None'}.get(kind,'Any')

def identifier(name):
    return name[0].upper()+name[1:]

def go(schema):
    if not isinstance(schema,dict):return 'json.RawMessage'
    if '$ref' in schema:return schema['$ref'].rsplit('/',1)[-1]
    if any(op in schema for op in ['oneOf','anyOf','allOf']):return 'json.RawMessage'
    kind=schema.get('type')
    if isinstance(kind,list):
        nonnull=[k for k in kind if k!='null']
        return '*'+go({**schema,'type':nonnull[0]}) if len(nonnull)==1 else 'json.RawMessage'
    if kind=='array':return '[]'+go(schema.get('items',True))
    if kind=='object' or 'properties' in schema:
        if 'properties' not in schema:return 'map[string]'+go(schema.get('additionalProperties',True))
        required=schema.get('required',[])
        fields=[]
        for key,value in schema['properties'].items():
            typ=go(value)
            if key not in required and not typ.startswith(('*','[]','map[','json.')):typ='*'+typ
            fields.append(identifier(key)+' '+typ+' `json:"'+key+('' if key in required else ',omitempty')+'"`')
        return 'struct {\n'+'\n'.join(fields)+'\n}'
    return {'string':'string','integer':'uint64' if schema.get('minimum',-1)>=0 else 'int64','number':'float64','boolean':'bool'}.get(kind,'json.RawMessage')

def swift(schema):
    if not isinstance(schema,dict):return 'JSONValue'
    if '$ref' in schema:return schema['$ref'].rsplit('/',1)[-1]
    if any(op in schema for op in ['oneOf','anyOf','allOf']):return 'JSONValue'
    kind=schema.get('type')
    if isinstance(kind,list):
        nonnull=[k for k in kind if k!='null']
        return swift({**schema,'type':nonnull[0]})+'?' if len(nonnull)==1 else 'JSONValue'
    if kind=='array':return '['+swift(schema.get('items',True))+']'
    if kind=='object':return '[String: '+swift(schema.get('additionalProperties',True))+']' if not schema.get('properties') else 'JSONValue'
    return {'string':'String','integer':'UInt64' if schema.get('minimum',-1)>=0 else 'Int64','number':'Double','boolean':'Bool','null':'JSONValue'}.get(kind,'JSONValue')

def generated_go(defs,methods):
    code='// Generated from runtime/schema. Run tools/generate.py to refresh.\npackage swarmie\n\nimport "encoding/json"\n\n'
    for name,value in sorted(defs.items()):
        if name=='RuntimeEvent':
            value={k:v for k,v in value.items() if k!='oneOf'}
            value={**value,'properties':{**value['properties'],'type':{'type':'string'},'data':True},'required':value['required']+['type']}
        typ=go(value)
        code+='type '+name+(' = ' if typ=='json.RawMessage' else ' ')+typ+'\n\n'
    code+='var Methods = []string{'+','.join(json.dumps(m) for m in sorted(methods))+'}\n'
    import subprocess
    return subprocess.run(['gofmt'],input=code,text=True,check=True,capture_output=True).stdout

def generated_swift(defs,methods):
    code='// Generated from runtime/schema. Run tools/generate.py to refresh.\n'
    for name,value in sorted(defs.items()):
        if value.get('type')=='object' and value.get('properties'):
            properties=dict(value['properties']);required=value.get('required',[])
            if name=='RuntimeEvent':properties.update({'type':{'type':'string'},'data':True});required=required+['type']
            fields=[]
            for key,field in properties.items():
                typ=swift(field)
                if key not in required and not typ.endswith('?'):typ+='?'
                fields.append((key,typ))
            code+='public struct '+name+': Codable, Sendable {\n'
            code+=''.join('    public var '+k+': '+t+'\n' for k,t in fields)
            code+='    public init('+', '.join(k+': '+t+(' = nil' if t.endswith('?') else '') for k,t in fields)+') {\n'
            code+=''.join('        self.'+k+' = '+k+'\n' for k,t in fields)+'    }\n}\n'
        else:code+='public typealias '+name+' = '+swift(value)+'\n'
    code+='public let protocolMethods: [String] = ['+','.join(json.dumps(m) for m in sorted(methods))+']\n'
    return code

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--url');parser.add_argument('--schema',type=pathlib.Path);parser.add_argument('--check',action='store_true');args=parser.parse_args()
    if args.url:
        request=urllib.request.Request(args.url.rstrip('/')+'/rpc',json.dumps({'jsonrpc':'2.0','id':1,'method':'runtime/schema','params':{}}).encode(),{'Content-Type':'application/json'})
        import os
        if os.environ.get('SWARMIE_TOKEN'):request.add_header('Authorization','Bearer '+os.environ['SWARMIE_TOKEN'])
        envelope=json.load(urllib.request.urlopen(request,timeout=30));schema=envelope['result']
    else:schema=json.loads((args.schema or CONTRACTS/'protocol.json').read_text())
    defs=schema['$defs'];methods=schema['x-methods'];files={CONTRACTS/'protocol.json':json.dumps(schema,indent=2,sort_keys=True)+'\n'}
    header='// Generated from runtime/schema. Run tools/generate.py to refresh.\n'
    code=header+'export type JsonValue = null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };\n'
    for name,value in sorted(defs.items()):code+='export type '+name+' = '+ts(value)+';\n'
    code+='export interface Methods {\n'+''.join('  '+json.dumps(name)+': { params: '+ts(value['params'])+'; result: '+ts(value['result'])+' };\n' for name,value in sorted(methods.items()))+'}\n'
    code+='export type { NativeClientMethods, SubmissionProjection, SessionProjection } from "./client.js";\n'
    files[CONTRACTS/'index.d.ts']=code
    code='# Generated from runtime/schema. Run tools/generate.py to refresh.\nfrom __future__ import annotations\nfrom typing import Any, Literal, Union, TypedDict, NotRequired\n\n'
    aliases=[]
    for name,value in sorted(defs.items()):
        if value.get('type')=='object' and value.get('properties'):
            code+='class '+name+'(TypedDict):\n'
            properties=dict(value['properties'])
            if 'oneOf' in value:
                properties['type']={'type':'string'}
                properties['data']=True
            for key,field in properties.items():
                typ=py(field);typ=typ if key in value.get('required',[]) else 'NotRequired['+typ+']';code+='    '+key+': '+typ+'\n'
            code+='\n'
        else:aliases.append(name+' = '+py(value))
    code+='\n'.join(aliases)+'\nMETHODS = '+repr(tuple(sorted(methods)))+'\n'
    files[ROOT/'python/swarmie_client/generated.py']=code
    files[ROOT/'go/generated.go']=generated_go(defs,methods)
    files[ROOT/'swift/Sources/SwarmieClient/Generated.swift']=generated_swift(defs,methods)
    for path,content in files.items():
        if args.check:
            if not path.exists() or path.read_text()!=content:raise SystemExit(f'Generated file is stale: {path.relative_to(ROOT.parent)}')
        else:path.write_text(content)
    print(f'{len(methods)} methods and {len(defs)} canonical types checked' if args.check else f'Generated {len(methods)} methods and {len(defs)} canonical types')
if __name__=='__main__':main()
