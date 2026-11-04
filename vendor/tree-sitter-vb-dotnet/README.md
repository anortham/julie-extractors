# tree‑sitter‑vb‑dotnet

[![npm version](https://img.shields.io/npm/v/tree-sitter-vb-dotnet.svg)](https://www.npmjs.com/package/tree-sitter-vb-dotnet)
[![license](https://img.shields.io/github/license/CodeAnt-AI/tree-sitter-vb-dotnet)](LICENSE)

> **Tree‑sitter grammar for Visual Basic .NET (VB.NET)** – incremental parsing for editors, code‑intel, refactoring tools and static‑analysis pipelines.

---

## Status

| Feature | Support |
|---------|---------|
| Modules / Classes / Structures / Interfaces | ✅ |
| Subs & Functions (incl. overloads, generics) | ✅ |
| Properties (auto / get‑set) | ✅ |
| Events & Delegates | ✅ |
| Control‑flow (`If…Else`, `Select Case`, loops, `Try…Catch`) | ✅ |
| LINQ / XML literals | ⚠️ *planned* |
| Preprocessor directives | ⚠️ parses as trivia only |
| Error recovery | ⚠️ basic |

The grammar aims to cover **VB 16.9 / .NET 5** syntax.  
Bug reports / PRs are very welcome!

---

## Installation

```bash
npm install tree-sitter tree-sitter-vb-dotnet   # parser + runtime
