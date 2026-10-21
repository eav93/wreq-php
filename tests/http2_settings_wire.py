"""Run against a built extension: python3 tests/http2_settings_wire.py /path/to/extension.
Uses only loopback and the Python standard library; no external API or TLS keys.
"""
import json
import pathlib
import socket
import subprocess
import sys
import threading

ROOT = pathlib.Path(__file__).resolve().parent.parent
EXT = str(pathlib.Path(sys.argv[1]).resolve())


def capture(omit):
    frames = []
    errors = []
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        listener.listen()
        listener.settimeout(10)
        port = listener.getsockname()[1]

        def server():
            try:
                with listener.accept()[0] as sock:
                    sock.settimeout(5)

                    def read(n):
                        buf = b''
                        while len(buf) < n:
                            chunk = sock.recv(n - len(buf))
                            if not chunk:
                                raise EOFError('incomplete frame')
                            buf += chunk
                        return buf

                    assert read(24) == b'PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n'
                    sock.sendall(bytes.fromhex('000000040000000000'))
                    while True:
                        header = read(9)
                        payload = read(int.from_bytes(header[:3], 'big'))
                        if header[3] == 4 and not header[4] & 1:
                            frames.append(payload.hex())
                            sock.sendall(bytes.fromhex('000000040100000000'))
                        if header[3] == 1:
                            # HPACK indexed :status=200; END_HEADERS | END_STREAM.
                            sock.sendall(bytes.fromhex('0000010105') + header[5:9] + b'\x88')
                            return
            except Exception as exc:
                errors.append(repr(exc))

        thread = threading.Thread(target=server)
        thread.start()
        omit_option = '' if omit is None else "'http2_omit_initial_window_size' => " + str(omit).lower() + ','
        code = f"""
$c = new Wreq\\Ext\\Client(['http2_only'=>true,'no_proxy'=>true,'timeout'=>3.0,
'http2_enable_push'=>false,'http2_initial_window_size'=>65535,
'http2_initial_connection_window_size'=>65535,{omit_option}]);
echo $c->request('GET', 'http://127.0.0.1:{port}/')->status();"""
        result = subprocess.run(['php', '-d', 'extension=' + EXT, '-r', code],
                                cwd=ROOT, capture_output=True, text=True, timeout=10)
        thread.join(10)
        assert not thread.is_alive() and not errors, (errors, result.stdout, result.stderr)
        assert result.returncode == 0 and result.stdout == '200', (result.stdout, result.stderr)
        return frames


for omit in (None, False, True):
    expected = '000200000000' if omit else '00020000000000040000ffff'
    observed = capture(omit)
    assert observed == [expected], (omit, observed)
    print(json.dumps({'omit': omit, 'settings_payload': observed[0]}))

for options in (
    "'http2_omit_initial_window_size'=>'yes'",
    "'http2_omit_initial_window_size'=>true,'http2_initial_window_size'=>100000",
    "'http2_omit_initial_window_size'=>true,'http2_adaptive_window'=>true",
):
    code = " try {new Wreq\\Ext\\Client([" + options + "]); exit(1);} catch (Throwable $e) {echo $e->getMessage();}"
    result = subprocess.run(['php', '-d', 'extension=' + EXT, '-r', code], cwd=ROOT,
                            capture_output=True, text=True, timeout=10)
    assert result.returncode == 0 and 'http2_omit_initial_window_size' in result.stdout, result
print('PASS: 3 wire cases and 3 invalid-option cases')
