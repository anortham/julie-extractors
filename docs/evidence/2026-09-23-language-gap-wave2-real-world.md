# Language gap wave 2: real-repository scan comparison

Date: 2026-09-23. Plan: [language gap closure](../plans/2026-09-22-language-gap-closure.md).
Wave 1: [wave-1 comparison](2026-09-22-language-gap-wave1-real-world.md).

Each repository under `~/source` was scanned into a fresh artifact with the
published 3.3.1 release binary and with the 3.4.0 release build of the commit
that adds this file. Columns count rows per language, old -> new. Every scan
exited 0 with no failed files.

## Scan time

The first comparison of the wave-2 head (`86f496f3`) showed scan-time
regressions against the wave-1 build (`0ac8705c`). Wall clock, best of three
runs, in seconds:

| Repository | Wave 1 | Wave 2 before fix | Final |
| --- | --- | --- | --- |
| zod | 2.90 | 3.64 | 2.55 |
| Newtonsoft.Json | 5.21 | 6.62 | 5.16 |
| radzen-blazor | 15.49 | 16.91 | 14.29 |
| Alamofire | 8.43 | 9.33 | 9.36 |
| nlohmann-json | 4.33 | 4.48 | 4.41 |
| omarchy | 3.27 | 3.13 | 3.08 |
| express | 0.51 | 0.65 | 0.53 |
| jq | 0.88 | 0.91 | 0.88 |

Wave 2 added symbols and owner lookups to code whose cost grew with the symbol
count or the tree depth. tree-sitter 0.26 `Node::parent` and the sibling
lookups search down from the root on every call.

- TypeScript, JavaScript, Dart, and Swift owner lookups walked up with
  `Node::parent`. They now descend once from the root.
- C# built a member-scope index and a symbol index for every call site, and
  its member scope walked up with `Node::parent`. Both indexes are now built
  once per relationship pass. One Newtonsoft.Json test file went from 1.2 s to
  0.05 s of relationship extraction.
- JSON, TOML, and YAML config key paths scanned every symbol for each
  ancestor of each scalar value. An id index now answers each ancestor lookup.
- Structural facts scanned every symbol to find each fact's owner. Wave 2
  nearly doubled YAML container symbols (`pnpm-lock.yaml` 3419 -> 6170), so
  that scan grew. The byte-containment pass is now one sweep by start byte.
- YAML test-role checks and the trailing-doc sibling lookup ran for files that
  cannot hold a match. They now check the file or the line first.

The fix changes no output. Every extraction table of all 13 repositories below
was dumped and compared between `86f496f3` and the final build: identical.

Alamofire stays about 10% slower than wave 1 because it writes about 190,000
more rows: HTML links become structured pending rows (2,324 -> 62,166) and
literals grow from 10,206 to 78,582. No single file got slower.

## Findings

- No crash and no failed file.
- Newtonsoft.Json C# parse diagnostics 857 -> 5: C# blanks preprocessor
  directive lines and `#elif`/`#else` branches before the parse.
- Alamofire HTML pending 2,324 -> 62,166: links, scripts, anchors, and form
  actions are structured pending rows instead of synthetic relationships.
- omarchy Lua symbols 759 (wave 1) -> 181: call-argument table fields are no
  longer symbols (fields 471 -> 16 against 3.3.1).
- omarchy QML identifiers 50,133 -> 33,200: declaration sites emit no
  `variable_ref` rows.
- zod YAML symbols 13,390 (wave 1) -> 15,792: flow-mapping pairs and anchored
  sequence items are symbols.
- nlohmann-json C++ symbols 23,197 (wave 1) -> 26,722: `#include` and
  `#define` give import and constant rows.
- JavaScript symbols fall (express 5,231 -> 4,648 against wave 1): data object
  literals in expressions no longer emit property symbols.
- radzen-blazor Razor symbols 13,263 (wave 1) -> 12,216: one row per
  directive and one file class per component.
- New file selections: Newtonsoft.Json XML files 9 -> 12, sinatra Ruby
  147 -> 160, zod JSON 41 -> 42.

The [3.4.0 output ledger](../contracts/extraction-output-changes.md) declares
each of these row changes.

## Report

```text
jq old exit=0 secs=0.8 failed=0
jq new exit=0 secs=0.9 failed=0
lang files: symbols rels pending idents facts diags (old -> new)
  bash        files 2->14 sym 24->120 rel 0->0 pend 0->2 id 98->1324 facts 12->77 diag 0->0
  c           files 47->47 sym 6481->6253 rel 2043->2193 pend 7343->7182 id 31036->31021 facts 947->947 diag 499->499
  cpp         files 2->2 sym 23->35 rel 0->0 pend 59->55 id 99->95 facts 0->0 diag 0->0
  css         files 1->1 sym 27->27 rel 0->0 pend 0->0 id 30->30 facts 27->27 diag 0->0
  javascript  files 1->1 sym 17->10 rel 0->3 pend 3->0 id 91->91 facts 0->0 diag 0->0
  json        files 9->9 sym 57->63 rel 0->0 pend 0->0 id 0->0 facts 97->97 diag 1->1
  markdown    files 5->5 sym 108->113 rel 0->0 pend 0->0 id 0->0 facts 109->111 diag 0->0
  python      files 5->5 sym 186->178 rel 61->60 pend 160->162 id 623->623 facts 1->1 diag 0->0
  toml        files 1-> sym 12-> rel 6-> pend 0-> id 0-> facts 18-> diag 0->
  unsupported files 316->303 sym 0->0 rel 0->0 pend 0->0 id 0->0 facts 0->0 diag 0->0
  yaml        files 19->19 sym 7042->9359 rel 0->7 pend 0->0 id 0->7 facts 11603->11669 diag 0->0
nlohmann-json old exit=0 secs=4.0 failed=0
nlohmann-json new exit=0 secs=4.6 failed=0
lang files: symbols rels pending idents facts diags (old -> new)
  bash        files 1->1 sym 2->2 rel 0->0 pend 0->0 id 8->8 facts 2->2 diag 0->0
  c           files 2->2 sym 22->22 rel 2->2 pend 13->13 id 50->50 facts 1->1 diag 1->1
  cpp         files 499->499 sym 21446->26722 rel 3948->5823 pend 32736->51250 id 147869->139901 facts 1465->1465 diag 8217->8192
  css         files 1->1 sym 1->1 rel 0->0 pend 0->0 id 2->2 facts 1->1 diag 0->0
  html        files 2->2 sym 6->6 rel 0->0 pend 0->0 id 0->0 facts 6->6 diag 0->0
  json        files 8->8 sym 52->52 rel 0->0 pend 0->0 id 0->0 facts 66->66 diag 0->0
  lua         files 1->1 sym 0->0 rel 0->0 pend 0->0 id 7->7 facts 0->0 diag 0->0
  markdown    files 272->272 sym 9372->9309 rel 36->48 pend 0->213 id 0->0 facts 8237->9553 diag 10->10
  python      files 7->7 sym 457->445 rel 48->45 pend 347->342 id 1515->1515 facts 0->0 diag 0->0
  sql         files 1->1 sym 6->6 rel 0->1 pend 0->0 id 3->944 facts 241->241 diag 0->0
  swift       files 1->1 sym 2->2 rel 0->0 pend 1->8 id 14->14 facts 0->3 diag 0->0
  unsupported files 426->426 sym 0->0 rel 0->0 pend 0->0 id 0->0 facts 0->0 diag 0->0
  yaml        files 23->23 sym 1428->1891 rel 0->0 pend 0->0 id 0->0 facts 2342->2508 diag 0->0
cobra old exit=0 secs=0.4 failed=0
cobra new exit=0 secs=0.4 failed=0
lang files: symbols rels pending idents facts diags (old -> new)
  go          files 36->36 sym 3589->3665 rel 658->618 pend 3218->2996 id 17397->17397 facts 53->53 diag 0->0
  json        files 6->6 sym 45->45 rel 0->0 pend 0->0 id 0->0 facts 51->51 diag 0->0
  markdown    files 17->17 sym 372->382 rel 1->2 pend 0->3 id 0->0 facts 374->375 diag 0->0
  unsupported files 15->15 sym 0->0 rel 0->0 pend 0->0 id 0->0 facts 0->0 diag 0->0
  yaml        files 5->5 sym 134->165 rel 0->0 pend 0->0 id 0->0 facts 232->251 diag 0->0
express old exit=0 secs=0.4 failed=0
express new exit=0 secs=0.5 failed=0
lang files: symbols rels pending idents facts diags (old -> new)
  css         files 4->4 sym 13->13 rel 0->0 pend 0->0 id 3->3 facts 13->13 diag 0->0
  html        files 8->8 sym 24->22 rel 0->0 pend 2->2 id 1->1 facts 3->4 diag 24->24
  javascript  files 141->141 sym 5436->4648 rel 290->282 pend 172->1227 id 21818->21818 facts 7->7 diag 0->0
  json        files 8->8 sym 121->121 rel 0->44 pend 0->0 id 0->0 facts 137->187 diag 0->0
  markdown    files 4->4 sym 197->504 rel 15->22 pend 0->0 id 0->0 facts 495->502 diag 0->0
  unsupported files 56->56 sym 0->0 rel 0->0 pend 0->0 id 0->0 facts 0->0 diag 0->0
  yaml        files 6->6 sym 259->305 rel 0->2 pend 0->0 id 0->2 facts 406->447 diag 0->0
flask old exit=0 secs=0.5 failed=0
flask new exit=0 secs=0.5 failed=0
lang files: symbols rels pending idents facts diags (old -> new)
  bash        files 1->1 sym 2->2 rel 0->0 pend 0->0 id 6->6 facts 1->1 diag 0->0
  css         files 2->2 sym 26->26 rel 0->0 pend 0->0 id 19->19 facts 26->26 diag 0->0
  html        files 20->20 sym 138->153 rel 0->1 pend 4->9 id 49->196 facts 59->62 diag 9->17
  json        files 10->10 sym 72->72 rel 0->0 pend 0->0 id 0->0 facts 86->86 diag 0->0
  markdown    files 6->6 sym 17->28 rel 0->5 pend 0->0 id 0->0 facts 18->23 diag 0->0
  python      files 83->83 sym 5801->5755 rel 250->210 pend 3588->3775 id 15419->15552 facts 715->975 diag 0->0
  sql         files 2->2 sym 11->10 rel 1->1 pend 0->0 id 0->8 facts 13->13 diag 2->2
  toml        files 5->5 sym 233->233 rel 19->19 pend 0->1 id 0->0 facts 242->282 diag 0->0
  unsupported files 113->113 sym 0->0 rel 0->0 pend 0->0 id 0->0 facts 0->0 diag 0->0
  yaml        files 8->8 sym 227->305 rel 0->2 pend 0->0 id 0->2 facts 414->451 diag 0->0
gson old exit=0 secs=1.4 failed=0
gson new exit=0 secs=1.2 failed=0
lang files: symbols rels pending idents facts diags (old -> new)
  java        files 264->264 sym 15621->15640 rel 1680->1728 pend 20650->21608 id 59292->62200 facts 3798->3810 diag 0->0
  json        files 8->8 sym 111->129 rel 0->0 pend 0->0 id 0->0 facts 137->137 diag 0->0
  markdown    files 16->16 sym 391->403 rel 37->37 pend 0->7 id 0->0 facts 392->405 diag 0->0
  unsupported files 23->23 sym 0->0 rel 0->0 pend 0->0 id 0->0 facts 0->0 diag 0->0
  xml         files 8->8 sym 2->18 rel 0->0 pend 0->14 id 0->0 facts 26->119 diag 0->0
  yaml        files 8->8 sym 300->346 rel 0->0 pend 0->0 id 0->0 facts 464->511 diag 0->0
Newtonsoft.Json old exit=0 secs=5.2 failed=0
Newtonsoft.Json new exit=0 secs=5.2 failed=0
lang files: symbols rels pending idents facts diags (old -> new)
  csharp      files 945->945 sym 38068->38101 rel 3083->3831 pend 44592->43989 id 196893->195380 facts 9->9 diag 857->5
  json        files 29->29 sym 1166->1469 rel 1->0 pend 0->0 id 0->0 facts 1855->1855 diag 0->0
  markdown    files 5->5 sym 55->70 rel 0->9 pend 0->0 id 0->0 facts 55->64 diag 0->0
  powershell  files 5->5 sym 417->243 rel 25->49 pend 138->198 id 1113->1141 facts 106->71 diag 47->47
  unsupported files 186->183 sym 0->0 rel 0->0 pend 0->0 id 0->0 facts 0->0 diag 0->0
  xml         files 9->12 sym 4->16 rel 0->0 pend 0->7 id 1->78 facts 21->175 diag 0->0
  yaml        files 2->2 sym 92->107 rel 0->0 pend 0->0 id 0->0 facts 138->145 diag 0->0
sinatra old exit=0 secs=0.6 failed=0
sinatra new exit=0 secs=0.7 failed=0
lang files: symbols rels pending idents facts diags (old -> new)
  json        files 6->6 sym 45->45 rel 0->0 pend 0->0 id 0->0 facts 51->51 diag 0->0
  markdown    files 12->12 sym 865->957 rel 91->160 pend 0->0 id 0->0 facts 864->936 diag 0->0
  ruby        files 147->160 sym 5860->5349 rel 1090->1710 pend 13040->13044 id 23198->24783 facts 4324->4417 diag 0->0
  unsupported files 127->114 sym 0->0 rel 0->0 pend 0->0 id 0->0 facts 0->0 diag 0->0
  yaml        files 10->10 sym 255->390 rel 3->3 pend 0->0 id 3->3 facts 512->528 diag 0->0
Alamofire old exit=0 secs=8.2 failed=0
Alamofire new exit=0 secs=9.4 failed=0
lang files: symbols rels pending idents facts diags (old -> new)
  css         files 4->4 sym 306->306 rel 0->0 pend 0->0 id 470->472 facts 306->306 diag 6->6
  html        files 332->332 sym 86660->86328 rel 0->0 pend 2324->62166 id 202610->202648 facts 69372->70036 diag 0->0
  javascript  files 6->6 sym 1998->1442 rel 124->288 pend 82->78 id 4986->4986 facts 0->0 diag 0->0
  json        files 27->27 sym 9980->10134 rel 0->0 pend 0->0 id 0->0 facts 12660->12660 diag 4->4
  markdown    files 12->12 sym 3143->3156 rel 38->282 pend 0->3 id 0->0 facts 3144->3162 diag 0->0
  ruby        files 1-> sym 0-> rel 0-> pend 0-> id 5-> facts 0-> diag 0->
  swift       files 101->101 sym 10713->10772 rel 410->514 pend 8254->9404 id 44231->43298 facts 1171->1181 diag 25->25
  unsupported files 90->89 sym 0->0 rel 0->0 pend 0->0 id 0->0 facts 0->0 diag 0->0
  xml         files 1->1 sym 0->0 rel 0->0 pend 0->0 id 0->0 facts 1->1 diag 0->0
  yaml        files 5->5 sym 553->693 rel 0->0 pend 0->0 id 0->0 facts 786->820 diag 0->0
moshi old exit=0 secs=0.9 failed=0
moshi new exit=0 secs=0.9 failed=0
lang files: symbols rels pending idents facts diags (old -> new)
  bash        files 1-> sym 28-> rel 0-> pend 0-> id 77-> facts 9-> diag 1->
  java        files 57->57 sym 3949->3951 rel 487->494 pend 8673->8784 id 20053->21103 facts 1088->1088 diag 0->0
  json        files 7->7 sym 53->53 rel 0->0 pend 0->0 id 0->0 facts 60->60 diag 0->0
  kotlin      files 99->99 sym 5080->5152 rel 462->470 pend 4537->4731 id 18633->17867 facts 763->763 diag 3->3
  markdown    files 7->7 sym 171->218 rel 0->29 pend 0->0 id 0->0 facts 179->208 diag 0->0
  toml        files 1->1 sym 69->69 rel 0->12 pend 0->0 id 0->0 facts 85->85 diag 0->0
  unsupported files 23->22 sym 0->0 rel 0->0 pend 0->0 id 0->0 facts 0->0 diag 0->0
  yaml        files 3->3 sym 71->84 rel 0->0 pend 0->0 id 0->0 facts 111->125 diag 0->0
zod old exit=0 secs=2.9 failed=0
zod new exit=0 secs=2.6 failed=0
lang files: symbols rels pending idents facts diags (old -> new)
  bash        files 1->1 sym 1->1 rel 0->0 pend 0->0 id 7->7 facts 1->1 diag 0->0
  css         files 2->2 sym 85->85 rel 10->20 pend 0->3 id 88->95 facts 74->83 diag 4->4
  html        files 2->2 sym 160->157 rel 0->1 pend 22->22 id 4->249 facts 68->82 diag 0->0
  javascript  files 6->6 sym 213->55 rel 10->10 pend 0->0 id 64->64 facts 1->1 diag 0->0
  json        files 41->42 sym 687->706 rel 0->96 pend 0->13 id 0->0 facts 881->1038 diag 5->5
  markdown    files 22->22 sym 1735->1966 rel 279->334 pend 0->1 id 0->0 facts 1733->1768 diag 0->0
  tsx         files 29->29 sym 322->375 rel 1->9 pend 8->2 id 673->673 facts 11->11 diag 0->0
  typescript  files 453->453 sym 23140->23487 rel 2231->3010 pend 2992->4437 id 121253->121197 facts 519->519 diag 21->21
  unsupported files 110->109 sym 0->0 rel 0->0 pend 0->0 id 0->0 facts 0->0 diag 0->0
  xml         files 1->1 sym 0->0 rel 0->0 pend 0->0 id 0->0 facts 1->1 diag 0->0
  yaml        files 11->11 sym 13323->15792 rel 0->3 pend 0->0 id 0->3 facts 23938->23991 diag 0->0
omarchy old exit=0 secs=2.5 failed=0
omarchy new exit=0 secs=3.1 failed=0
lang files: symbols rels pending idents facts diags (old -> new)
  bash        files 453->899 sym 6136->11231 rel 133->1203 pend 700->1532 id 30324->51199 facts 2784->4713 diag 94->111
  css         files 1->1 sym 9->9 rel 0->0 pend 0->0 id 12->12 facts 9->9 diag 0->0
  javascript  files 30->30 sym 2567->2202 rel 352->352 pend 258->0 id 7171->7171 facts 1->1 diag 0->0
  json        files 71->71 sym 25132->27062 rel 0->0 pend 0->0 id 0->0 facts 31625->31625 diag 1->1
  lua         files 75->75 sym 651->181 rel 9->26 pend 83->85 id 1523->1507 facts 470->500 diag 0->0
  markdown    files 90->90 sym 1062->1163 rel 2->2 pend 0->6 id 0->0 facts 1123->1135 diag 0->0
  python      files 5->5 sym 344->344 rel 35->35 pend 243->247 id 972->972 facts 17->17 diag 0->0
  qml         files 106->106 sym 9779->9779 rel 6794->6869 pend 5783->6217 id 50133->33200 facts 12222->12254 diag 0->0
  qmldir      files 3->3 sym 39->39 rel 0->0 pend 0->0 id 0->0 facts 39->39 diag 0->0
  toml        files 29->29 sym 726->726 rel 0->0 pend 0->0 id 0->0 facts 735->735 diag 0->0
  unsupported files 889->443 sym 0->0 rel 0->0 pend 0->0 id 0->0 facts 0->0 diag 0->0
  xml         files 1->1 sym 218->218 rel 0->0 pend 0->0 id 0->0 facts 1->1 diag 0->0
  yaml        files 4->4 sym 62->67 rel 0->0 pend 0->0 id 0->0 facts 88->88 diag 0->0
radzen-blazor old exit=0 secs=15.6 failed=0
radzen-blazor new exit=0 secs=14.4 failed=0
lang files: symbols rels pending idents facts diags (old -> new)
  csharp      files 1276->1276 sym 58461->58785 rel 5658->6463 pend 55101->53783 id 274462->275403 facts 37->64 diag 2216->2215
  css         files 13->13 sym 53404->53410 rel 9081->51972 pend 0->0 id 179776->179410 facts 53410->53410 diag 60->60
  html        files 1->1 sym 11->10 rel 0->0 pend 3->3 id 1->1 facts 2->3 diag 0->0
  javascript  files 3->3 sym 7570->6047 rel 163->375 pend 498->862 id 32762->32762 facts 13->13 diag 0->0
  json        files 13->13 sym 3138->3142 rel 0->0 pend 0->0 id 0->0 facts 3226->3226 diag 0->0
  razor       files 1477->1477 sym 14319->12216 rel 276->747 pend 0->23487 id 76385->90221 facts 25201->23658 diag 4489->4489
  unsupported files 196->196 sym 0->0 rel 0->0 pend 0->0 id 0->0 facts 0->0 diag 0->0
  xml         files 20->20 sym 4598->4519 rel 0->1 pend 0->15 id 24->90 facts 40->189 diag 0->0
  yaml        files 5->5 sym 142->168 rel 0->0 pend 0->0 id 0->0 facts 215->238 diag 0->0
```
