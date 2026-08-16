# -*- coding: utf-8 -*-
import re, sys, io, json, os
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8', errors='replace')

SD = os.path.dirname(os.path.abspath(__file__))
STOCK = os.path.join(SD, 'stock-dictionary')
DOBIE = r"C:\Program Files (x86)\Steam\steamapps\common\Kerbal Space Program\GameData\Squad\Localization\dictionary.cfg"

KV = re.compile(r'^\s*(#autoLOC_\S+)\s*=\s*(.*?)\s*$')

def parse(path):
    d = {}
    with open(path, encoding='utf-8-sig', errors='replace') as f:
        for line in f:
            m = KV.match(line)
            if m:
                d[m.group(1)] = m.group(2)
    return d

def node_names(path):
    s = open(path, encoding='utf-8-sig', errors='replace').read(4000)
    return re.findall(r'^\s*([a-z]{2}(?:-[a-z]{2})?)\s*$', s, re.M)

dob = parse(DOBIE)
en  = parse(os.path.join(STOCK, 'en-us.cfg'))
ja  = parse(os.path.join(STOCK, 'ja.cfg'))
zh  = parse(os.path.join(STOCK, 'zh-cn.cfg'))

print('nodes:', {p: node_names(os.path.join(STOCK, p + '.cfg')) for p in ('en-us','ja','zh-cn')})
print('counts: dobie=%d en=%d ja=%d zh=%d' % (len(dob), len(en), len(ja), len(zh)))

def ascii_only(v):
    return v != '' and all(0x20 <= ord(c) <= 0x7e for c in v)

def norm(v):
    return re.sub(r'\s+', ' ', v).strip().lower()

# a) Dobie keys kept in English (pure ASCII value)
K_dobie = {k for k, v in dob.items() if ascii_only(v)}
# restrict to keys present in stock en-us AND in ja/zh (comparable universe)
common = set(dob) & set(en) & set(ja) & set(zh)
print('common keys (dobie n en n ja n zh): %d' % len(common))
K = K_dobie & common
print('|K_dobie| all = %d ; within common = %d' % (len(K_dobie), len(K)))

def kept_en(lang, k):
    """lang value counts as 'kept English' if pure ASCII and equal/similar to en-us."""
    v = lang.get(k, '')
    if not ascii_only(v):
        return False
    e = en.get(k, '')
    return norm(v) == norm(e)

def ascii_kept_loose(lang, k):
    return ascii_only(lang.get(k, ''))

ja_keep = {k for k in common if kept_en(ja, k)}
zh_keep = {k for k in common if kept_en(zh, k)}
ja_keep_loose = {k for k in common if ascii_kept_loose(ja, k)}
zh_keep_loose = {k for k in common if ascii_kept_loose(zh, k)}

def pct(a, b):
    return 100.0 * a / b if b else 0.0

print()
print('=== strict (ASCII and == en-us) ===')
print('ja kept-English total: %d (%.1f%% of common)' % (len(ja_keep), pct(len(ja_keep), len(common))))
print('zh kept-English total: %d (%.1f%%)' % (len(zh_keep), pct(len(zh_keep), len(common))))
print('ja&zh: %d ; ja|zh: %d' % (len(ja_keep & zh_keep), len(ja_keep | zh_keep)))
print()
print('=== loose (ASCII only) ===')
print('ja: %d  zh: %d  ja&zh: %d  ja|zh: %d' % (len(ja_keep_loose), len(zh_keep_loose),
      len(ja_keep_loose & zh_keep_loose), len(ja_keep_loose | zh_keep_loose)))

print()
print('=== K_dobie coverage (strict) ===')
for name, S in (('ja', ja_keep), ('zh-cn', zh_keep), ('ja AND zh', ja_keep & zh_keep), ('ja OR zh', ja_keep | zh_keep)):
    inter = K & S
    print('%-10s kept too: %5d / %5d = %.1f%%' % (name, len(inter), len(K), pct(len(inter), len(K))))

print()
print('=== K_dobie coverage (loose ASCII) ===')
for name, S in (('ja', ja_keep_loose), ('zh-cn', zh_keep_loose),
                ('ja AND zh', ja_keep_loose & zh_keep_loose), ('ja OR zh', ja_keep_loose | zh_keep_loose)):
    inter = K & S
    print('%-10s kept too: %5d / %5d = %.1f%%' % (name, len(inter), len(K), pct(len(inter), len(K))))

print()
print('=== reverse: lang kept English but Dobie translated ===')
K_trans = common - K   # Dobie translated (non-ASCII value)
for name, S in (('ja', ja_keep), ('zh-cn', zh_keep), ('ja AND zh', ja_keep & zh_keep), ('ja OR zh', ja_keep | zh_keep)):
    r = S & K_trans
    print('%-10s kept but Dobie translated: %5d  (precision of mask = %.1f%%)' %
          (name, len(r), pct(len(S & K), len(S)) if S else 0))

def sample(keys, n, label):
    print()
    print('--- %s (%d total, showing %d) ---' % (label, len(keys), min(n, len(keys))))
    for k in sorted(keys)[:n]:
        print('%s\n   en: %s\n   ja: %s\n   zh: %s\n   ko: %s' %
              (k, en.get(k, '')[:110], ja.get(k, '')[:110], zh.get(k, '')[:110], dob.get(k, '')[:110]))

sample(K - (ja_keep | zh_keep), 20, 'Dobie kept EN but BOTH ja & zh translated')
sample((ja_keep & zh_keep) & K_trans, 20, 'ja & zh kept EN but Dobie translated')

# dump mask sets
out = {
    'K_dobie_common': sorted(K),
    'ja_keep': sorted(ja_keep),
    'zh_keep': sorted(zh_keep),
    'ja_and_zh': sorted(ja_keep & zh_keep),
    'ja_or_zh': sorted(ja_keep | zh_keep),
}
json.dump(out, open(os.path.join(SD, 'mask_sets.json'), 'w', encoding='utf-8'), ensure_ascii=False)
print('\nwrote mask_sets.json')
