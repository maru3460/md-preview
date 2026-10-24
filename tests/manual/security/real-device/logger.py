# 実機の WKWebView から外へ出た接続を記録する、記録用のサーバ。
#
# 127.0.0.1 の 8766 で待ち、接続が来たら時刻・ポート・要求の 1 行目を標準出力に出す。
# 画像・CSS・フォント・iframe・fetch などの本物の読み込みの行き先。接続だけ張って何も
# 送らないものも拾えるよう、受けた時点（ACCEPT）でも 1 行出す
# どの要求にも 204 を返す（中身は要らない。来たかどうかだけを見る）。
#
# 使い方: python3 -u logger.py > server.log
import socket
import sys
import threading
import time

PORTS = (8766,)


def log(msg):
    print(f"{time.strftime('%H:%M:%S')} {msg}", flush=True)


def handle(conn, port):
    try:
        conn.settimeout(3)
        data = conn.recv(4096)
        line = data.split(b'\r\n')[0].decode(errors='replace') if data else '(何も送られなかった)'
        log(f"port={port} {line}")
        if data:
            conn.sendall(b'HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: 0\r\n\r\n')
    except Exception as e:
        log(f"port={port} (切れた: {e})")
    finally:
        conn.close()


def serve(port):
    s = socket.socket()
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind(('127.0.0.1', port))
    s.listen(50)
    while True:
        conn, _ = s.accept()
        log(f"ACCEPT port={port}")
        threading.Thread(target=handle, args=(conn, port), daemon=True).start()


for p in PORTS:
    threading.Thread(target=serve, args=(p,), daemon=True).start()
log(f"listening on {', '.join(map(str, PORTS))}")
try:
    while True:
        time.sleep(1)
except KeyboardInterrupt:
    sys.exit(0)
