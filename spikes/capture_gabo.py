"""Addon do mitmdump: guarda os eventos que o Spotify de desktop manda ao gabo-receiver (doc 09, Fase 0)."""
import os
import time

OUT = os.path.join(os.path.dirname(__file__), "capture")
os.makedirs(OUT, exist_ok=True)
LOG = os.path.join(OUT, "events.log")


def log(line):
    with open(LOG, "a", encoding="utf-8") as f:
        f.write(time.strftime("%H:%M:%S ") + line + "\n")


def varint(b, i):
    v = s = 0
    while True:
        c = b[i]
        i += 1
        v |= (c & 0x7F) << s
        s += 7
        if c < 0x80:
            return v, i


def fields(b):
    """Campos de um protobuf: [(número, tipo, valor)]."""
    i, out = 0, []
    while i < len(b):
        key, i = varint(b, i)
        n, t = key >> 3, key & 7
        if t == 0:
            v, i = varint(b, i)
        elif t == 2:
            ln, i = varint(b, i)
            v, i = b[i:i + ln], i + ln
        elif t == 1:
            v, i = b[i:i + 8], i + 8
        elif t == 5:
            v, i = b[i:i + 4], i + 4
        else:
            raise ValueError(f"tipo {t}")
        out.append((n, t, v))
    return out


def describe(body):
    """PublishEventsRequest -> nomes dos eventos e dos fragmentos."""
    lines = []
    for n, t, env in fields(body):
        if n != 1 or t != 2:
            continue
        name, frags = "?", []
        for en, et, ev in fields(env):
            if en == 2:
                name = ev.decode(errors="replace")
            elif en == 3:
                f = dict((fn, fv) for fn, _, fv in fields(ev))
                frags.append(f"{f.get(1, b'?').decode(errors='replace')}({len(f.get(2, b''))}B)")
        lines.append(f"  {name}: {', '.join(frags)}")
    return lines


class Capture:
    def __init__(self):
        self.n = 0

    def http_connect(self, flow):
        log(f"CONNECT {flow.request.host}:{flow.request.port}")

    def request(self, flow):
        url = flow.request.pretty_url
        if flow.request.method != "CONNECT":
            log(f"req {flow.request.method} {url.split("?")[0][:120]}")
        if not any(k in url for k in ("gabo-receiver-service/v3/events/", "connect-state/v1/devices", "melody", "track-playback")):
            return
        self.n += 1
        body = flow.request.get_content(strict=False) or b""
        if body[:2] == bytes([0x1F, 0x8B]):
            import gzip
            body = gzip.decompress(body)
        path = os.path.join(OUT, f"{self.n:04d}.bin")
        with open(path, "wb") as f:
            f.write(body)
        ct = flow.request.headers.get("content-type", "")
        log(f"#{self.n} {flow.request.method} {url} [{ct}] {len(body)}B enc={flow.request.headers.get('content-encoding','')}")
        if "gabo-receiver" in url and "protobuf" in ct:
            try:
                for line in describe(body):
                    log(line)
            except Exception as e:
                log(f"  (não decodificou: {e})")

    def response(self, flow):
        if "gabo-receiver" in flow.request.pretty_url:
            log(f"  -> {flow.response.status_code} {len(flow.response.raw_content or b'')}B")

    def tls_failed_client(self, data):
        log(f"TLS recusado pelo client (pinning?): {data.context.server.address}")


addons = [Capture()]
