# Language gap wave 1: real-repository scan comparison

Date: 2026-09-22. Plan: [language gap closure](../plans/2026-09-22-language-gap-closure.md).

Each repository under `~/source` was scanned twice into a fresh artifact: with the
published 3.3.1 release binary (built at `ea0bd76a`) and with the 3.4.0 release
build at `0ac8705c`. Columns count rows per language, old -> new. Every scan exited
0 with no failed files.

## Findings

- No crash, no failed file, and no scan-time regression beyond noise. omarchy
  takes 0.7 s longer because it now indexes 446 more shell scripts.
- Extensionless shell scripts and `.bats` files are now `bash`: omarchy bash
  files 453 -> 899 (unsupported 889 -> 443), jq 2 -> 14, moshi `gradlew`.
  The report prints a language present in only one scan in the old columns;
  moshi's `bash 1->` line is bash 0 -> 1.
- omarchy JavaScript pending 258 -> 0: the old binary emitted a resolved call
  edge and a duplicate pending row when the callee name also appeared as an
  exported object key. The edges are unchanged (352); the duplicates are gone.
- radzen-blazor CSS relationships 9081 -> 51942: rules now emit `references`
  edges to the custom properties their `var(--x)` values read.
- Razor gains pending relationships (0 -> 24706 in radzen-blazor), matching the
  new capability flag.
- flask HTML parse diagnostics 9 -> 17: inline `<script>` blocks now run the
  JavaScript pipeline, so Jinja syntax inside them reports JavaScript parse
  errors at host coordinates.
- C# relationships drop 3-7% (Newtonsoft.Json, radzen-blazor): `nameof` rows
  and generic callee `type_usage` rows are gone, as declared in the ledger.

## Report

```text
jq old exit=0 secs=0.9 failed=0
jq new exit=0 secs=1.0 failed=0
lang files: symbols rels pending idents facts diags (old -> new)
  bash        files 2->14 sym 24->149 rel 0->0 pend 0->3 id 98->1475 facts 12->77 diag 0->0
  c           files 47->47 sym 6481->6240 rel 2043->2195 pend 7343->7181 id 31036->31027 facts 947->947 diag 499->499
  cpp         files 2->2 sym 23->26 rel 0->0 pend 59->55 id 99->95 facts 0->0 diag 0->0
  css         files 1->1 sym 27->27 rel 0->0 pend 0->0 id 30->30 facts 27->27 diag 0->0
  javascript  files 1->1 sym 17->14 rel 0->3 pend 3->0 id 91->91 facts 0->0 diag 0->0
  json        files 9->9 sym 57->63 rel 0->0 pend 0->0 id 0->0 facts 97->97 diag 1->1
  markdown    files 5->5 sym 108->111 rel 0->0 pend 0->0 id 0->0 facts 109->105 diag 0->0
  python      files 5->5 sym 186->178 rel 61->60 pend 160->162 id 623->623 facts 1->1 diag 0->0
  unsupported files 316->304 sym 0->0 rel 0->0 pend 0->0 id 0->0 facts 0->0 diag 0->0
  yaml        files 19->19 sym 7042->9359 rel 0->7 pend 0->0 id 0->7 facts 11603->11669 diag 0->0
nlohmann-json old exit=0 secs=4.1 failed=0
nlohmann-json new exit=0 secs=4.5 failed=0
lang files: symbols rels pending idents facts diags (old -> new)
  bash        files 1->1 sym 2->2 rel 0->0 pend 0->0 id 8->8 facts 2->2 diag 0->0
  c           files 2->2 sym 22->22 rel 2->2 pend 13->13 id 50->50 facts 1->1 diag 1->1
  cpp         files 499->499 sym 21446->23197 rel 3948->5821 pend 32736->53677 id 147869->143432 facts 1465->1465 diag 8217->8192
  css         files 1->1 sym 1->1 rel 0->0 pend 0->0 id 2->2 facts 1->1 diag 0->0
  html        files 2->2 sym 6->6 rel 0->0 pend 0->0 id 0->0 facts 6->6 diag 0->0
  json        files 8->8 sym 52->52 rel 0->0 pend 0->0 id 0->0 facts 66->66 diag 0->0
  lua         files 1->1 sym 0->0 rel 0->0 pend 0->0 id 7->7 facts 0->0 diag 0->0
  markdown    files 272->272 sym 9372->9237 rel 36->13 pend 0->0 id 0->0 facts 8237->9214 diag 10->10
  python      files 7->7 sym 457->445 rel 48->45 pend 347->353 id 1515->1515 facts 0->0 diag 0->0
  sql         files 1->1 sym 6->6 rel 0->0 pend 0->0 id 3->238 facts 241->241 diag 0->0
  swift       files 1->1 sym 2->2 rel 0->0 pend 1->1 id 14->14 facts 0->0 diag 0->0
  unsupported files 426->426 sym 0->0 rel 0->0 pend 0->0 id 0->0 facts 0->0 diag 0->0
  yaml        files 23->23 sym 1428->1891 rel 0->0 pend 0->0 id 0->0 facts 2342->2508 diag 0->0
cobra old exit=0 secs=0.4 failed=0
cobra new exit=0 secs=0.4 failed=0
lang files: symbols rels pending idents facts diags (old -> new)
  go          files 36->36 sym 3589->3589 rel 658->619 pend 3218->3274 id 17397->17397 facts 53->53 diag 0->0
  json        files 6->6 sym 45->45 rel 0->0 pend 0->0 id 0->0 facts 51->51 diag 0->0
  markdown    files 17->17 sym 372->378 rel 1->1 pend 0->0 id 0->0 facts 374->374 diag 0->0
  unsupported files 15->15 sym 0->0 rel 0->0 pend 0->0 id 0->0 facts 0->0 diag 0->0
  yaml        files 5->5 sym 134->165 rel 0->0 pend 0->0 id 0->0 facts 232->251 diag 0->0
express old exit=0 secs=0.4 failed=0
express new exit=0 secs=0.5 failed=0
lang files: symbols rels pending idents facts diags (old -> new)
  css         files 4->4 sym 13->13 rel 0->0 pend 0->0 id 3->3 facts 13->13 diag 0->0
  html        files 8->8 sym 24->24 rel 0->0 pend 2->2 id 1->1 facts 3->3 diag 24->24
  javascript  files 141->141 sym 5436->5231 rel 290->280 pend 172->1184 id 21818->21818 facts 7->7 diag 0->0
  json        files 8->8 sym 121->121 rel 0->0 pend 0->0 id 0->0 facts 137->137 diag 0->0
  markdown    files 4->4 sym 197->502 rel 15->15 pend 0->0 id 0->0 facts 495->495 diag 0->0
  unsupported files 56->56 sym 0->0 rel 0->0 pend 0->0 id 0->0 facts 0->0 diag 0->0
  yaml        files 6->6 sym 259->300 rel 0->2 pend 0->0 id 0->2 facts 406->447 diag 0->0
flask old exit=0 secs=0.5 failed=0
flask new exit=0 secs=0.5 failed=0
lang files: symbols rels pending idents facts diags (old -> new)
  bash        files 1->1 sym 2->2 rel 0->0 pend 0->0 id 6->6 facts 1->1 diag 0->0
  css         files 2->2 sym 26->26 rel 0->0 pend 0->0 id 19->19 facts 26->26 diag 0->0
  html        files 20->20 sym 138->136 rel 0->1 pend 4->10 id 49->193 facts 59->59 diag 9->17
  json        files 10->10 sym 72->72 rel 0->0 pend 0->0 id 0->0 facts 86->86 diag 0->0
  markdown    files 6->6 sym 17->23 rel 0->0 pend 0->0 id 0->0 facts 18->18 diag 0->0
  python      files 83->83 sym 5801->5755 rel 250->210 pend 3588->3693 id 15419->15419 facts 715->975 diag 0->0
  sql         files 2->2 sym 11->11 rel 1->1 pend 0->0 id 0->2 facts 13->13 diag 2->2
  toml        files 5->5 sym 233->233 rel 19->19 pend 0->1 id 0->0 facts 242->282 diag 0->0
  unsupported files 113->113 sym 0->0 rel 0->0 pend 0->0 id 0->0 facts 0->0 diag 0->0
  yaml        files 8->8 sym 227->271 rel 0->2 pend 0->0 id 0->2 facts 414->451 diag 0->0
gson old exit=0 secs=1.4 failed=0
gson new exit=0 secs=1.2 failed=0
lang files: symbols rels pending idents facts diags (old -> new)
  java        files 264->264 sym 15621->15621 rel 1680->1728 pend 20650->21608 id 59292->59292 facts 3798->3798 diag 0->0
  json        files 8->8 sym 111->129 rel 0->0 pend 0->0 id 0->0 facts 137->137 diag 0->0
  markdown    files 16->16 sym 391->393 rel 37->37 pend 0->0 id 0->0 facts 392->392 diag 0->0
  unsupported files 23->23 sym 0->0 rel 0->0 pend 0->0 id 0->0 facts 0->0 diag 0->0
  xml         files 8->8 sym 2->18 rel 0->0 pend 0->14 id 0->0 facts 26->111 diag 0->0
  yaml        files 8->8 sym 300->346 rel 0->0 pend 0->0 id 0->0 facts 464->511 diag 0->0
Newtonsoft.Json old exit=0 secs=5.4 failed=0
Newtonsoft.Json new exit=0 secs=5.4 failed=0
lang files: symbols rels pending idents facts diags (old -> new)
  csharp      files 945->945 sym 38068->38068 rel 3083->2865 pend 44592->44780 id 196893->194566 facts 9->9 diag 857->857
  json        files 29->29 sym 1166->1469 rel 1->1 pend 0->0 id 0->0 facts 1855->1855 diag 0->0
  markdown    files 5->5 sym 55->70 rel 0->0 pend 0->0 id 0->0 facts 55->55 diag 0->0
  powershell  files 5->5 sym 417->237 rel 25->39 pend 138->163 id 1113->1135 facts 106->106 diag 47->47
  unsupported files 186->186 sym 0->0 rel 0->0 pend 0->0 id 0->0 facts 0->0 diag 0->0
  xml         files 9->9 sym 4->14 rel 0->0 pend 0->7 id 1->78 facts 21->169 diag 0->0
  yaml        files 2->2 sym 92->107 rel 0->0 pend 0->0 id 0->0 facts 138->145 diag 0->0
sinatra old exit=0 secs=0.7 failed=0
sinatra new exit=0 secs=0.7 failed=0
lang files: symbols rels pending idents facts diags (old -> new)
  json        files 6->6 sym 45->45 rel 0->0 pend 0->0 id 0->0 facts 51->51 diag 0->0
  markdown    files 12->12 sym 865->938 rel 91->91 pend 0->0 id 0->0 facts 864->866 diag 0->0
  ruby        files 147->147 sym 5860->5448 rel 1090->1620 pend 13040->12885 id 23198->23185 facts 4324->4349 diag 0->0
  unsupported files 127->127 sym 0->0 rel 0->0 pend 0->0 id 0->0 facts 0->0 diag 0->0
  yaml        files 10->10 sym 255->279 rel 3->3 pend 0->0 id 3->3 facts 512->528 diag 0->0
Alamofire old exit=0 secs=8.4 failed=0
Alamofire new exit=0 secs=8.9 failed=0
lang files: symbols rels pending idents facts diags (old -> new)
  css         files 4->4 sym 306->306 rel 0->0 pend 0->0 id 470->470 facts 306->306 diag 6->6
  html        files 332->332 sym 86660->86660 rel 0->0 pend 2324->2324 id 202610->202610 facts 69372->69372 diag 0->0
  javascript  files 6->6 sym 1998->1682 rel 124->286 pend 82->72 id 4986->4986 facts 0->0 diag 0->0
  json        files 27->27 sym 9980->10134 rel 0->0 pend 0->0 id 0->0 facts 12660->12660 diag 4->4
  markdown    files 12->12 sym 3143->3152 rel 38->38 pend 0->0 id 0->0 facts 3144->3144 diag 0->0
  swift       files 101->101 sym 10713->10757 rel 410->514 pend 8254->8699 id 44231->44208 facts 1171->1171 diag 25->25
  unsupported files 90->90 sym 0->0 rel 0->0 pend 0->0 id 0->0 facts 0->0 diag 0->0
  xml         files 1->1 sym 0->0 rel 0->0 pend 0->0 id 0->0 facts 1->1 diag 0->0
  yaml        files 5->5 sym 553->693 rel 0->0 pend 0->0 id 0->0 facts 786->820 diag 0->0
moshi old exit=0 secs=0.9 failed=0
moshi new exit=0 secs=1.0 failed=0
lang files: symbols rels pending idents facts diags (old -> new)
  bash        files 1-> sym 30-> rel 0-> pend 0-> id 82-> facts 9-> diag 1->
  java        files 57->57 sym 3949->3949 rel 487->490 pend 8673->8788 id 20053->20053 facts 1088->1088 diag 0->0
  json        files 7->7 sym 53->53 rel 0->0 pend 0->0 id 0->0 facts 60->60 diag 0->0
  kotlin      files 99->99 sym 5080->5085 rel 462->470 pend 4537->4731 id 18633->18715 facts 763->763 diag 3->3
  markdown    files 7->7 sym 171->208 rel 0->0 pend 0->0 id 0->0 facts 179->179 diag 0->0
  toml        files 1->1 sym 69->69 rel 0->0 pend 0->0 id 0->0 facts 85->85 diag 0->0
  unsupported files 23->22 sym 0->0 rel 0->0 pend 0->0 id 0->0 facts 0->0 diag 0->0
  yaml        files 3->3 sym 71->84 rel 0->0 pend 0->0 id 0->0 facts 111->125 diag 0->0
zod old exit=0 secs=2.9 failed=0
zod new exit=0 secs=3.1 failed=0
lang files: symbols rels pending idents facts diags (old -> new)
  bash        files 1->1 sym 1->1 rel 0->0 pend 0->0 id 7->7 facts 1->1 diag 0->0
  css         files 2->2 sym 85->85 rel 10->20 pend 0->3 id 88->88 facts 74->77 diag 4->4
  html        files 2->2 sym 160->159 rel 0->0 pend 22->22 id 4->247 facts 68->68 diag 0->0
  javascript  files 6->6 sym 213->213 rel 10->10 pend 0->0 id 64->64 facts 1->1 diag 0->0
  json        files 41->41 sym 687->690 rel 0->0 pend 0->0 id 0->0 facts 881->881 diag 5->5
  markdown    files 22->22 sym 1735->1724 rel 279->279 pend 0->0 id 0->0 facts 1733->1728 diag 0->0
  tsx         files 29->29 sym 322->357 rel 1->9 pend 8->2 id 673->673 facts 11->11 diag 0->0
  typescript  files 453->453 sym 23140->23328 rel 2231->2290 pend 2992->4242 id 121253->121253 facts 519->519 diag 21->21
  unsupported files 110->110 sym 0->0 rel 0->0 pend 0->0 id 0->0 facts 0->0 diag 0->0
  xml         files 1->1 sym 0->0 rel 0->0 pend 0->0 id 0->0 facts 1->1 diag 0->0
  yaml        files 11->11 sym 13323->13390 rel 0->3 pend 0->0 id 0->3 facts 23938->23991 diag 0->0
omarchy old exit=0 secs=2.6 failed=0
omarchy new exit=0 secs=3.3 failed=0
lang files: symbols rels pending idents facts diags (old -> new)
  bash        files 453->899 sym 6136->12235 rel 133->1202 pend 700->1817 id 30324->50862 facts 2784->4706 diag 94->111
  css         files 1->1 sym 9->9 rel 0->0 pend 0->0 id 12->12 facts 9->9 diag 0->0
  javascript  files 30->30 sym 2567->2566 rel 352->352 pend 258->0 id 7171->7171 facts 1->1 diag 0->0
  json        files 71->71 sym 25132->27062 rel 0->0 pend 0->0 id 0->0 facts 31625->31625 diag 1->1
  lua         files 75->75 sym 651->759 rel 9->17 pend 83->94 id 1523->1523 facts 470->470 diag 0->0
  markdown    files 90->90 sym 1062->1159 rel 2->2 pend 0->0 id 0->0 facts 1123->1123 diag 0->0
  python      files 5->5 sym 344->344 rel 35->35 pend 243->249 id 972->972 facts 17->17 diag 0->0
  qml         files 106->106 sym 9779->9779 rel 6794->6869 pend 5783->6169 id 50133->50133 facts 12222->12222 diag 0->0
  qmldir      files 3->3 sym 39->39 rel 0->0 pend 0->0 id 0->0 facts 39->39 diag 0->0
  toml        files 29->29 sym 726->726 rel 0->0 pend 0->0 id 0->0 facts 735->735 diag 0->0
  unsupported files 889->443 sym 0->0 rel 0->0 pend 0->0 id 0->0 facts 0->0 diag 0->0
  xml         files 1->1 sym 218->218 rel 0->0 pend 0->0 id 0->0 facts 1->1 diag 0->0
  yaml        files 4->4 sym 62->67 rel 0->0 pend 0->0 id 0->0 facts 88->88 diag 0->0
radzen-blazor old exit=0 secs=15.5 failed=0
radzen-blazor new exit=0 secs=15.6 failed=0
lang files: symbols rels pending idents facts diags (old -> new)
  csharp      files 1276->1276 sym 58461->58461 rel 5658->5460 pend 55101->54556 id 274462->267624 facts 37->37 diag 2216->2216
  css         files 13->13 sym 53404->53404 rel 9081->51942 pend 0->0 id 179776->179776 facts 53410->53410 diag 60->60
  html        files 1->1 sym 11->11 rel 0->0 pend 3->3 id 1->1 facts 2->2 diag 0->0
  javascript  files 3->3 sym 7570->6699 rel 163->303 pend 498->857 id 32762->32762 facts 13->13 diag 0->0
  json        files 13->13 sym 3138->3142 rel 0->0 pend 0->0 id 0->0 facts 3226->3226 diag 0->0
  razor       files 1477->1477 sym 14319->13263 rel 276->623 pend 0->24706 id 76385->91735 facts 25201->25209 diag 4489->4489
  unsupported files 196->196 sym 0->0 rel 0->0 pend 0->0 id 0->0 facts 0->0 diag 0->0
  xml         files 20->20 sym 4598->4615 rel 0->1 pend 0->8 id 24->114 facts 40->189 diag 0->0
  yaml        files 5->5 sym 142->168 rel 0->0 pend 0->0 id 0->0 facts 215->238 diag 0->0
```
