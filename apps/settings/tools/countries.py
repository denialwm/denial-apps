#!/usr/bin/env python3
"""Writes src/countries.rs: the countries Settings offers for the Wi-Fi
rules, those the wireless regulatory database knows, with their names in
English and Simplified Chinese from iso-codes.

Usage: tools/countries.py [REGULATORY_DB] [ISO_CODES_JSON] [ZH_MO]
(defaults: this machine's wireless-regdb and iso-codes)
"""
import gettext
import json
import struct
import sys
from pathlib import Path

regdb = Path(sys.argv[1] if len(sys.argv) > 1 else '/usr/lib/firmware/regulatory.db')
iso = Path(sys.argv[2] if len(sys.argv) > 2 else '/usr/share/iso-codes/json/iso_3166-1.json')
zh_mo = Path(sys.argv[3] if len(sys.argv) > 3 else
             '/usr/share/locale/zh_CN/LC_MESSAGES/iso_3166-1.mo')

# regulatory.db: "RGDB", a version, then {alpha2[2], be16 collection} per
# country until a collection of 0.
data = regdb.read_bytes()
assert data[:4] == b'RGDB', f'{regdb} is not a regulatory database'
known = set()
at = 8
while True:
    alpha2, collection = struct.unpack_from('>2sH', data, at)
    if collection == 0:
        break
    code = alpha2.decode()
    if code != '00':
        known.add(code)
    at += 4

zh = gettext.GNUTranslations(zh_mo.open('rb'))
rows = []
for entry in json.loads(iso.read_text())['3166-1']:
    code = entry['alpha_2']
    if code not in known:
        continue
    english = entry.get('common_name', entry['name'])
    chinese = zh.gettext(english)
    if chinese == english:
        chinese = zh.gettext(entry['name'])
    rows.append((english, code, chinese))
rows.sort()
# Codes ISO 3166-1 no longer has, such as AN, stay out.
left_out = sorted(known - {code for _, code, _ in rows})


def quoted(text):
    return json.dumps(text, ensure_ascii=False)


out = Path(__file__).resolve().parent.parent / 'src/countries.rs'
with out.open('w') as f:
    f.write('//! The countries the Wi-Fi rules can be set for: those the wireless\n'
            '//! regulatory database knows, by name. Written by tools/countries.py\n'
            '//! from wireless-regdb and iso-codes; do not edit.\n\n'
            '/// (ISO 3166-1 code, English name, Simplified Chinese name)\n'
            f'pub const COUNTRIES: [(&str, &str, &str); {len(rows)}] = [\n')
    for english, code, chinese in rows:
        f.write(f'    ({quoted(code)}, {quoted(english)}, {quoted(chinese)}),\n')
    f.write('];\n')
print(f'{out}: {len(rows)} countries; left out {left_out}')
