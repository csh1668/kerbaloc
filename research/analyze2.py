# -*- coding: utf-8 -*-
import re, sys, io, os
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8', errors='replace')
SD = os.path.dirname(os.path.abspath(__file__))
STOCK = os.path.join(SD, 'stock-dictionary')
LEGACY = r"C:\Program Files (x86)\Steam\steamapps\common\Kerbal Space Program\GameData\Squad\Localization\dictionary.cfg"
KV = re.compile(r'^\s*(#autoLOC_\S+)\s*=\s*(.*?)\s*$')
def parse(p):
    d = {}
    for line in open(p, encoding='utf-8-sig', errors='replace'):
        m = KV.match(line)
        if m: d[m.group(1)] = m.group(2)
    return d
dob = parse(LEGACY); en = parse(STOCK+'/en-us.cfg'); ja = parse(STOCK+'/ja.cfg'); zh = parse(STOCK+'/zh-cn.cfg')
common = set(dob)&set(en)&set(ja)&set(zh)
A = lambda v: v != '' and all(0x20 <= ord(c) <= 0x7e for c in v)
N = lambda v: re.sub(r'\s+',' ',v).strip().lower()
K = {k for k,v in dob.items() if A(v)} & common
print('missing in ja:', sorted(set(en)-set(ja)), ' missing in zh:', sorted(set(en)-set(zh)))
diff = [k for k in K if N(dob[k]) != N(en[k])]
print('K_legacy where ko value != en-us value:', len(diff))
for k in sorted(diff)[:8]: print('  ',k,'| en:',en[k][:70],'| ko:',dob[k][:70])
zh_keep = {k for k in common if A(zh[k]) and N(zh[k])==N(en[k])}
ja_keep = {k for k in common if A(ja[k]) and N(ja[k])==N(en[k])}
T = common - K
print()
print('--- zh kept EN but legacy translated: %d (sample, zh-only) ---' % len(zh_keep&T))
for k in sorted((zh_keep-ja_keep)&T)[:20]:
    print(' ',k,'| en:',en[k][:55],'| ja:',ja[k][:35],'| ko:',dob[k][:35])
print()
print('--- ja kept EN but legacy translated: %d (all) ---' % len(ja_keep&T))
for k in sorted(ja_keep&T): print(' ',k,'| en:',en[k][:55],'| ko:',dob[k][:45])
# key-range breakdown of ja_keep
print()
from collections import Counter
c = Counter(k.split('_')[1][:3] for k in ja_keep)
print('ja_keep key prefixes:', c.most_common(12))
c2 = Counter(k.split('_')[1][:3] for k in K)
print('K_legacy key prefixes:', c2.most_common(12))
