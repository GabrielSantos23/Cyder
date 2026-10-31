# -*- coding: utf-8 -*-
"""Decodifica os corpos do gabo-receiver capturados do desktop (PublishEventsRequest).

Uso: python decode_gabo.py <arquivo.bin> [nome_do_evento]
Imprime cada envelope: nome, sequence e os campos de cada fragmento.
"""
import sys


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
            v, i = b[i:i + 4], i + 5 - 5
            i += 4
        else:
            raise ValueError("wiretype %d" % t)
        out.append((n, t, v))
    return out


def show(b, indent=""):
    for n, t, v in fields(b):
        if t == 2:
            txt = printable(v)
            if txt:
                print(f"{indent}{n}: str {txt!r}")
            elif len(v) in (4, 8, 16, 20) and not all(32 <= c < 127 for c in v):
                print(f"{indent}{n}: hex {v.hex()}")
            else:
                print(f"{indent}{n}: bytes[{len(v)}] {v[:24].hex()}{'…' if len(v) > 24 else ''}")
                sub = fields(v)
                if sub and all(1 <= f[0] <= 20 for f in sub):
                    show(v, indent + "    ")
        else:
            if t == 1:
                import struct
                print(f"{indent}{n}: double {struct.unpack('<d', v)[0]}")
            else:
                print(f"{indent}{n}: {v}")


def printable(v):
    try:
        s = v.decode("utf-8")
        return s if all(32 <= ord(c) < 127 or c in "áéíóúâêôãõàçÁÉÍÓÚÂÊÔÃÕÀÇ" for c in s) else None
    except Exception:
        return None


def main():
    path = sys.argv[1]
    want = sys.argv[2] if len(sys.argv) > 2 else None
    body = open(path, "rb").read()
    for n, t, env in fields(body):
        if n != 1 or t != 2:
            continue
        name, frags, extra = "?", [], []
        for en, et, ev in fields(env):
            if en == 1:
                extra.append(("name-field-1", ev))
            elif en == 2:
                name = ev.decode(errors="replace")
            elif en == 3:
                f = {fn: fv for fn, _, fv in fields(ev)}
                frags.append((f.get(1, b"?").decode(errors="replace"), f.get(2, b"")))
            elif en in (4, 5):
                extra.append((f"field-{en}", ev))
        if want and want.lower() not in name.lower():
            continue
        print(f"=== {name}  seq={ {k: (v.hex() if isinstance(v, bytes) else v) for k, v in extra} }")
        for fname, fdata in frags:
            print(f"  -- {fname} ({len(fdata)}B)")
            if fname == "message":
                show(fdata, "      ")
            else:
                show(fdata, "      ")


if __name__ == "__main__":
    main()
