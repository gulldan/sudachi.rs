# sudachi.rs — комплексное исследование оптимизации и оценки (issue #117 и далее)

> Сводный отчёт уровня публикации: методология бенчмарка (скорость + качество),
> все проверенные гипотезы с числами (положительные и **отрицательные**), и выводы.
> Провенанс каждого числа помечен: **[meas]** — измерено нами на M4 Max в этом
> исследовании; **[doc]** — из `docs/performance-investigation.md` (прежние замеры);
> **[lit]** — из рецензируемой литературы (см. §8); **[proj]** — проекция по Амдалю.

## 1. Аннотация

Мы исследовали, можно ли ускорить горячий путь sudachi.rs (lattice-Viterbi
японский морфологический анализатор: common-prefix search по double-array trie +
Viterbi по матрице связности) **без потери и с потерей** совместимости формата, и
построили **золотой бенчмарк** (скорость + точность), которого у Sudachi не было.
46 пронумерованных гипотез (§5.0), четыре раунда литературного research'а,
ассемблерный разбор и прототипы сошлись на трёх выводах.

**(1) Byte-identical путь имеет потолок.** Зашипованы prefetch (+16.7%) и varint
(+2.7%); это исследование добавило два новых вывод-сохраняющих переноса —
gate `normalized_form`-декода в JoinNumeric (#39) и early-out переменной секции
WordInfoParser (#40), вместе **~9%** на реалистичном subset (do_tokenize 320→294
ns/char). Но **3× byte-identical недостижимо**: профиль упирается в `Lattice::insert`
(матрица связности ~25%, исторически непробиваемо) + материализацию WordInfo на узел
(~17%, из неё **H7/#52 отыграл +20–21%** через shared `Arc`-кэш); потолок полного-вывода
≈ **1.35×** (с H7).

**(2) Парадигма pointwise (Vaporetto) даёт иксы — но «делая меньше».** На matched
golden-бенчмарке против оптимизированного Sudachi (293 ns/char): seg+POS **8.1×**.
Но как **полная замена** (с `normalized_form`, который пайплайн восстанавливает
dict-lookup'ом) — **2.3× без кэша → 3.3× с decode-кэшем (#2) → до 6.5× с
surface-кэшем** (§5.9, зависит от повторяемости поверхностей), асимптота → 8×
(= сам сегментатор). decode-кэш — **первый рычаг, общий решётке и pointwise**
(~40 ns/char), его prod-версия без leak + thread-safe выживает (6.1×, §5.9-Б).

**(3) Цена парадигмы — меньше, чем казалось.** POS-gap **не −3.96, а −1.19**:
бо́льшая часть была артефактом MODEL-предсказанного POS; беря **dict-POS** (словарь
и так на руках), gap сжимается до −1.19, остаток — пропагация сегментации
(§5.10). Итоговая цена замены: SEG **−0.92**, POS **−1.19**, жёстко — **нет A/B/C**
+ переобучка. **Синтез:** 3× с сохранением ПОЛНОГО вывода Sudachi недостижимо ни
byte-identical (~1.35× с H7), ни парадигмой при паритете фич без кэша (2.3×); все
большие иксы = **редукция задачи** (drop фич / A-B-C / решётки) ± кэш повторов.

## 2. Среда и данные

| параметр | значение |
|---|---|
| CPU | Apple M4 Max (12 P + 4 E ядра), line 128 B, L1d 128 KB/P, **L2 16 MB / 6 P-ядер**, page 16 KB, 48 GB |
| toolchain | rustc/cargo 1.96.0, `--release` |
| словарь | SudachiDict full (small+core+notcore) = **2.59M ключей**; trie ≈ 72 MB; матрица связности 5981² × i16 ≈ 71 MB |
| корпус (скорость) | kyoto-leads: 16 051 предложений / **461 815 codepoints** |
| корпус (качество) | **UD_Japanese-GSD** test: 543 предложения / **13 034 SUW-токена** (CC BY-SA 4.0) |

## 3. Методология бенчмарка

### 3.1 Скорость, end-to-end
`examples/tokenize_pipeline_bench`: `reset → do_tokenize → collect_results`
(`InfoSubset::all`), scalar и pipelined пути **interleaved** на каждом trial,
median из N (N=31) + coefficient of variation (CV); выигрыш засчитывается выше
шумового порога. Метрика: **ns/char** и sentences/s. ⚠️ Run-to-run шум на ноутбуке
реален (CV ~3–4%); надёжны изолированные и относительные числа.

### 3.2 Изолированный lookup
`examples/dictionary_matcher_report` (feature `matcher-comparison`): строит
yada / crawdad / daachorse / fst / rsmarisa над **реальными 2.59M ключами**,
гоняет common-prefix с реальных позиций корпуса, метрика **ns/start** + heap +
serialized bytes; все методы проверяются на **идентичность матчей** (exact).

### 3.3 Профилирование
`sample`-профиль pipelined-токенизатора [doc] + **счётчик зависимых загрузок**
(атомик в `step_once`, 1 проход корпуса) [meas].

### 3.4 Качество (золотой бенчмарк) — net-new
Seminal-статья Sudachi (Takaoka et al., LREC 2018) **не приводит ни одного числа
точности** [lit] → бенчмарк построен с нуля.
- **Корпус:** UD_Japanese-GSD (word-units = slightly-changed **SUW**, UniDic;
  CoNLL-U c FORM/UPOS/**XPOS=UniDic SUW POS**), CC BY-SA 4.0.
- **Метрика:** token-span F-measure (Nagata 1994 → Kudo 2004 → KyTea 2011 →
  Vaporetto 2024): токен верен, если совпали **И границы (span), И POS-тег**.
  P=#correct/#system, R=#correct/#gold, F=2PR/(P+R). Глубины: **seg** (только
  границы), **top** (границы + POS[0]). [lit]
- **Совпадение схем (критично для честности):** опубликованного метода честно
  сравнивать РАЗНЫЕ гранулярности на одном gold НЕТ [lit]; принцип — **схема под
  gold**: Sudachi mode A (≈ UniDic SUW) и Vaporetto (модель bccwj-suw) скорятся
  нативно против GSD-SUW. Token-span сравнение разных гранулярностей (Vaporetto-SUW
  vs Sudachi-C) **не defensible** — для таких случаев только boundary-level seg-F1.
- **Scorer:** `bench/quality/score.py` (наш), `gold_align_miss=0` (валидирован).

## 4. Факты горячего пути

| факт | значение | провенанс |
|---|---|---|
| профиль: connection-matrix / Viterbi | **22.8%** | [doc] |
| профиль: trie walk (common-prefix) | **16.9%** | [doc] |
| профиль: alloc / WordInfo+UTF16→8 / varint / OOV | 9.6 / 9.6 / 9.5 / 7.5% | [doc] |
| зависимых загрузок trie | **7.36 / codepoint** (2.45 / byte), ~3.4M/корпус | [meas] |
| средняя длина walk | ~2.45 codepoint (~7.4 байта) до dead-end | [meas] |
| asm walk (M4 Max) | 1 завис. загрузка/байт, bounds-checks элиминированы, **load-latency-bound** | [meas] |
| матрица | volume-bound, НЕ miss-bound (prefetch матрицы вредит) | [doc/meas] |

## 5. Эксперименты — полная таблица (все гипотезы)

### 5.0 Сводная нумерованная таблица (все гипотезы по порядку)

| # | Гипотеза | Число на бенчмарке | Δ | Рез. |
|---:|---|---|---|:--:|
| 1 | baseline 0.7 (scalar) | 457.6 ns/char · 75 959 sent/s | — | 📊 база |
| 2 | prefetch K=4 (PR #348, L1 #117) | 391.9 ns/char · 88 682 | +5…17% (median +9%) | ✅ |
| 3 | varint direct decoder | — | +2.7% | ✅ |
| 4 | daac (charwise AC, in-situ) | ~387 ns/char · 90 034 | ~0% | ➖ +41% размер |
| 5 | multithread ×16 | 802 669 sent/s | ~8.9× | ✅✅ |
| 6 | Vaporetto seg | 41.6 ns/char · 835 768 | 9.4× vs prefetch | ✅ парадигма |
| 7 | Vaporetto seg+POS | 54.9 ns/char · 632 813 | 7.1× vs prefetch | ✅ парадигма |
| 8 | yada trie (isolated) | 35.25 ns/start | 1.00× | 📊 база |
| 9 | yada + prefetch K=4 | 29.4 ns/start | 1.20–1.25× | ✅ |
| 10 | yada + prefetch K=8/12/16 | — | ≤1.0× | ❌ |
| 11 | crawdad trie | 25.4 ns/start | 1.37–1.39× · 0.76× mem | ✅ рычаг |
| 12 | crawdad MP-trie | 27.6 ns/start | 1.28–1.37× · 0.67× mem | ➖ память↓ |
| 13 | daachorse charwise | 25.4 ns/start | 1.29–1.39× · 1.86× mem | ➖ память↑ |
| 14 | daachorse bytewise | 42.2 ns/start | 0.84–0.89× | ❌ |
| 15 | MARISA (LOUDS succinct) | 303 ns/start | 0.12× (8× медл.) · 0.16× mem | ❌ |
| 16 | FST (Burntsushi) | 1237 ns/start | 0.03× (35× медл.) | ❌ |
| 17 | prefetch hint L2-keep vs L1 | 1.201 vs 1.245 | −3.5% отн. | ❌ L1 верх |
| 18 | Sudachi A — качество | SEG-F1 97.99 · POS-top 97.46 | — | 📊 база кач-ва |
| 19 | Sudachi B — качество | SEG-F1 95.06 · POS 94.58 | −2.93 | ➖ гранул. |
| 20 | Sudachi C — качество | SEG-F1 92.35 · POS 91.88 | −5.64 | ➖ гранул. |
| 21 | Vaporetto — качество | SEG-F1 97.07 · POS-top 93.50 | −0.92 SEG / −3.96 POS vs A | ⚖️ цена парадигмы |
| 22 | SoA `connect_node` | −1…4% e2e (exact) | отриц. | ❌ |
| 23 | conn-ID freq remap (Vibrato) | 0.99× + сломал 92/16051 | ~0 + поломка | ❌ |
| 24 | matrix software prefetch | 181→202 мс | −12% | ❌ |
| 25 | matrix row-hoist | — | no-op | ❌ |
| 26 | int8-квантизация матрицы | lossy (ломает вывод) | — | ❌ |
| 27 | vEB / cache-oblivious релейаут | теор. медленнее BFS | — | ❌ |
| 28 | build-time node relayout | 14× build / ≤1.10× lookup | — | ❌ |
| 29 | freq-weighted layout (MinWEP) | закрыт (yada сорт-ключи) | — | ❌ |
| 30 | path-compression MP (японский) | 11→21 нс/start | медленнее | ❌ |
| 31 | k-byte stride DA | память 256^k | — | ❌ |
| 32 | code-dividing / D2FA | +14…47% латентности | — | ❌ |
| 33 | Cuckoo Trie / MLP front-end | = наш prefetch, хуже CJK | — | ❌ |
| 34 | SIMD / ART node-decode | N/A (XOR, нет скана) | — | ❌ |
| 35 | MeCab+UniDic (fugashi) — качество | SEG-F1 99.11 · POS-top 97.57 | +1.12 SEG / +0.11 POS vs Sudachi-A | 📊 UniDic-эталон |
| 36 | Jagger (KWDLC/JUMAN) — boundary+speed | SEG 81.37 (scheme-mismatch) · 272k sent/s [meas py] / >1M [lit C++] | n/a POS | ⚠️ boundary-only; очень быстр (~3.7× Sudachi даже через py-binding) |
| 37 | Vibrato-UniDic (Rust lattice) | SEG 97.64 · POS 97.03 · 112 ns/char · 226k sent/s | −0.35 SEG vs Sudachi-A; **~3.1× быстрее** (non-compact) | ✅ Rust-lattice быстрее Sudachi при ≈той же точности → speed-headroom |
| 38 | esupar (neural BERT, CPU) — speed | 30.6 sent/s · 831k ns/char · SEG 59.17 (LUW≠SUW) | **~2400× медленнее Sudachi, ~30000× Vaporetto** | ⚠️ neural: точность ценой катастрофической CPU-скорости; default-модель LUW (granularity-mismatch, не качество) |
| 39 | **JoinNumeric: gate `normalized_form`-декода** (перенос, реализует §5.5②③) | do_tokenize mode A 383→354 ns/char (median; min 360→341); mode C 357→337 (min) | **−5…8% do_tokenize** (~20–30 ns/char), pipelined тоже | ✅ **byte-identical** A/B/C + num-stress; **первый положительный format-preserving перенос** из §5.5 |
| 40 | **WordInfoParser: early-out переменной секции** (перенос; skip split/synonym/user-data когда subset их не просит) | mode C `min` 301→294 (−2.3% vs #39); `pos` 302→289 (−4.3% vs #39); срабатывает только без SPLIT в subset | **−2…4% do_tokenize** доп. (mode C / CLI-subset); mode A `min` ≈0 (контроль: SPLIT_A нужен → не срабатывает) | ✅ **byte-identical** (12 комбо: ±`-a` × A/B/C × GSD/num-stress); устранил `parse_u32/i32_array` из профиля |
| — | **методологическая правка бенчмарка** (не гипотеза) | `tokenize_pipeline_bench` гонял под `InfoSubset::all()` (дефолт `StatefulTokenizer`); прод. CLI ставит `POS_ID\|NORMALIZED_FORM` (+DICT/READING/SYNONYM под `-a`) | артефакт `all()` ≈ **+6%** (21 ns/char) к do_tokenize: лишний парс split/synonym массивов, которых прод не трогает | ⚠️ все прежние числа (#1–#38) измерены под `all()` → реалистичный baseline ниже; добавлен `SUDACHI_BENCH_SUBSET` |
| 41 | **Vaporetto matched vs оптимизированный Sudachi** (#39+#40), GSD | seg 27.3 / seg+POS 36.4 ns/char vs Sudachi e2e mode C **293.5** | **8.1× (seg+POS), 10.7× (seg)** | ✅ парадигма >3× для seg+POS — но см. #42 для полного паритета |
| 42 | **feature-recovery: Vaporetto+dict как ПОЛНАЯ замена** (seg+POS+`normalized_form`) | dict-lookup **89.3 ns/char** (143 ns/token, 99.1% hit) → Vaporetto-full ≈ **125 ns/char** | **~2.3× vs Sudachi** (не 8×!) | ⚠️ [meas] **главный честный итог**: 8× — только для seg+POS; для паритета вывода ~2.3× + POS −3.96 + нет A/B/C |
| 43 | **прототип #2: decode-cache** (per-thread WordId→&str для `normalized_form`, env `SUDACHI_NF_CACHE`) | Vaporetto feature-recovery 89.7→**49.8** (−44%); Sudachi e2e+NF-output 326.8→**285.1** (−13%) | оба пути −~40 ns/char (декод `from_utf16`); **полный паритет 2.6×→3.3×** | ✅ content-identical (totals совпали + CLI `-a` diff чист A/B/C); прототип leaks (prod = owned-кэш в словаре) |
| 44 | **word_id-кэш (Vaporetto-путь)** — кэш `word_id→normalized` ДО fetch'а, скип `get_word_info_subset`+декод на хите | feature-recovery 90.3→**18.1** ns/char; Vaporetto-full = 36.4+18.1 = **54.5** | **5.2× vs decode-cached Sudachi / 6.0× vs uncached** | ✅ [meas] асимметрия честна (Sudachi не скипнет WordInfo — нужна решётке) |
| 45 | **surface→norm кэш (Vaporetto-путь)** — ключ=строка поверхности, скип даже trie на хите | feature-recovery 90→**7.6** ns/char; Vaporetto-full = 36.4+7.6 = **44.0** | **6.5× vs cached / 7.4× vs uncached** | ✅ [meas] ~6.8× подтверждён; почти на асимптоте 8× (= bare seg+POS); пол Sudachi ~285 неизбежен |
| 46 | **многопоточность кэша** — global Mutex vs DashMap vs thread_local (T=1…12, M4 Max) | Mutex 0.3–0.4× (коллапс), DashMap 1.2–1.4×, **thread_local 8.6–9.5×** (агрегат Mchar/s) | per-thread масштабируется почти линейно; глобальный lock — convoy | ✅ [meas] **prod-рекомендация: thread_local** (не Mutex, не DashMap для крошечной критической секции) |
| 47 | **`connect_node`/матрица — НЕ DRAM-bound (опровержение)** [18-агентный микроарх-разбор: LRU-sim + nomat-probe + cycle-arith + disasm] | live working set **3.77 MiB** (30916 строк-кэш-линий, 0 capacity-misses @4MB и @16MB L2, 97.95% L2-хитов); матричная загрузка = **≤7.2% do_tokenize** (nomat: 6.67→6.19ms); skew: 1474/5981 conn-id, top-16 строк=47% | **0% byte-identical рычага** — ядро на полу; matrix L2-резидентна, throughput/issue-bound, не latency | ✅ [meas] **исправляет премиссу «матрица volume/DRAM-bound»**; объясняет провал #24 (prefetch на cache-resident = load-port pressure); batching MLP бьёт в non-problem |
| 48 | **РЕАЛЬНЫЙ fused Vaporetto+dict бинарь** (vaporetto+sudachi в одном крейте, LTO, Sentence-reuse) — **исправляет #41/#42/#45** | seg+POS материализован **~71** (не 36); full-parity+cache **~78 ns/char**; SEG 97.07/96.16, POS-hybrid 95.72/94.40, dict-first 70 (омограф-ловушка), normalized 100% | **~3.7× vs Sudachi** (НЕ 6.5×); −0.92 SEG / −1.74 POS | ✅ [meas] **главная поправка**: проекция 6.5× смешала профили + un-материализованный v64-seg; робастно 71–113 ns/char на GSD-test/dev/kyoto |
| 49 | **exact min-plus batching `connect_node` по `left_id` (H1)** — probe + byte-identical прототип | **probe:** work-cut −22% (H1, ratio 0.777) / −39% (H1+H2, 0.609); cross-validирует 1.5M lookups. **прототип:** byte-identical (A/B/C+numstress), но **−3.6%/−5.2%** (lean Vec-memo; −15% с HashMap) | алгоритмический выигрыш реален, но **во время не конвертируется** | ❌ [meas] **подтверждает §5.11**: connect-loop слишком дёшев (L2-резидент, issue-bound, 13-инстр) — per-node dedup-bookkeeping дороже сэкономленных итераций. H2/H3 — тот же overhead-профиль |
| 50 | **H4/H5 first-char/hot-prefix trie sidecar** — measured + reason-closed | probe: **avg 7.216 trie-steps/start** (GSD scalar); first-char ≈3 шага (~42% step-count), но это **самые горячие** (root, L1) шаги, и латентность walk'а уже скрыта K=4-prefetch (#117) | ~0.5–0.8% best-case − bookkeeping → H1-redux | ❌ [reason+meas] урок H1: бьёт по дешёвому (горячему) — пер-start table-lookup доминирует; **sidecar** на byte-indexed полу (§6, 4 раунда). Это закрывает sidecar, НЕ корпус-частотный prefetch-relayout двойного массива (идея #117) — тот не прототипирован, оценён лишь по литературе (N7/N8), остаётся открытым |
| 51 | **H6/H7 материализация WordInfo** — два probe (WI-cache + no-WI) | **WI-cache** (clone resolved) **~0%** (mode A −1.0% / C +1.1%); **no-WI** (skip parse+resolve, no clone) **+17.8% / +15.9%** | **H6 alone refuted (~0%)** — clone ≈ parse cost; **H7 ceiling ПОДТВЕРЖДЁН ~16–18%** | ⚠️ [meas] **самый крупный byte-identical рычаг**: parse+resolve = ~17% do_tokenize, но отыгрывается ТОЛЬКО borrowed/shared, не deep-clone-кэшем |
| 52 | **H7 РЕАЛИЗОВАН + доведён до prod — `WordInfo` = `Arc<WordInfoData>` + `Arc<StringsCache>` + bounded subset-keyed cache** | H7 (data only) +11/+12%; **H7b (+ Arc<StringsCache>, делим и декод строк): +20.6% / +21.1%** byte-identical; bounded 64k direct-mapped (~1 MB/поток), ключ=(word_id,subset), hit-rate 87.7% (~5438 distinct combo на GSD, нужно ≥8k слотов) | **✅✅ САМЫЙ КРУПНЫЙ byte-identical выигрыш** (≈ #2+#39-декод+материализация разом) | ✅ [meas] byte-identical A/B/C × GSD/num-stress. **H7b субсумирует #2** (StringsCache-sharing вместо leaked-&str). subset-key обязателен (иначе A/B ломались). Ловит ~весь потолок #51 + декод |
| 53 | **H8 deterministic islands / H9 teacher-student** — characterized | H8-lossy и H9 (learned) меняют вывод → вне Java-compat (как парадигма #48); H8-exact (certifying bounds) byte-identical, но атакует дешёвую решётку (урок H1) + сложен | low-priority/separate | ⚠️ H8-lossy/H9 — отдельный инструмент (не sudachi.rs); H9 — «почти статья», seed = fused-прототип §5.12; H8-exact — маловероятен (решётка дёшева) |

(Разбивка по подсистемам — в §5.1–5.4 ниже. Доп. движки на golden-бенчмарке — §5.3. Перенос #39 детально — §5.6.)

### 5.1 Скорость e2e (full dict, kyoto-leads)

| подход | ns/char | sent/s | Δ | вывод | формат | провенанс |
|---|---:|---:|---|---|---|---|
| baseline 0.7 (scalar) | 457.6 | 75 959 | — | идентичен | — | [meas] |
| **+ prefetch K=4 (PR #348)** | 391.9 | 88 682 | **+5…17%** (median doc +9%) | идентичен | без изм. | [meas/doc] |
| **+ varint decoder** | — | — | **+2.7%** | идентичен | без изм. | [doc] |
| daac (charwise AC) | ~387 | ~90 034 | ~0% | идентичен (260 114) | +41% размер | [meas] |
| **multithread ×16** | — | **802 669** | **~8.9×** | идентичен | без изм. | [doc] |
| **Vaporetto seg** | 41.6 | 835 768 | **9.4×** vs prefetch | другой | др. движок | [meas] |
| **Vaporetto seg+POS** | 54.9 | 632 813 | **7.1×** vs prefetch | другой | др. движок | [meas] |

### 5.2 Изолированный lookup (реальные 2.59M ключей, exact-матчи) [meas]

| структура | vs yada (ns/start) | память vs yada | вывод |
|---|---:|---:|---|
| yada (текущий) | 1.00× | 1.00× | база |
| **yada + prefetch K=4** | **1.20–1.25×** | 1.00× | зашиплено |
| yada + prefetch K=8/12/16 | ≤1.0× | 1.00× | хуже K=4 |
| **crawdad trie** | **1.37–1.39×** | **0.76×** | рычаг |
| crawdad MP-trie | 1.28–1.37× | **0.67×** | память↓, latency≈ |
| daachorse charwise | 1.29–1.39× | 1.86× | быстр., память↑ |
| daachorse bytewise | 0.84–0.89× | 2.98× | хуже |
| MARISA (LOUDS succinct) | **0.12×** (8× медл.) | **0.16×** | succinct: память↓, latency↓↓ |
| FST (Burntsushi) | **0.03×** (35× медл.) | 0.37× | непригоден |
| prefetch hint L1-keep vs L2-keep | **1.245 vs 1.201** | — | L1 правильный |
| AC scan ns/char (char daac / byte / crate) | 17.3 / 35.1 / 58.6 | — | движок Vaporetto |

### 5.3 Качество (UD_Japanese-GSD, span-F1, matched SUW) [meas]

| движок | SEG-F1 | POS-top-F1 | скорость | схема |
|---|---:|---:|---:|---|
| **Sudachi mode A** | **97.99** | **97.46** | 1× | SUW, lattice |
| Sudachi mode B | 95.06 | 94.58 | 1× | средние юниты |
| Sudachi mode C | 92.35 | 91.88 | 1× | NE-юниты |
| **Vaporetto-SUW** | **97.07** (−0.92) | **93.50** (−3.96) | **7–9×** | SUW, pointwise |
| **MeCab+UniDic** (fugashi) | **99.11** (+1.12) | **97.57** (+0.11) | CRF-lattice (~MeCab) | SUW/UniDic-эталон |
| **Vibrato-UniDic** (Rust lattice) | **97.64** (−0.35) | **97.03** (−0.43) | **112 ns/char · 226k sent/s (~3.1× Sudachi)** | SUW/UniDic |

Вывод по scheme-matched: MeCab+UniDic — потолок точности на GSD (UniDic-эталон, =Kudo'04
~99% news). Sudachi-A −1.12 SEG = измеренная цена ревизий UniDic в SudachiDict (POS почти
паритет). Vaporetto — самый быстрый, но и самый низкий по точности (особенно POS).
JUMAN-движки (Jagger/Juman++) — только boundary-level (иной стандарт сегментации), POS несравним.
Эмпирически: Jagger (KWDLC/JUMAN) на GSD-SUW даёт SEG-F1 **81.37** (11 848 токенов vs 13 034) —
но это мера РАСХОЖДЕНИЯ СХЕМ (JUMAN≠SUW), не качества; подтверждает принцип scheme-matching
числом. Ценность Jagger — скорость (>1M sent/s [lit]), не измеримая как качество на SUW-gold.
Аналогично esupar (neural BERT, default ja = LUW-модель): SEG-F1 **59.17** на GSD-SUW = LUW≠SUW
granularity-mismatch, не качество (8803 токена vs 13 034). Чистый вклад нейро — **скорость на CPU:
30.6 sent/s ([meas]) = ~2400× медленнее Sudachi, ~30000× медленнее Vaporetto** — точка спектра
«transformer: высокая точность ценой катастрофической CPU-скорости». UPOS≠XPOS-метрике, не сравнивался.

**Matched-corpus скорость (GSD, оба Rust/native, 543 предл., [meas]):** Sudachi ~348 ns/char
(mode C, full analysis, 73k sent/s) vs Vaporetto seg 27 ns/char (940k sent/s) = **~12.9×** на GSD
(seg+POS сузит до ~10×); подтверждает kyoto-leads 9.4× на втором корпусе. Caveat: Sudachi-full
vs Vaporetto-seg (seg-only-vs-full), классический pitfall — честнее ~10× при равном объёме.

Замечание по POS-top: часть разрыва Vaporetto — артефакт (модель не выдаёт тег для
пунктуации, считается POS-неверной); даже с поправкой POS-gap реален и больше
сегментационного. Формат тега Vaporetto = UniDic POS (совпадает с GSD XPOS, иногда
мельче: `動詞-一般` vs `動詞-一般-五段-サ行`) — top-level POS[0] сравним напрямую.

Эталонный потолок новостного домена (для рамки): Kudo et al. 2004 seg/top/all ≈
98.96/98.31/96.75 на Kyoto Corpus [lit]; GSD — веб-текст, поэтому 97.99 ожидаемо
ниже. 2.01% недобор Sudachi-A = измеренный **residual mismatch Sudachi-A vs GSD-SUW**.

### 5.4 ❌ Отрицательные результаты — «не ходить сюда» (с числами и причиной)

| # | гипотеза | результат | причина | провенанс |
|---|---|---|---|---|
| N1 | SoA `connect_node` (lattice) | **−1…4%** (exact) | короткие списки предшественников → 2 потока хуже 1 padded | [meas] |
| N2 | conn-ID freq remap (Vibrato) | **0.99× + сломал 92/16051** | матрица volume-bound; OOV берут conn-ID из runtime-конфига | [doc] |
| N3 | matrix software prefetch | **181→202 мс** | строка матрицы L2-резидентна → prefetch overhead | [doc] |
| N4 | matrix row-hoist | no-op | LLVM уже выносит инвариант | [doc] |
| N5 | int8-квантизация матрицы | lossy, не argmin-preserving | сломает вывод (как N2) | [lit] |
| N6 | vEB / cache-oblivious релейаут trie | теор. медленнее BFS | index-арифметика > промах; DA уже cache-conscious | [lit] |
| N7 | build-time node relayout | **14× build / ≤1.10×** | не окупается | [doc] |
| N8 | frequency-weighted layout (MinWEP) | закрыт | yada строит из сорт. ключей → не переупорядочить; relayout = N7 | [lit] |
| N9 | path-compression MP-trie (японский) | **медленнее (11→21 нс)** | короткие плотные ключи, мало неветвящихся хвостов | [lit] |
| N10 | k-byte stride DA | память **256^k** | для 3-байтных kanji невозможно | [lit] |
| N11 | code-dividing / D2FA | **+14…47% латентности** | обратная ось — больше загрузок | [lit] |
| N12 | prefetch hint L2-keep | 1.201 < L1 1.245 | L1-keep правильный | [meas] |
| N13 | prefetch lanes K≥8 | хуже K=4 | low-instruction walk, потолок MLP (ART 1.6–1.7×) | [meas/lit] |
| N14 | Cuckoo Trie / MLP front-end | = наш prefetch, хуже на CJK | глубокие общие kanji-префиксы | [lit] |
| N15 | SIMD/ART node-decode | N/A | DA считает ребёнка через XOR — нет скана кандидатов | [meas/lit] |
| N16 | FST / MARISA backend | 35× / 8× медленнее | не для common-prefix-from-boundary | [meas] |

### 5.5 Почему Vibrato ~3× быстрее Sudachi (измеренная декомпозиция) [meas]

Vibrato 112 ns/char vs Sudachi mode-A `do_tokenize`-only 382 (full) / 301 (без input+path плагинов).
- **`collect_results` ≈ бесплатно** (full 369 ≈ nocollect 380) → материализация WordInfo НЕ узкое место; lazy-WordInfo «на выходе» не поможет.
- **Режим A ≈ C** → split-режимы не виноваты.
- **`sample`-профиль `do_tokenize`:** доминируют **аллокации** (malloc/free/memmove/realloc — #1 self-time) и **декод WordInfo** (`parse_u32_array`, `String::from_utf16`, `get_word_info`); последнее тянут **path-rewrite плагины** (JoinNumeric/JoinKatakanaOov фетчат POS/normalized) даже в mode A.
- Плагинный слой = **~21%**, разложен [meas]: path-rewrite (JoinNumeric/JoinKatakanaOov, фетч WordInfo→from_utf16) **~14%** (382→330), input-norm **~8%** (330→301). Ядро без плагинов = 301 ns/char — всё ещё ~2.7× Vibrato (доминируют аллокации).
- Остаток 301 vs 112 = **~2.7×**. Профиль ядра (без плагинов) [meas]: #1 **`Lattice::insert`** (connect_node min-plus по матрице + push в 3 параллельных Vec); #2 **eager-фетч WordInfo в `do_tokenize`** (`get_word_info_subset`→`parse_u32/i32_array` — вызывается ДАЖЕ без плагинов и без collect; решётке для Viterbi нужны лишь word-params, не полный WordInfo → **потенциально откладываемо**, Vibrato его при tokenize не фетчит); #3 MeCabOov `provide_oov`. Vibrato: feature=lazy &str-срез, tighter connector.

**Формула:** 3.4× ≈ ~1.27× (плагины) × ~1.39× (bytewise→charwise trie) × ~1.9× (impl: аллокации/OOV/varint/lattice). Vibrato: feature = `&str`-срез (UTF-8, без аллокации и UTF-16-декода), нет плагинного слоя, reuse-буферы.

**Цели переноса (format-preserving, по профилю, ранжировано):** ① сократить аллокации (arena/reuse для WordInfo-строк и per-word Vec) — #1 по self-time; ② избегать `String::from_utf16` на каждый lookup (UTF-8-кэш/интернинг или узкий `InfoSubset`) — **реализовано в §5.6 (перенос #39)**; ③ облегчить eager-фетч WordInfo в path-rewrite (~21%) — **частично закрыто #39**; ④ charwise trie (×1.39 lookup, Амдаль-cap). Ни один не даёт 3× в одиночку — gap накопительный (Vibrato тюнингован сквозь).

### 5.6 Перенос #39 — gate `normalized_form`-декода в `JoinNumericPlugin` [meas]

**Находка (§5.5):** `JoinNumericPlugin::rewrite_gen` декодировал `normalized_form` (`String::from_utf16` + malloc) для **каждого** узла пути (`join_numeric.rs:97`), хотя строка `s` влияет на ветку только когда узел числовой **или** его нормальная форма — одиночный `","`/`"."` (UTF-16-длина 1). На неноминально-числовом тексте это сотни лишних декодов на предложение.

**Перенос (byte-identical gate):**
- Новый O(1)-аксессор `WordInfo::normalized_form_len` → `StringsCache::normalized_form_len` возвращает UTF-16-длину **без** декода: для self-referencing нормальной формы (общий случай) — `headword_strptr().length`; иначе один `get_word_info_subset(HEADWORD)` (парс заголовка, без `from_utf16`); для OOV — длина уже материализованной строки.
- Gate в `rewrite_gen`: декодируем `s` только если `ctypes ∈ {NUMERIC, KANJINUMERIC}` **или** `normalized_form_len == 1`; иначе `s = ""`.
- **Корректность:** `StringPointer.length` в UTF-16-кодпойнтах (`strings.rs:41`), поэтому `length != 1` ⟹ `s` не может быть ни `","`, ни `"."`, ни `len()==1` → все ветки, читающие `s`, идут идентично при `s == ""`. Доказуемо вывод-сохраняющий.

**Эмпирическая проверка byte-identity** (CLI `-a`, full plugins, `JoinNumeric enableNormalize:true`):

| корпус | mode A | mode B | mode C |
|---|---|---|---|
| UD-GSD (543 предл., 13–13.6k токенов) | ✅ identical | ✅ identical | ✅ identical |
| num-stress (запятые/точки/полноширинные/телефоны/%) | ✅ identical | ✅ identical | ✅ identical |

**Скорость** (`do_tokenize`, NOCOLLECT, GSD, 31 trials × 3 повтора, M4 Max, min — самый устойчивый):

| путь / mode | clean (ns/char) | gated (ns/char) | Δ |
|---|---:|---:|---|
| scalar, mode A (median) | ~383 | ~354 | **−7.6%** |
| scalar, mode A (min) | ~360 | ~341 | **−5.3%** |
| scalar, mode C (min) | ~338 | ~321 | **−5.0%** |
| pipelined+prefetch, mode A (min) | ~784 *(шум rep1 cv 12.8%)* | ~739 | **−5.7%** |

Перенос **ортогонален** trie-lookup (живёт в path-rewrite) → scalar и pipelined выигрывают одинаково. Это **первый положительный format-preserving перенос** из vibrato-декомпозиции §5.5: подтверждает, что плагинный слой (~14% path-rewrite) — настоящий и устранимый источник, причём без изменения вывода. Файлы: `strings_cache.rs`, `word_info/data.rs`, `plugin/path_rewrite/join_numeric.rs`.

### 5.7 Перенос #40 + методологическая правка subset + карта пути к 3× [meas]

**Методологическая правка (важно для всего исследования):** `tokenize_pipeline_bench` не вызывал `set_subset`, т.е. гонял под `InfoSubset::all()` (дефолт `StatefulTokenizer::new`). Продакшен так не делает: CLI `output.rs::subset()` запрашивает `POS_ID|NORMALIZED_FORM` (без `-a`) или `+DICT|READING|SYNONYM` (с `-a`); Python передаёт свой `fields`. Под `all()` каждый узел best-path лишне `parse_u32_array`-ит все split/word_structure/synonym массивы. Измеренный артефакт на GSD: `all()`→`min` ≈ **−6% / −21 ns/char** (mode C, best-min). Добавлен `SUDACHI_BENCH_SUBSET` (`all`/`min`/`pos`/`all_cli`); реалистичный baseline = `min`.

**Перенос #40 (byte-identical):** в `WordInfoParser::parse` — ранний выход, когда `flds` не пересекается с `{SPLIT_A,B,C, WORD_STRUCTURE, SYNONYM_GROUP_IDS, USER_DATA}`: пропускаем парс/скип всей переменной секции (хвост записи больше не читается; границы валидируются при загрузке). Срабатывает для CLI-дефолта и mode C. Byte-identical проверен на 12 комбинациях (±`-a` × A/B/C × GSD/num-stress).

**Кумулятивная траектория сессии (do_tokenize, GSD, best-min, реалистичный subset):**

| этап | mode C `min` | mode C `pos` | Δ vs base |
|---|---:|---:|---|
| base (этой сессии, реалистичный subset) | 320.2 | 310.4 | — |
| +#39 (gate normalized_form) | 301.0 | 302.0 | −6.0% / −2.7% |
| +#39+#40 (early-out var-section) | **294.0** | **288.8** | **−8.2% / −6.9%** |

Два byte-identical переноса = **~8–9%** на реалистичном пути. Текущий рубеж ≈ **290 ns/char** против Vibrato 112 → остаётся **~2.6×**.

**Карта оставшегося разрыва (профиль `do_tokenize` после #39+#40, mode C `min`, [meas]):**

| компонент | доля | природа / достижимость |
|---|---:|---|
| `Lattice::insert` (Viterbi min-plus + connection-matrix) | **~25%** | matrix-volume-bound; #22–26 уже показали отриц./lossy → byte-identical выигрыш маловероятен |
| `resolve_best_path` материализация WordInfo (owned per node, `resolve`/`memmove`/`free`) | **~20%** | Vibrato держит узлы лёгкими (borrowed `&str`); требует рефактора `ResultNode`/плагинов |
| input-text плагины (Default/IgnoreYomigana/ProlongedSound) | **~10%** | переписывают вход; output-sensitive |
| path-rewrite (JoinNumeric+JoinKatakanaOov), ~55% — `from_utf16` | **~9%** | #39 срезал контентные слова; одно-символьная пунктуация всё ещё декодит |
| OOV `provide_oov` (MeCabOov) | **~5%** | аллокации |

**Честный вывод по 3×:** ни один оставшийся **byte-identical drop-in** не даёт >~5% e2e. Разрыв до Vibrato структурный и накопительный: (i) charwise-trie (×1.39 *изолированно*, но trie ~17% e2e по Амдалю → ~5–6% e2e; **смена формата**); (ii) лёгкие/borrowed узлы решётки + отказ от per-node owned WordInfo (**рефактор архитектуры**, самый большой кусок — ~20% + аллокации); (iii) `Lattice::insert`/матрица (~25%, исторически непробиваемо без потери точности). 3× в Sudachi-архитектуре = компаунд всех трёх (как у Vibrato), а не один перенос. Парадигм-сдвиг (Vaporetto, #6–7) даёт 7–13× сразу, но меняет вывод (см. §5.3 цену точности).

### 5.8 Путь A (выбран): Vaporetto как замена Sudachi — matched бенчмарк [meas]

Решение развилки §7.5 — парадигм-сдвиг. Здесь — честное **matched** сравнение на GSD против **уже оптимизированного** Sudachi (#39+#40, реалистичный `min` subset), оба выдают surface+POS.

**Скорость (GSD, 21328 chars, M4 Max, best-min):**

| движок | ns/char | sent/s | vs Sudachi |
|---|---:|---:|---:|
| Vaporetto seg-only | **27.3** | 932k | **10.7×** |
| Vaporetto seg+POS (`fill_tags`) | **36.4** | 699k | **8.1×** |
| Sudachi e2e mode C (collect, min subset) | 293.5 | — | 1× |
| Sudachi e2e mode A | 331.0 | — | 0.9× |

**Вывод:** даже после того как byte-identical переносы довели Sudachi до реалистичного пола ~293 ns/char, парадигма **всё ещё ~8× быстрее**. 3× не просто достигается — превышается втрое. Это подтверждает §5.7: разрыв структурный, и быстрее всего его закрывает не оптимизация решётки, а отказ от неё.

**⚠️ Ключевая поправка — feature-recovery измерен [meas], не спроецирован.** Я ожидал +30–50 ns/char; реальность **существенно дороже**. Бенч `feature_lookup_bench` (exact-lookup + `normalized_form`-fetch на 13258 сегментах Vaporetto, словарь Sudachi, subset `NORMALIZED_FORM`, после #40):

| метрика | значение |
|---|---|
| feature-recovery | **89.3 ns/char** (median; min ~83), **143 ns/token** |
| dict-hit rate | **99.1%** (13140/13258 нашлись, 118 OOV) |

Декомпозиция [meas, best-min] — что именно стоит дорого:

| стадия | ns/char | добавляет |
|---|---:|---|
| exact-lookup только (trie common-prefix) | **12.95** | — |
| + fetch POS (`get_word_info_subset`) | 45.64 | +33 (парс fixed-данных) |
| + декод `normalized_form` (полный) | 88.82 | +43 (`from_utf16`+resolve ref) |

**Вывод декомпозиции:** trie-lookup дёшев (~13); дорог **WordInfo-fetch + `from_utf16`-декод (~76)** — это **тот же декод, что платит и Sudachi**, неизбежный для паритета `normalized_form` при ЛЮБОМ сегментаторе. Значит «лучший exact-match-API» не спасёт (≤13 экономии) — пол задаётся декодом. Это же объясняет, почему `normalized_form`-декод всплывал в #39: процессный кэш декода (Arc<str> per WordId, отложенный план #2) помог бы ОБОИМ путям (и Sudachi, и Vaporetto-replacement) на повторяющихся словах.

**Итог — Vaporetto КАК ЗАМЕНА Sudachi (полный паритет вывода seg+POS+normalized):**

| режим вывода | путь | ns/char | vs Sudachi 293 |
|---|---|---:|---:|
| seg only | модель | 27.3 | 10.7× |
| seg + POS | модель | 36.4 | 8.1× |
| **seg + POS + `normalized_form`** (паритет) | модель + dict-lookup | **36 + 89 ≈ 125** | **~2.3×** |

**Вывод (честно):** парадигменные 8× существуют **только если нормальные формы не нужны**. Как только мы восстанавливаем фичи, которые Sudachi даёт через решётку, dict-lookup стоит ~89 ns/char, и реальный speedup замены падает до **~2.3×** — *плюс* POS −3.96 и потеря A/B/C. Т.е. стоимость решётки частично **покупает** эти фичи; «pointwise в 8× быстрее» — для задачи сегментация+тег, не для полного морфоанализа.

**Прочая цена парадигмы:**
1. **POS-точность** [meas #21]: POS-top **93.50 vs 97.46** (−3.96). SEG-F1 97.07 vs 97.99 (−0.92).
2. **Мульти-гранулярность:** pointwise даёт ОДНУ сегментацию — нет A/B/C из одного прохода (жёсткое ограничение).
3. **Интеграция:** модель привязана к словарю/языку, нужна переобучка; ~58 MB.
4. (Для полного паритета добавится ещё input-нормализация ~30 ns/char, не учтена в 125 → реальная замена ближе к ~1.9× при равном входе.)

**Что осталось бы (путь A, глубже):** dict-lookup можно ускорить exact-match-API (без common-prefix-перебора) — потенциально <89; оценить POS-gap на дообученной модели. Harness: `/tmp/vapo64` (`v64`), `feature_lookup_bench`, scorer `/tmp/gold/score.py`.

### 5.9 Прототип #2 — process-wide decode-cache на обоих путях [meas]

§5.8 показал: пол feature-recovery — это **`from_utf16`-декод `normalized_form` (~43 ns/char)**, и платят его ОБА пути при выводе нормальных форм. Плановый #2 (process-wide кэш декода) бьёт ровно по нему.

**Прототип:** per-thread `WordId → &'static str` кэш в `WordInfo::normalized_form` (env `SUDACHI_NF_CACHE`, один бинарь меряет off/on). Per-instance `StringsCache` не хитит между узлами (свежий `WordInfo` на узел/lookup); per-thread кэш дедуплицирует декод повторяющихся слов (служебные, частые кандзи). Строки ликаются (ограничено числом уникальных WordId; prod-версия владела бы ими в словаре). Content-identical: тоталы совпали off/on.

**Результат на обоих путях (GSD, best-min):**

| путь (выдаёт `normalized_form`) | cache OFF | cache ON | Δ |
|---|---:|---:|---|
| Vaporetto feature-recovery (§5.8) | 89.7 ns/char | **49.8** | **−44%** |
| Sudachi full-analysis e2e + NF-output | 326.8 ns/char | **285.1** | −13% |

Оба экономят ~40 ns/char (тот самый декод). Относительный выигрыш больше у Vaporetto (меньше база).

**Следствие — #2 переводит полную замену ЧЕРЕЗ 3×:**

| вывод seg+POS+`normalized_form` | cache OFF | cache ON |
|---|---:|---:|
| Sudachi e2e + NF | 326.8 | 285.1 |
| Vaporetto+dict (36.4 + recovery) | 126.1 | **86.2** |
| **отношение** | 2.6× | **3.3×** |

**Итоги [meas]:**
1. **#2 ускоряет ОБА пути** на величину декода (~40 ns/char) — это первый рычаг, общий для решётки и pointwise.
2. Для **seg+POS-only** Sudachi #2 бесполезен (декода нет — `normalized_form` ленив и не трогается); помогает только при выводе нормальных форм (поиск/индексация, CLI `-a`).
3. **С #2 полная замена Vaporetto+dict достигает 3.3×** (против 2.6× без кэша) — наконец пробили 3× для паритета вывода (но цена прежняя: POS −3.96, нет A/B/C).

**Уровень 2 — word_id-кэш на Vaporetto-пути [meas]:** кэш `word_id → normalized_form`, проверяемый ДО `get_word_info_subset` → на попадании скипается весь fetch+декод (не только декод). Декомпозиция feature-recovery (best-min ns/char):

| режим | ns/char | что делает |
|---|---:|---|
| lookup только (trie) | 12.5 | exact-match |
| + POS-fetch | 44.6 | + парс fixed |
| + декод (full, без кэша) | 90.3 | базовый §5.8 |
| **word_id-кэш** | **18.1** | скип fetch+декод на хите (>trie на ~5.6) |

**Полная замена Vaporetto+dict с word_id-кэшем [meas]:**

| вывод seg+POS+`normalized_form` | ns/char | vs Sudachi |
|---|---:|---:|
| Sudachi e2e+NF, decode-cached (пол) | 285.1 | 1× |
| Sudachi e2e+NF, без кэша | 326.8 | — |
| **Vaporetto+dict (36.4 + 18.1 word_id-cached)** | **54.5** | **5.2× vs cached / 6.0× vs uncached** |

**~5× подтверждён (измерено).** Асимметрия честна: lookups Vaporetto-пути — *чистое* восстановление фич → кэш убирает их на повторе; Sudachi не может скипнуть материализацию WordInfo (нужна решётке), его пол остаётся ~285.

**Уровень 3 — surface→normalized кэш [meas]:** ключ = сама строка поверхности → на хите скипается ДАЖЕ trie-lookup (только хеш строки). Самый агрессивный — естественный для pointwise-пайплайна, у которого текст сегмента уже на руках.

**Полная лестница кэширования Vaporetto feature-recovery [meas, best-min]:**

| кэш recovery | recovery ns/char | Vaporetto-full (36.4+rec) | vs Sudachi (285 cached / 327 uncached) |
|---|---:|---:|---:|
| нет | 90 | 126.6 | 2.3× / 2.6× |
| #2 decode | 50 | 86.2 | 3.3× / 3.8× |
| word_id | 22 | 58.9 | 4.8× / 5.5× |
| **surface** | **7.6** | **44.0** | **6.5× / 7.4×** |
| асимптота (recovery→0) | →0 | 36.4 | 7.8× / 9.0× |

**Синтез:** чем агрессивнее кэш фич, тем ближе full-parity Vaporetto-замена к **чистым 8× сегментатора** (= пол = сам Vaporetto seg+POS 36.4). Surface-кэш (6.5–7.4×) почти на асимптоте — это bare seg+POS плюс хеш строки. **~6.8× из проекции подтверждён** (взят в вилку 6.5–7.4× в зависимости от того, кэширует ли и Sudachi). Пол Sudachi ~285 неизбежен (решётка + WordInfo-материализация — byte-identical стена §5.7), сколько фичи ни кэшируй.

4. **(Б) Prod-версия без leak + thread-safe — выигрыш выживает [meas].** Прототипы текут (`Box::leak`) и не Sync (`RefCell`). Production-вариант surface-кэша (owned `Arc<str>` без leak + `Mutex` для Sync):

| surface-кэш | recovery ns/char | Vaporetto-full | vs Sudachi 285 |
|---|---:|---:|---:|
| без кэша | 100.6 | 137.0 | 2.1× |
| leaky-прототип (`&'static`+RefCell) | 7.0 | 43.4 | 6.6× |
| **prod (`Arc<str>`+`Mutex`, без leak)** | **10.0** | **46.4** | **6.1×** |

Машинерия (lock + Arc-clone + owned-ключ) стоит **+3 ns/char**; выигрыш сохраняется (**6.1×** vs 6.6×, single-thread). Leak-как-intern-pool тоже валиден для долгоживущего процесса (ограничен словарём, ~единицы MB).

**Многопоточное масштабирование [meas]** (M4 Max 12 P-ядер, T потоков гоняют корпус ×25, агрегатная пропускная Mchar/s, `cache_mt_bench`):

| стратегия | T=1 | T=4 | T=12 | масштаб |
|---|---:|---:|---:|---|
| **global `Mutex`** | ~55 | ~17 | ~24 | **0.3–0.4× — КОЛЛАПС ниже 1 потока** |
| **`DashMap`** (sharded) | ~72 | ~96 | ~95 | 1.2–1.4× (безопасно, скромно) |
| **`thread_local`** (per-thread) | ~92 | ~323 | ~833 | **8.6–9.5× — почти линейно** |

**Вердикт:** глобальный `Mutex` — **lock convoy**: на T≥4 пропускная *падает ниже одно-поточной* (критическая секция крошечная — хеш+Arc-clone — всё время уходит в lock). `DashMap` безопасен, но даёт лишь ~1.3× (per-item работа слишком мала относительно shard-lock+Arc-refcount). **`thread_local` масштабируется почти линейно** (8.6–9.5× на 12 ядрах) ценой N× памяти (ограничено словарём) + отсутствия cross-thread reuse (каждый поток холодно заполняет свой кэш, амортизируется за время жизни). **Production-рекомендация: per-thread кэш** (`thread_local`), не общий lock и не DashMap. Это согласуется с тем, что multithread (#5, ~8.9×) — крупнейший рычаг: кэш должен ему не мешать.
5. **Важный caveat:** выигрыш кэшей пропорционален **повторяемости поверхностей**. GSD (543 предл.) — высокая повторяемость (служебные/частые слова) → тёплый кэш хитит почти всё → 6.5×. На разнородном/коротком входе recovery лежит между 7.6 (все хиты) и 90 (все промахи) ns/char; «холодный» кэш = базовые 2.3×. Числа лестницы — steady-state на повторяющемся корпусе (типично для батч-обработки/индексации).

### 5.10 (А) Диагноз POS-gap — он не фундаментальный [meas]

Вопрос: −3.96 POS (#21/#42) — это парадигма, артефакт тегсетов, или источник POS? Разбор:

1. **Тегсеты выровнены [meas].** Тег Vaporetto (`名詞-普通名詞-形状詞可能`) форматно ИДЕНТИЧЕН gold XPOS; скорер берёт `split("-")[0]` у обоих → сравнение честное, не артефакт. Значит −3.96 реален для MODEL-POS.
2. **Корень:** Sudachi POS не предсказывает, а **берёт из словаря** (gold UD-GSD размечен тем же UniDic → почти идеально, conditional 99.5%); Vaporetto **предсказывает** POS моделью (conditional 96.3%).
3. **Ключ:** полная замена Vaporetto+dict делает dict-lookup и так (ради `normalized_form`) → может взять **dict-POS вместо model-POS**. Замер на сегментации Vaporetto:

| источник POS | POS-top F | gap vs Sudachi 97.46 |
|---|---:|---:|
| MODEL tag (предсказание) | 93.50 | −3.96 |
| DICT in-context (95.1% спанов совпали с Sudachi) | 95.07 | −2.39 |
| **HYBRID (dict где спан совпал, иначе model)** | **96.27** | **−1.19** |

**Вывод (А): POS-gap не фундаментален.** Бо́льшая часть −3.96 — от MODEL-POS; взяв dict-POS (который пайплайн и так считает), gap сжимается до **−1.19**, и остаток — в основном пропагация сегментации (неверный спан → неверный POS). На верно сегментированных токенах dict-POS даёт conditional ≈99.2% ≈ Sudachi 99.5%. **Переобучение не нужно** — словарь (уже на руках ради `normalized_form`) даёт POS почти уровня Sudachi; модельный POS нужен лишь для дизамбигуации омографов (его 93.5% хватает выбрать среди dict-кандидатов).

**Итог честной цены замены Vaporetto+dict (с dict-POS):** SEG **−0.92**, POS-top **−1.19** (не −3.96) — порядок величины меньше. Остаётся жёсткое: **нет A/B/C** (одна сегментация) + переобучка модели под язык/словарь. Скрипт: `/tmp/gold/score_vapo_dictpos.py`.

### 5.11 Микроархитектурный разбор `connect_node` — матрица НЕ DRAM-bound (опровержение премиссы) [meas]

Свежий угол «ниже ассемблера»: 18-агентный разбор горячего ядра `Lattice::insert`/`connect_node` (Viterbi min-plus, ~22–25% do_tokenize). Гипотеза была — матрица 71MB DRAM-latency-bound, и батчинг connect_node по right-узлам границы углубит MLP. **Гипотеза опровергнута, премисса исследования исправлена.**

**Дизасм (release ARM64): кодген оптимален.** Инвариант `R.left_id*num_left` + data-ptr вынесены из цикла; `ldrsh` без bounds-check (`get_unchecked`); min+argmin полностью branchless (3× `csel`); матричная загрузка адресно-независима между итерациями → **уже MLP-capable**. Ассемблер не рычаг.

**Эмпирика (GSD, инструментированный connect_node):** 173 803 вызова, 1 506 919 матричных lookup'ов, **avg 8.67 left-узлов/вызов** (max 64); 27% вызовов ≤4 узла. **BOS-skip = 0%** на полностью достижимом корпусе → ветка `is_connected_to_bos` всегда not-taken (мёртвый груз, но ~free).

**Три независимых пилона опровержения:**
1. **LRU-симуляция** на точной трассе 1 506 919 lookup'ов: всего **30 916 уникальных 128B-линий = 3.77 MiB working set**, miss 2.05% **и при 4MB, и при 16MB L2, ZERO capacity-misses** (все compulsory) → 97.95% загрузок — L2-хиты. Skew: тронуто лишь **1474/5981** conn-id; top-16 строк = 47%, top-256 = 92.5%. Conn-id выводятся из POS (ограниченный Zipf-словарь) → working set L2-резидентен для естественного текста.
2. **nomat-проба** (матричная загрузка заменена на ALU, форма цикла/VNode-чтения/min-argmin сохранены — проверено по сдвигу morpheme-count 12375→12572): baseline 6.67ms vs nomat 6.19ms = **1.0775×** → вся матрица 71MB = **≤7.2% do_tokenize**. Удаление загрузки целиком — теоретический потолок любого latency-hiding (batch/prefetch только переупорядочивают, не удаляют).
3. **Цикловая арифметика:** 5.42 cyc/load (в ~1.7× от issue-floor 3.25); серийный DRAM при глубине 8.67 дал бы 150.7ms ≫ всего прохода 6.67ms (физически невозможно). L2-hit-модель (4ns × 8.67) → 0.70ms ≈ наблюдаемая дельта 0.5–0.8ms.

**Вывод: ядро на байт-идентичном полу.** Матрица L2-резидентна и **throughput/issue-bound, не latency-bound**. Потолок ЛЮБОЙ matrix-locality/MLP/prefetch-оптимизации = ~7.2%, реально **~0%** (батчинг бьёт в non-problem; ожидаемо net-негативен — 8–44 спиленных аккумулятора + пересчёт row-base ломают плотный 13-инстр цикл, ровно как провал #24 −12%). **Исправление премиссы:** прежнее «матрица 22.8%, volume/DRAM-bound» (§6) — это connect_node-ЦИКЛ (1.5M итераций × дешёвая работа), а матричная ЗАГРУЗКА внутри — лишь ≤7.2% и L2-резидентна; вывод «не улучшаема byte-identical» остаётся, но механизм был не тот.

**Редирект:** оставшиеся ~93% do_tokenize — вне этого ядра и вне matrix/MLP-линзы: VNode parallel-array pushes (`insert` L131-133), trie/lexicon lookup, OOV, path-rewrite (последний уже срезан #39/#40). Ни один не даёт большого рычага в matrix/MLP-линзе — но **позже H7/#52 нашёл крупный (~+20–21%) именно в материализации WordInfo** (parse+resolve ~17%, отыгранный shared `Arc`-кэшем), подняв byte-identical потолок до ~1.35×. **Диагностический приём** (для статьи/регрессий): nomat-проба + LRU-sim — воспроизводимый способ доказать «kernel at floor» (артефакты воспроизводимы; см. синтез workflow `lattice-microarch`).

### 5.12 Реальный fused-прототип Vaporetto→Sudachi-dict — авторитетные числа + поправка [meas]

§5.8–5.9 давали «полную замену» как арифметику `v64(36) + feature-recovery(7.6) = 44 → 6.5×`. **Это была проекция со смешением профилей.** Собран РЕАЛЬНЫЙ fused-бинарь (`/tmp/vapo-sud`: vaporetto 0.6.5 + sudachi path-dep в одном крейте, LTO, Sentence-reuse): Vaporetto сегментирует+`fill_tags`, на каждый токен — exact dict-lookup (`normalized_form` + dict-POS), вывод в Sudachi-форме. Один бинарь, внутренне-консистентно.

**Поправка (что вскрыл реальный билд):** `v64`-число 36 ns/char было `predict()` **без `fill_tags` и без материализации токенов** — недосчёт. Реальный seg+POS (с извлечёнными тегами) = **~71 ns/char**.

**Скорость (GSD, best-min, fused-бинарь):**

| вариант | ns/char | примечание |
|---|---:|---|
| vaporetto-only (seg+POS, материализован) | **~71** | было ошибочно 36 |
| fused, без кэша | ~182 | dict-lookup на каждый токен дорог (+110) |
| **fused + surface-cache (полный паритет вывода)** | **~78** | dict-recovery cached **+7–9** (валидирует feature_lookup_bench 7.6) |

→ **полная замена ≈ 78 ns/char vs Sudachi ~290 = ~3.7×** (НЕ 6.5×).

**Качество (реальный вывод fused, scored vs gold):**

| POS-источник | GSD-test | GSD-dev | смысл |
|---|---:|---:|---|
| SEG-F1 | 97.07 | 96.16 | −0.92 vs Sudachi |
| POS model (предсказание) | 93.50 | 92.31 | −3.96 |
| POS **dict-first** (без дизамбигуации) | **69.76** | **70.61** | ⚠️ омограф-ловушка — НЕЛЬЗЯ брать первую запись |
| POS **hybrid** (dict-кандидаты, дизамбиг моделью) | **95.72** | 94.40 | −1.74; реалистичный быстрый путь |
| normalized произведено | 13258/13258 | 12592/12592 | 100% |

**Два инженерных открытия от реального артефакта:**
1. **Полная замена ~3.7×, а не 6.5×** — проекция была оптимистична (v64 не материализовал теги). Component-числа (seg 27 / recovery 7.6) валидны порознь, но их СУММА была неверна как «полная замена».
2. **Dict-POS требует дизамбигуации моделью.** Брать первую exact-запись = POS 70 (омографы). Гибрид (dict-кандидаты ∩ model-POS) = 95.7. Т.е. fused-пайплайн обязан использовать ОБА: Vaporetto-POS (выбрать среди кандидатов) + словарь (normalized + POS-кандидаты).

**Робастность (GSD-test/dev + kyoto-leads news 461k):** скорость **71–113 ns/char** (короче предложения → выше per-sentence overhead), ~3.5–4× vs Sudachi; SEG **96–97**, hybrid-POS **94–96**; dict-first-ловушка (~70) консистентна. Числа держатся, не коллапсируют. Артефакт: `/tmp/vapo-sud` (запускаемый), scorer `/tmp/gold/score_fused.py`.

**Итог честной цены замены (исправлено):** скорость **~3.7×** (не 6.5×), SEG **−0.92**, POS-top **−1.74** (быстрый hybrid; −1.19 при in-context дизамбигуации §5.10), жёстко — нет A/B/C + переобучка. >3× по-прежнему верно, но порядок 3–4×, не 6–8×.

## 6. Обсуждение

**Сходимость.** Четыре независимых раунда литературного research'а (структуры
данных; SIMD/asm; build-time layout; load-count) + ассемблерный разбор walk'а +
эмпирические прототипы дали один и тот же вывод: **byte-indexed darts-clone walk на
практическом полу** — 1 зависимая загрузка/байт, оптимальный кодоген, latency-bound;
формат уже cache-conscious (XOR кладёт всех детей в 256-unit окно); единственная
незакрытая ось — **число загрузок**, срезаемое только charwise-индексацией (формат).

**Что сработало.** prefetch K=4 (исходная L1-идея #117, измеренный +1.20–1.25× iso /
+5–17% e2e); varint (+2.7%); оба зашиплены и exact. Это исследование добавило два
новых byte-identical переноса из vibrato-декомпозиции (§5.5): **#39** (gate
`normalized_form`-декода в JoinNumeric) + **#40** (early-out переменной секции
WordInfoParser) = **~9%** на реалистичном subset (§5.6–5.7). **multithread ~8.9×** —
крупнейший format-preserving рычаг. **Vaporetto** воспроизводит литературные 7–9×.

**Качество (золотой бенчмарк).** Впервые измерена точность Sudachi: mode A
**SEG-F1 97.99 / POS-top 97.46**. Парадигменный размен честно квантифицирован и
**уточнён** (§5.10): ранее казавшийся POS −3.96 — артефакт MODEL-предсказанного POS;
при **dict-POS** (словарь и так на руках ради `normalized_form`) gap = **−1.19**.
Итог цены замены Vaporetto+dict: SEG −0.92, POS −1.19, жёстко — нет A/B/C.

**Vaporetto как замена — лестница (§5.8–5.9).** Bare seg+POS **8.1×**; полная замена
(паритет вывода) **2.3× без кэша → 3.3× decode-кэш (#2) → 6.5× surface-кэш**,
асимптота → 8× (= сам сегментатор). decode-кэш (#2, ~40 ns/char) — **первый рычаг,
общий решётке и pointwise**; prod-версия без leak + thread-safe выживает (6.1×).
Выигрыш кэшей ∝ повторяемости поверхностей (steady-state на батч-корпусе).

**Главные отрицательные уроки.** `connect_node`-цикл — #1 хотспот (~22–25%), но
**механизм исправлен (§5.11):** не DRAM/volume-bound, а L2-резидентен и
throughput/issue-bound — live working set лишь **3.77 MiB** (0 capacity-misses, 97.95%
L2-хитов; тронуто 1474/5981 conn-id), а сама матричная загрузка = **≤7.2% do_tokenize**
(nomat-проба); ядро на байт-идентичном полу, кодген оптимален. Поэтому matrix
prefetch/quant/relayout/MLP-batching бьют в non-problem (объясняет #24 −12%).
Проверенные layout-трюки на trie либо мертвы (vEB), либо обратны (code-dividing),
либо backfire для японского (path-compression). Оговорка: корпус-частотный
prefetch-friendly relayout двойного массива (исходная идея #117) **не прототипирован
эмпирически** — закрыт лишь по литературе (N7/N8: yada строит из сорт. ключей, relayout
≈ ≤1.10× lookup при ~14× build); как направление он остаётся открытым, особенно если
паттерны окажутся доступнее аппаратному prefetch (потенциально и для Java-версии).

## 7. Выводы и рекомендации

1. **Зашипить как есть** prefetch K=4 + varint (PR #348) — основная часть
   доступного format-preserving одно-поточного выигрыша, exact.
2. **Для throughput** — multithreading (~8.9×), складывается со всем.
3. **Единственный trie-рычаг** — charwise crawdad (+1.37× iso / ~+6–10% e2e [proj],
   смена формата); прототип-интеграция оценена, но не доведена до e2e-числа.
4. **Парадигма pointwise (Vaporetto)** — измерено (§5.8–5.10, #41–45):
   **8.1× для seg+POS**; как полная замена (с `normalized_form`) — **2.3× без кэша,
   3.3× с decode-кэшем (#2), до 6.5× с surface-кэшем** (§5.9, зависит от
   повторяемости). Цена качества — **не −3.96, а SEG −0.92 / POS −1.19** при dict-POS
   (§5.10); жёстко остаётся лишь потеря A/B/C + переобучка. «Иксы» = «делать меньше».
5. **Главный синтез (исправлено #48/#52):** 3× с сохранением ПОЛНОГО вывода
   Sudachi недостижимо byte-identical (потолок **~1.35×**: #39/#40 + **H7b +20–21%** #52;
   ядро решётки на полу §5.11, но материализация WordInfo — нет). Парадигма (реальный
   fused) = **~3.7×**, меняет вывод (SEG −0.92, POS −1.74, нет A/B/C) = редукция задачи.
6. **Связующее ограничение — Java-совместимость (решающее).** sudachi.rs
   контрактно повторяет вывод Java-Sudachi (A/B/C, точные POS/границы). Парадигма
   это ломает → **не опция для sudachi.rs**, только отдельный инструмент. Реальный
   путь библиотеки: byte-identical-выигрыши (#39/#40) + multithreading (~8.9×);
   матрицу/решётку не трогать (§5.11). Детали — `paradigm-proposal.md`.
7. **Не повторять** N1–N16 (§5.4).

### 7.5 Дорожная карта: выбранный путь и отложенные направления

После #39+#40 (~9% byte-identical, рубеж ~290 ns/char do_tokenize, реалистичный subset) развилка к 3× (Vibrato 112 ns/char). **Решение (2026-06-13): идём в парадигм-сдвиг Vaporetto** — единственный путь с проверенным ≥3× (реально 7–13×). Остальные три **отложены, но жизнеспособны** — сюда возвращаться, если парадигма не подойдёт по точности/выводу:

| # | направление | ожид. выигрыш | byte-id? | объём/риск | где начинать |
|---|---|---|---|---|---|
| **A (выбран)** | **Vaporetto pointwise** | **7–13× (#6–7)** | ❌ меняет вывод | модель готова (`/tmp/vapo/model.raw`); harness `/tmp/vapo64` | §5.7 + текущая работа: честный matched full-pipeline бенч |
| B (отложен) | borrowed-узлы решётки (отказ от owned WordInfo на узел; `&str`-срезы как Vibrato) | ~1.3–1.6× (атакует ~20% `resolve_best_path` + аллокации) | ✅ возможно | **большой рефактор** `ResultNode`/плагинов/`MorphemeList`; риск для идентичности | профиль §5.7; цель — убрать `memmove`/`free`/`resolve` per-node |
| C (отложен) | charwise-trie (crawdad) | ×1.39 iso / ~5–6% e2e | ✅ (но смена формата словаря) | средний; нужен ре-энкод словаря + своп trie | §5.0 #11; интеграция оценена, не доведена до e2e |
| D (тупик) | `Lattice::insert`/connection-matrix (~25%) | — | — | исторически отриц./lossy (#22–26); matrix volume-bound | НЕ ходить без смены вывода |

**Состояние прототипов (для возврата):** byte-identical переносы #39+#40 живут в worktree `/tmp/sud-vib` (branch `vib-cmp`, off `feat/117-runtime-prefetch`), 4 файла: `dic/strings_cache.rs`, `dic/word_info/data.rs`, `dic/word_info/parse.rs`, `plugin/path_rewrite/join_numeric.rs` + бенч `examples/tokenize_pipeline_bench.rs` (env `SUDACHI_BENCH_SUBSET`). Не закоммичены, PR не делали (по запросу — сейчас только бенчмарк). 3× в byte-identical-режиме признан недостижимым (потолок = matrix ~25% + архитектура owned-узлов); компаунд B+C+аллокации дал бы у Vibrato ~3×, но это месяцы рефактора.

## 8. Ссылки (рецензируемые источники)

- M. Farrar. *Striped Smith-Waterman speeds database searches.* Bioinformatics 23(2):156–161, 2007.
- S. Marco-Sola et al. *Fast gap-affine pairwise alignment using the wavefront algorithm (WFA).* Bioinformatics 37(4):456–463, 2021.
- Ferreira/Roma/Russo. *Inter-task SIMD Viterbi (COPS/HMMER).* BMC Bioinformatics 15:165, 2014.
- Chen et al. *Exact Lattice Generation on GPU (Kaldi WFST).* Interspeech 2018.
- André, Kermarrec, Le Scouarnec. *Cache locality / Quick(er) ADC (PQ Fast Scan).* VLDB 2016 / ICMR 2017 / IEEE TPAMI 2019.
- Wang et al. *Hyperscan (FDR/Teddy).* USENIX NSDI 2019.
- D. Lemire, N. Kurz, C. Rupp. *Stream VByte.* Information Processing Letters 130, 2018.
- V. Leis, A. Kemper, T. Neumann. *The Adaptive Radix Tree (ART).* IEEE ICDE 2013.
- Barratt & Zhang. *Cache-Friendly Search Trees.* arXiv:1907.01631, 2019.
- Khuong & Morin. *Array Layouts for Comparison-Based Searching.* ACM JEA / SEA 2017.
- Brodal, Fagerberg, Jacob. *Cache Oblivious Search Trees via Binary Trees of Small Height.* SODA 2002.
- Bender, Demaine, Farach-Colton. *Cache-Oblivious B-Trees.* FOCS 2000 / SICOMP 2005.
- Bender et al. *The Cost of Cache-Oblivious Searching.* Algorithmica 61(2), 2011.
- Lindstrom & Rajan. *Optimal Hierarchical Layouts for Cache-Oblivious Search Trees (MinWEP).* IEEE ICDE 2014.
- Rao & Ross. *Cache Conscious Indexing (CSS) / Cache-Sensitive B+-Trees (CSB+).* VLDB 1999 / SIGMOD 2000.
- Yoshinaga & Kitsuregawa. *A Self-adaptive Classifier... (cedar, XOR double-array).* COLING 2014.
- Kanda, Morita, Fuketa. *Compressed double-array tries (XCDA).* Knowledge and Information Systems 51:1023–1042, 2017.
- Yata et al. *A compact static double-array / minimal-prefix double array.* IP&M 43(1) / SPE 37(5), 2007.
- Zeitak & Morrison. *Cuckoo Trie: Exploiting Memory-Level Parallelism.* SOSP 2021.
- Liu et al. *Compression Methods... Code Dividing for Chinese DA-trie.* IJCNLP 2011.
- *StriD2FA / stride-k DFA.* IEEE ICC 2011 / JNCA 2015.
- Kudo, Yamamoto, Matsumoto. *Applying CRFs to Japanese Morphological Analysis (MeCab).* EMNLP 2004.
- Neubig, Nakata, Mori. *Pointwise Prediction for... Japanese MA (KyTea).* ACL 2011.
- Takaoka et al. *Sudachi: a Japanese Tokenizer...* LREC 2018.
- Akabe, Kanda, Oda, Mori. *Vaporetto: Efficient Japanese Tokenization...* 2024 (arXiv:2406.17185).
- N. Yoshinaga. *Back to Patterns: Efficient Japanese MA with Feature-Sequence Trie (Jagger).* ACL 2023.
- Kanda, Akabe, Oda. *Engineering faster double-array Aho-Corasick (daachorse).* SPE 53(6):1332–1361, 2023.
- *UD_Japanese-GSD treebank* (Universal Dependencies); NINJAL *BCCWJ* SUW/LUW scheme.

## Приложение A — воспроизведение

```bash
# Качество (golden benchmark)
wget https://github.com/UniversalDependencies/UD_Japanese-GSD/raw/master/ja_gsd-ud-test.conllu
python score.py            # Sudachi A/B/C SEG/POS-F1 vs GSD; пишет gsd_text.txt
VAPO_MODEL=model.raw VAPO_INPUTS=gsd_text.txt VAPO_DUMP=1 v64 > vapo_out.txt
python score.py            # + Vaporetto SEG-F1

# Скорость e2e
SUDACHI_BENCH_DICT=full/system_full.dic SUDACHI_BENCH_TRIALS=31 \
  cargo run -p sudachi --release --example tokenize_pipeline_bench

# Изолированный lookup (cross-method)
SUDACHI_TRIE_BENCH_SURFACE_LEXICONS=small.csv:core.csv:notcore.csv \
  cargo run -p sudachi --release --example dictionary_matcher_report --features matcher-comparison

# do_tokenize по реалистичному subset + #39/#40 + decode-кэш (#2)
SUDACHI_BENCH_SUBSET=min SUDACHI_BENCH_MODE=C SUDACHI_BENCH_NOCOLLECT=1 \
  SUDACHI_NF_CACHE=1 SUDACHI_BENCH_ACCESS_NF=1 \
  cargo run -p sudachi --release --example tokenize_pipeline_bench
# subset: all|min|pos|all_cli ; SUDACHI_NF_CACHE — decode-кэш; ACCESS_NF — вывод norm-форм

# Vaporetto путь A: feature-recovery + лестница кэшей (full/pos/lookup/cache/scache/scache_prod)
VAPO_MODEL=model.raw VAPO_INPUTS=gsd_text.txt VAPO_DUMP=1 v64 | tr '\t' '\n' | grep -v '^$' > vapo_surfaces.txt
SUDACHI_BENCH_INPUTS=vapo_surfaces.txt SUDACHI_BENCH_FEAT=scache \
  cargo run -p sudachi --release --example feature_lookup_bench

# Диагноз POS-gap (А): model-POS vs dict-POS vs hybrid на сегментации Vaporetto
python score_vapo_dictpos.py

# Многопоточность кэша: Mutex vs DashMap vs thread_local (T=1..12)
SUDACHI_BENCH_INPUTS=vapo_surfaces.txt SUDACHI_BENCH_THREADS=1,2,4,8,12 SUDACHI_BENCH_REPEAT=25 \
  cargo run -p sudachi --release --example cache_mt_bench   # требует dashmap (dev-dep)
```

**Артефакты:** `bench/quality/{test.conllu,score.py,gsd_text.txt}`; результаты —
`/tmp/gold/` (+`score_vapo_dictpos.py`, `vapo_surfaces.txt`), Vaporetto-harness
`/tmp/vapo64` (`v64`), worktree `/tmp/sud-vib` (`vib-cmp`: #39/#40/#2 +
`feature_lookup_bench` с режимами кэшей). Прототипы не закоммичены (PR не делали —
задача была измерить). Провенанс каждого числа — теги [meas]/[doc]/[lit]/[proj] в шапке.
