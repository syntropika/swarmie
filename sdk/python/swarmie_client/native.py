"""C ABI lowering of the WIT client interface; all I/O executes inside Rust."""
import ctypes
import json
import pathlib
import sys
import threading
import weakref

class RpcError(Exception):
    def __init__(self, code, message, data=None):
        super().__init__(message)
        self.code, self.data = code, data

class ProtocolError(Exception): pass

_suffix = {'darwin': 'dylib', 'win32': 'dll'}.get(sys.platform, 'so')
_path = pathlib.Path(__file__).parent / 'native' / (('' if sys.platform == 'win32' else 'lib') + 'swarmie_client_ffi.' + _suffix)
_library = ctypes.CDLL(str(_path))
_library.swarmie_client_execute.argtypes = [ctypes.c_char_p]
_library.swarmie_client_execute.restype = ctypes.c_void_p
_library.swarmie_client_free.argtypes = [ctypes.c_void_p]
_library.swarmie_client_free.restype = None
_library.swarmie_client_abi_version.restype = ctypes.c_uint32
if _library.swarmie_client_abi_version() != 1: raise ImportError('Unsupported native client ABI')

def execute(command):
    # CDLL releases the GIL while Rust waits for network/process I/O.
    pointer = _library.swarmie_client_execute(json.dumps(command).encode())
    if not pointer: raise ProtocolError('Native allocation failed')
    try: value = json.loads(ctypes.string_at(pointer))
    finally: _library.swarmie_client_free(pointer)
    if 'ok' in value: return value['ok']
    error = value['error']
    kind, message = error['kind'], error['message']
    if kind == 'rpc': raise RpcError(error['code'], message, json.loads(error['data_json']) if error['data_json'] is not None else None)
    if kind == 'timeout': raise TimeoutError(message)
    if kind == 'cancelled': raise InterruptedError(message)
    if kind == 'invalid_input': raise ValueError(message)
    raise ProtocolError(message)

def invoke(command, cancel=None):
    operation = execute({'action': 'prepare'})
    done = threading.Event()
    watcher = None
    def watch():
        while not done.wait(.005):
            if cancel.is_set():
                execute({'action': 'cancel', 'operation': operation})
                return
    try:
        if cancel:
            if cancel.is_set(): execute({'action': 'cancel', 'operation': operation})
            else:
                watcher = threading.Thread(target=watch, daemon=True)
                watcher.start()
        return json.loads(execute({**command, 'operation': operation}))
    finally:
        done.set()
        if watcher: watcher.join()
        execute({'action': 'release', 'handle': operation})

class Handle:
    def __init__(self, command):
        self.id = execute(command)
        self._finalizer = weakref.finalize(self, execute, {'action': 'release', 'handle': self.id})
    def dispose(self): self._finalizer()
