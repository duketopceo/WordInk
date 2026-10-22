#!/usr/bin/env python3
"""Post-link hook for xtool's ld64.lld wrapper.

xtool's app-extension trampoline executables link with only __stubs/__got —
no __text section. The bundled zsign computes load-command free space from
__text.offset and refuses to sign when the section is absent. Injecting an
empty LC_CODE_SIGNATURE command (dataoff=filesize) makes zsign take its
existing-command path and write the real signature at end of file.
"""
import os
import struct
import sys

MH_MAGIC_64 = 0xFEEDFACF
MH_EXECUTE = 0x2
LC_SEGMENT_64 = 0x19
LC_CODE_SIGNATURE = 0x1D


def find_output(argv):
    # Handles both `-o <path>` and `-o<path>`; last flag wins, as ld64 treats it.
    out = None
    for i, a in enumerate(argv):
        if a == "-o" and i + 1 < len(argv):
            out = argv[i + 1]
        elif a.startswith("-o") and len(a) > 2:
            out = a[2:]
    return out


def main():
    path = find_output(sys.argv[1:])
    if not path or not os.path.isfile(path):
        return
    with open(path, "r+b") as f:
        d = bytearray(f.read())
        if len(d) < 32 or struct.unpack("<I", d[:4])[0] != MH_MAGIC_64:
            return
        filetype = struct.unpack("<I", d[12:16])[0]
        if filetype != MH_EXECUTE:
            return
        ncmds, sizeofcmds = struct.unpack("<II", d[16:24])
        off = 32
        has_text = False
        has_sig = False
        for _ in range(ncmds):
            cmd, cmdsize = struct.unpack("<II", d[off:off + 8])
            if cmd == LC_CODE_SIGNATURE:
                has_sig = True
            if cmd == LC_SEGMENT_64:
                if d[off + 8:off + 24] == b"__TEXT" + b"\0" * 10:
                    nsects = struct.unpack("<I", d[off + 64:off + 68])[0]
                    s = off + 72
                    for _ in range(nsects):
                        if d[s:s + 16] == b"__text" + b"\0" * 10:
                            has_text = True
                        s += 80
            off += cmdsize
        if has_text or has_sig:
            return
        end = 32 + sizeofcmds
        if len(d) < end + 16:
            return
        struct.pack_into("<IIII", d, end, LC_CODE_SIGNATURE, 16, len(d), 0)
        struct.pack_into("<II", d, 16, ncmds + 1, sizeofcmds + 16)
        f.seek(0)
        f.write(d)


main()
