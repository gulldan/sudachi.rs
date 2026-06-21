#!/usr/bin/env python3
# Golden benchmark: Sudachi (SudachiDict) + Vaporetto vs UD_Japanese-GSD (SUW gold).
# Metric: token-span F1 (KyTea/Kudo lineage). seg = span match; top = span + POS[0].
import os
from sudachipy import Dictionary, SplitMode

CONLLU = "/tmp/gold/test.conllu"
GSD_TEXT = "/tmp/gold/gsd_text.txt"
VAPO_OUT = "/tmp/gold/vapo_out.txt"
CONFIG = "/Users/mmk/github/sudachi.rs/target/bench-lookup/full-v1/sudachi.json"

def parse_conllu(path):
    sents, text, toks = [], None, []
    for line in open(path, encoding="utf-8"):
        line = line.rstrip("\n")
        if line.startswith("# text ="):
            text = line[len("# text ="):].strip()
        elif line == "":
            if text is not None and toks:
                sents.append((text, toks))
            text, toks = None, []
        elif line[:1].isdigit():
            c = line.split("\t")
            if "-" in c[0] or "." in c[0]:
                continue
            toks.append((c[1], c[4]))  # FORM, XPOS
    if text is not None and toks:
        sents.append((text, toks))
    return sents

def gold_spans(text, toks):
    spans, i, n, miss = [], 0, len(text), 0
    for form, xpos in toks:
        while i < n and text[i].isspace():
            i += 1
        if text[i:i+len(form)] == form:
            spans.append((i, i+len(form), xpos)); i += len(form)
        else:
            j = text.find(form, i)
            if j != -1:
                spans.append((j, j+len(form), xpos)); i = j+len(form)
            else:
                spans.append((i, i+len(form), xpos)); i += len(form); miss += 1
    return spans, miss

def prf(c, sy, go):
    p = c/sy if sy else 0.0
    r = c/go if go else 0.0
    f = 2*p*r/(p+r) if (p+r) else 0.0
    return p*100, r*100, f*100

def score_external(sents, path, label):
    lines = open(path, encoding="utf-8").read().split("\n")
    seg_c = seg_sys = seg_gold = 0
    for (text, toks), outline in zip(sents, lines):
        g, _ = gold_spans(text, toks)
        gold_seg = {(s, e) for (s, e, p) in g}
        seg_gold += len(g)
        pos = 0
        for su in outline.split("\t"):
            if not su:
                continue
            sp = (pos, pos + len(su)); pos += len(su)
            seg_sys += 1
            if sp in gold_seg:
                seg_c += 1
    p, r, f = prf(seg_c, seg_sys, seg_gold)
    print(f"{label}: sys={seg_sys} gold={seg_gold} | SEG  P={p:5.2f} R={r:5.2f} F={f:5.2f}")

def main():
    sents = parse_conllu(CONLLU)
    with open(GSD_TEXT, "w", encoding="utf-8") as fh:
        fh.write("\n".join(t for t, _ in sents))
    tok = Dictionary(config_path=CONFIG).create()
    total_miss = sum(gold_spans(t, ts)[1] for t, ts in sents)
    print(f"# corpus: UD_Japanese-GSD test | sentences={len(sents)} | gold_align_miss={total_miss}")
    for mname, mode in [("A", SplitMode.A), ("B", SplitMode.B), ("C", SplitMode.C)]:
        seg_c = seg_sys = seg_gold = pos_c = 0
        for text, toks in sents:
            g, _ = gold_spans(text, toks)
            gold_seg = {(s, e) for (s, e, p) in g}
            gold_pos = {(s, e, p.split("-")[0]) for (s, e, p) in g}
            seg_gold += len(g)
            for m in tok.tokenize(text, mode):
                s, e = m.begin(), m.end()
                seg_sys += 1
                if (s, e) in gold_seg:
                    seg_c += 1
                if (s, e, m.part_of_speech()[0]) in gold_pos:
                    pos_c += 1
        sp, sr, sf = prf(seg_c, seg_sys, seg_gold)
        pp, pr, pf = prf(pos_c, seg_sys, seg_gold)
        print(f"Sudachi-{mname}: sys={seg_sys:6d} gold={seg_gold} | "
              f"SEG  P={sp:5.2f} R={sr:5.2f} F={sf:5.2f} | POS-top F={pf:5.2f}")
    if os.path.exists(VAPO_OUT):
        score_external(sents, VAPO_OUT, "Vaporetto-SUW")

main()
