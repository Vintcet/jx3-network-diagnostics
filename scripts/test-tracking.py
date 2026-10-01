"""Real TCP/UDP + local relay fixture; no game client or third-party accelerator is controlled."""
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / '.tmp' / 'tracking-check'
OUT.mkdir(parents=True, exist_ok=True)

def echo_connection(conn):
    try:
        with conn:
            while data := conn.recv(4096):
                conn.sendall(data)
    except OSError:
        pass

def echo_tcp(port):
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', port))
        listener.listen()
        while True:
            conn, _ = listener.accept()
            threading.Thread(target=echo_connection, args=(conn,), daemon=True).start()

def backend(a, b, udp):
    for port in (a, b):
        threading.Thread(target=echo_tcp, args=(port,), daemon=True).start()
    with socket.socket(type=socket.SOCK_DGRAM) as sock:
        sock.bind(('127.0.0.1', udp))
        while True:
            data, addr = sock.recvfrom(4096)
            sock.sendto(data, addr)

def relay(port, a, b):
    threading.Thread(target=echo_tcp, args=(port,), daemon=True).start()
    start = time.monotonic()
    # Independent upstream traffic represents a relay process whose internal mapping is unknown.
    while True:
        target = a if time.monotonic() - start < 9 else b
        try:
            with socket.create_connection(('127.0.0.1', target), timeout=2) as conn:
                while target == (a if time.monotonic() - start < 9 else b):
                    conn.sendall(b'fixture-upstream' * 16)
                    conn.recv(4096)
                    time.sleep(.15)
        except OSError:
            time.sleep(.1)

def game(port, udp):
    with socket.create_connection(('127.0.0.1', port), timeout=2) as conn, socket.socket(type=socket.SOCK_DGRAM) as datagram:
        datagram.settimeout(1)
        while True:
            conn.sendall(b'fixture-game' * 16)
            conn.recv(4096)
            datagram.sendto(b'fixture-udp' * 16, ('127.0.0.1', udp))
            datagram.recvfrom(4096)
            time.sleep(.15)

def port():
    with socket.socket() as s:
        s.bind(('127.0.0.1', 0))
        return s.getsockname()[1]

def ready(port):
    for _ in range(100):
        try:
            socket.create_connection(('127.0.0.1', port), timeout=.2).close()
            return
        except OSError:
            time.sleep(.05)
    raise RuntimeError('Fixture did not start')

if len(sys.argv) > 1:
    {'backend': backend, 'relay': relay, 'game': game}[sys.argv[1]](*map(int, sys.argv[2:]))
else:
    a, b, udp, proxy = port(), port(), port(), port()
    children = []
    def launch(mode, *ports):
        child = subprocess.Popen([sys.executable, str(Path(__file__).resolve()), mode, *map(str, ports)], cwd=ROOT, creationflags=subprocess.CREATE_NO_WINDOW)
        children.append(child)
        return child
    try:
        launch('backend', a, b, udp)
        ready(a)
        proxy_process = launch('relay', proxy, a, b)
        ready(proxy)
        game_process = launch('game', proxy, udp)
        time.sleep(.3)
        exe = ROOT / 'src-tauri' / 'target' / 'debug' / 'jx3-network-diagnostics.exe'
        result = subprocess.run([str(exe), '--self-check', '--seconds', '18', '--game-pid', str(game_process.pid)], cwd=ROOT, capture_output=True, timeout=40, encoding='utf-8', creationflags=subprocess.CREATE_NO_WINDOW)
        assert result.returncode == 0, result.stderr
        data = json.loads(result.stdout)
        (OUT / 'result.json').write_text(result.stdout, encoding='utf-8')
        assert data['status'] == 'completed', data.get('error')
        report = data['report']
        assert any(p['pid'] == proxy_process.pid for p in report['relayProcesses']), 'Reverse socket ownership must find relay'
        endpoints = report['gameEndpoints']
        assert {a, b} <= {e['port'] for e in endpoints if e['source'] == 'relay'}, 'Both upstream destinations must be recorded'
        assert any(c['endpoint']['port'] == b and c['kind'] == 'first_seen' for c in report['connectionChanges']), 'New upstream address needs a timestamped event'
        status = report['trafficStatus']
        assert status['endpointEvents'] > 0 and status['endpointErrors'] == 0, status
        observed_udp = [e for e in endpoints if e['pid'] == game_process.pid and e['protocol'] == 'UDP4' and e['port'] == udp]
        assert observed_udp and observed_udp[0]['sent'] > 0 and observed_udp[0]['received'] > 0, 'UDP source/destination and ports must decode correctly in both directions'
        print(json.dumps({'status': data['status'], 'relayDetected': True, 'bothUpstreamsRecorded': True, 'udpRoundTripAttributed': True, 'endpointEvents': status['endpointEvents'], 'endpointErrors': status['endpointErrors'], 'addressChanges': len(report['connectionChanges'])}))
    finally:
        for child in reversed(children):
            child.terminate()
            child.wait(timeout=5)
