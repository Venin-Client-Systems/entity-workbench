"""Fixed stdlib-only synthetic boundary probe. Never an application worker."""
import errno
import hashlib
import json
import os
from pathlib import Path
import socket
import sys


def require(condition):
    if not condition:
        raise ValueError('fixed-hostile-contract')


def unique(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result)
        result[key] = value
    return result


def read_json(path):
    with path.open('rb') as stream:
        data = stream.read(64 * 1024 + 1)
    require(len(data) <= 64 * 1024)
    return json.loads(data, object_pairs_hook=unique)


def write_json(path, value):
    data = json.dumps(value, sort_keys=True, allow_nan=False).encode()
    require(len(data) <= 16 * 1024)
    with path.open('xb') as stream:
        stream.write(data)


def file_attempt(path, write=False):
    # Neither branch truncates or writes. Successful write-open is itself a violation.
    try:
        descriptor = os.open(path, os.O_WRONLY if write else os.O_RDONLY)
    except OSError as error:
        return {'opened': False, 'read_completed': False, 'errno': error.errno}
    try:
        if not write:
            os.read(descriptor, 64)
        return {'opened': True, 'read_completed': not write, 'errno': None}
    finally:
        os.close(descriptor)


def network_attempt(protocol, port, marker):
    result = {'attempted': True, 'socket_created': False, 'connected': False,
              'send_accepted': False, 'received_echo': False, 'errno': None}
    kind = socket.SOCK_STREAM if protocol == 'tcp' else socket.SOCK_DGRAM
    channel = None
    try:
        channel = socket.socket(socket.AF_INET, kind)
        result['socket_created'] = True
        channel.settimeout(0.5)
        if protocol == 'tcp':
            channel.connect(('127.0.0.1', port))
            result['connected'] = True
            channel.sendall(marker)
            result['send_accepted'] = True
            result['received_echo'] = channel.recv(64) == marker
        else:
            result['send_accepted'] = channel.sendto(marker, ('127.0.0.1', port)) == len(marker)
            reply, address = channel.recvfrom(64)
            result['received_echo'] = reply == marker and address == ('127.0.0.1', port)
    except OSError as error:
        # Timeout is distinguished from a kernel error, without exception text.
        result['errno'] = errno.ETIMEDOUT if isinstance(error, TimeoutError) else error.errno
    finally:
        if channel is not None:
            channel.close()
    return result


def main():
    job = Path(__file__).resolve().parent.parent
    phase = 'bootstrap'
    try:
        assignment = read_json(job / 'input/assignment.json')
        require(set(assignment) == {'schema_version', 'job_id', 'prefix', 'manifest_sha256',
                                    'sibling', 'original', 'tcp_port', 'udp_port', 'marker'})
        require(type(assignment['schema_version']) is int and assignment['schema_version'] == 1)
        prefix = Path(assignment['prefix'])
        require(prefix.is_absolute() and prefix.resolve() == prefix)
        require(sys.flags.isolated == 1 and sys.flags.no_site == 1 and sys.dont_write_bytecode is True)
        require(Path(sys.executable) == prefix / 'install/bin/python3.13')
        require(Path(sys.prefix) == prefix / 'install' and Path(sys.base_prefix) == prefix / 'install')
        paths = [Path(path) for path in sys.path]
        stdlib = prefix / 'install/lib/python3.13'
        require(len(paths) == 3 and len(set(paths)) == 3 and set(paths) ==
                {prefix / 'install/lib/python313.zip', stdlib, stdlib / 'lib-dynload'})
        require(all(path.is_absolute() for path in paths))
        marker = assignment['marker'].encode('ascii')
        require(len(marker) == 32 and all(byte in b'0123456789abcdef' for byte in marker))
        require(all(type(assignment[key]) is int and 0 < assignment[key] <= 65535 for key in ('tcp_port', 'udp_port')))
        phase = 'assigned-controls'
        assigned = (job / 'input/sentinel.txt').read_bytes()
        require(assigned == b'fixed assigned synthetic sentinel\n')
        scratch = job / 'scratch/roundtrip.txt'
        with scratch.open('xb') as stream:
            stream.write(assigned)
        require(scratch.read_bytes() == assigned)
        phase = 'file-attempts'
        files = {'sibling_read': file_attempt(assignment['sibling']),
                 'original_write_open': file_attempt(assignment['original'], write=True),
                 'prefix_write_open': file_attempt(prefix / 'install/bin/python3.13', write=True)}
        phase = 'network-attempts'
        network = {protocol: network_attempt(protocol, assignment[protocol + '_port'], marker)
                   for protocol in ('tcp', 'udp')}
        result = {'schema_version': 1, 'recipe': 'python-hostile-v1', 'job_id': assignment['job_id'],
                  'manifest_sha256': assignment['manifest_sha256'], 'python_version': sys.version.split()[0],
                  'isolated': True, 'no_site': True, 'no_bytecode': True, 'verified_paths': True,
                  'assigned_read': True, 'scratch_roundtrip': True,
                  'sentinel_sha256': hashlib.sha256(assigned).hexdigest(), 'files': files, 'network': network}
        write_json(job / 'scratch/result.json', result)
        write_json(job / 'scratch/checkpoint.json', {'phase': 'complete'})
        return 0
    except Exception:
        write_json(job / 'scratch/failure.json', {'phase': phase, 'failure': 'hostile-contract-failed'})
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
