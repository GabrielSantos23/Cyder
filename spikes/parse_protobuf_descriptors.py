# -*- coding: utf-8 -*-
"""Extrai descritores protobuf (FileDescriptorProto) embutidos num binário.

Procura padrões `0x0A <len> <nome>.proto` (campo name do descritor),
recupera o comprimento do blob testando prefixos de comprimento antes
dele e decodifica mensagens/campos recursivamente.
Uso: python -I parse_desc.py <binario> <filtro_mensagens_regex>
"""
import re
import sys


def read_varint(buf, i, end):
    r = 0
    s = 0
    while i < end:
        b = buf[i]
        i += 1
        r |= (b & 0x7F) << s
        if not b & 0x80:
            return r, i
        s += 7
        if s > 70:
            raise ValueError("varint longo demais")
    raise ValueError("varint sem fim")


def parse_fields(buf, i, end):
    out = []
    while i < end:
        tag, i = read_varint(buf, i, end)
        fn, wt = tag >> 3, tag & 7
        if wt == 2:
            ln, i = read_varint(buf, i, end)
            if i + ln > end:
                raise ValueError("estoura o fim")
            out.append((fn, "bytes", buf[i:i + ln]))
            i += ln
        elif wt == 0:
            v, i = read_varint(buf, i, end)
            out.append((fn, "varint", v))
        elif wt == 5:
            out.append((fn, "f32", buf[i:i + 4]))
            i += 4
        elif wt == 1:
            out.append((fn, "f64", buf[i:i + 8]))
            i += 8
        else:
            raise ValueError("wiretype %d" % wt)
    return out


def parse_descriptor(buf, i, end):
    """FileDescriptorProto/DescriptorProto: name=1, field=2, nested=3, enum=4, msg=5(?)."""
    fields = parse_fields(buf, i, end)
    return fields


TYPES = {1: "double", 2: "float", 3: "int64", 4: "uint64", 5: "int32", 6: "fixed64", 7: "fixed32",
         8: "bool", 9: "string", 10: "group", 11: "message", 12: "bytes", 13: "uint32",
         14: "enum", 15: "sfixed32", 16: "sfixed64", 17: "sint32", 18: "sint64"}
LABELS = {1: "opt", 2: "req", 3: "rep"}


def decode_message(fields, path, out, depth=0):
    for fn, kind, val in fields:
        if depth == 0 and fn == 1 and kind == "bytes":
            out.append(("NAME", val.decode("utf-8", "replace")))
        elif fn == 2 and kind == "bytes" and depth >= 1:  # field (FieldDescriptorProto)
            try:
                f = parse_fields(val, 0, len(val))
            except Exception:
                continue
            name = number = label = tname = typ = None
            for x, _, v in f:
                if x == 1 and isinstance(v, bytes):
                    name = v.decode("utf-8", "replace")
                elif x == 3 and isinstance(v, int):
                    number = v
                elif x == 4 and isinstance(v, int):
                    label = LABELS.get(v, v)
                elif x == 5 and isinstance(v, int):
                    typ = TYPES.get(v, v)
                elif x == 6 and isinstance(v, bytes):
                    tname = v.decode("utf-8", "replace")
            if name and number is not None:
                out.append(("FIELD", name, number, label, typ, tname))
        elif fn in (3, 4, 5, 6, 7) and kind == "bytes" and depth >= 1:
            # nested types: nested_type=3, enum_type=4, ext=... oneof=8 in DescriptorProto
            try:
                sub = parse_fields(val, 0, len(val))
                decode_message(sub, path, out, depth + 1)
            except Exception:
                pass


def dump_file(blob_fields, targets, results):
    msgs = []

    def walk(fields, prefix, depth):
        name = None
        for fn, kind, val in fields:
            if fn == 1 and kind == "bytes" and isinstance(val, bytes):
                name = val.decode("utf-8", "replace")
        full = prefix + "." + name if (prefix and name) else (name or prefix or "?")
        out = []
        try:
            decode_message(fields, full, out, depth if depth >= 1 else 1)
            if any(o[0] == "FIELD" for o in out):
                msgs.append((full, out))
        except Exception:
            pass
        # FileDescriptorProto: message_type=4; DescriptorProto: nested_type=3
        for fn, kind, val in fields:
            if fn in (4, 3) and kind == "bytes":
                try:
                    sub = parse_fields(val, 0, len(val))
                except Exception:
                    continue
                walk(sub, full, depth + 1)

    walk(blob_fields, "?", 0)
    for full, out in msgs:
        base = full.split(".")[-1]
        if targets.search(full) or targets.search(base):
            fields = [o for o in out if o[0] == "FIELD"]
            if fields:
                results.append((full, fields))


def main():
    path, pattern = sys.argv[1], sys.argv[2]
    targets = re.compile(pattern)
    data = open(path, "rb").read()
    results = []
    seen = set()
    for m in re.finditer(rb"[\w/]+\.proto", data):
        name = m.group(0)
        # name vem precedido de: 0x0A <varint len>; a tag fica 1..3 bytes antes
        start = None
        for back in (1, 2, 3):
            p = m.start() - back
            if p >= 0 and data[p] == 0x0A:
                start = p
                break
        if start is None:
            continue
        # sem prefixo de comprimento: parseia para frente até o primeiro lixo
        i = start
        fields = []
        try:
            while True:
                tag, j = read_varint(data, i, len(data))
                fn, wt = tag >> 3, tag & 7
                if fn == 0 or fn > 16:
                    break
                if wt == 2:
                    ln, j = read_varint(data, j, len(data))
                    if j + ln > len(data):
                        break
                    fields.append((fn, "bytes", data[j:j + ln]))
                    j += ln
                elif wt == 0:
                    v, j = read_varint(data, j, len(data))
                    fields.append((fn, "varint", v))
                elif wt == 5:
                    fields.append((fn, "f32", data[j:j + 4]))
                    j += 4
                elif wt == 1:
                    fields.append((fn, "f64", data[j:j + 8]))
                    j += 8
                else:
                    break
                i = j
        except Exception:
            pass
        if len(fields) < 2:
            continue
        pkg = None
        for fn, kind, val in fields:
            if fn == 2 and isinstance(val, bytes):
                pkg = val.decode("utf-8", "replace")
        key = (pkg, name.decode())
        if key in seen:
            continue
        seen.add(key)
        try:
            dump_file(fields, targets, results)
        except Exception:
            pass
    for full, fields in results:
        print("== %s (%d campos)" % (full, len(fields)))
        for _, name, number, label, typ, tname in sorted(fields, key=lambda f: f[2]):
            t = tname if tname else (typ or "?")
            print("  %3d  %-4s %-32s %s" % (number, label or "", name, t))


if __name__ == "__main__":
    main()
