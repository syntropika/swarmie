#!/usr/bin/env python3
"""A deterministic loopback forward for exercising supervisor process ownership."""
import asyncio, json, os, pathlib, sys

forward=sys.argv[sys.argv.index('-L')+1].split(':')
local=int(forward[1]);remote=int(forward[3])
log=pathlib.Path(os.environ['MPAI_SSH_FIXTURE_LOG'])
with log.open('a') as output:output.write(json.dumps({'pid':os.getpid(),'localPort':local,'remotePort':remote})+'\n')
async def pump(reader,writer):
    try:
        while data:=await reader.read(65536):
            writer.write(data);await writer.drain()
    except (OSError,asyncio.CancelledError):pass
    finally:writer.close()
async def client(reader,writer):
    try:upstream,output=await asyncio.open_connection('127.0.0.1',remote)
    except OSError:writer.close();return
    await asyncio.gather(pump(reader,output),pump(upstream,writer))
async def main():
    server=await asyncio.start_server(client,'127.0.0.1',local)
    async with server:await server.serve_forever()
asyncio.run(main())
