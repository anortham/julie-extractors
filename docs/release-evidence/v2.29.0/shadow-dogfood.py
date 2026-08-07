import json, os, re, shutil, subprocess, sys, time
from pathlib import Path

JULIE = "/Users/murphy/source/julie-extractors/.worktrees/row-level-scoping/target/release/julie-extract"
SP = Path(os.environ["SP"])
WORK = SP / "shadow-dogfood"

CORPORA = {
    "julie-rust": {
        "src": "/Users/murphy/source/julie-extractors/.worktrees/row-level-scoping/crates",
        "saves": 14,
        "renames": 6,
        "rename_re": re.compile(r"\b(pub fn|fn|pub struct|struct) ([a-zA-Z_][a-zA-Z0-9_]*)"),
    },
    "miller-csharp": {
        "src": "/Users/murphy/source/miller/src",
        "saves": 14,
        "renames": 6,
        "rename_re": re.compile(r"\b(public sealed class|public class|internal sealed class|internal class|public record|internal record) ([A-Za-z_][A-Za-z0-9_]*)"),
    },
}

EXTS = {".rs", ".cs"}


def run_update(root, db, rel, env_extra):
    env = dict(os.environ)
    env.update(env_extra)
    argv = [JULIE, "update", "--root", str(root), "--db", str(db), "--file", rel, "--json"]
    t = time.monotonic()
    p = subprocess.run(argv, capture_output=True, text=True, env=env)
    ms = int((time.monotonic() - t) * 1000)
    return p, ms


def main():
    WORK.mkdir(parents=True, exist_ok=True)
    results = {"total_saves": 0, "mismatches": 0, "corpora": {}}
    for name, cfg in CORPORA.items():
        croot = WORK / name
        if croot.exists():
            shutil.rmtree(croot)
        shutil.copytree(cfg["src"], croot, ignore=shutil.ignore_patterns(
            "target", "bin", "obj", ".git", "node_modules", ".worktrees"))
        db = WORK / f"{name}.db"
        for s in ("", "-wal", "-shm"):
            p = Path(str(db) + s)
            if p.exists():
                p.unlink()
        t = time.monotonic()
        scan = subprocess.run([JULIE, "scan", "--root", str(croot), "--db", str(db),
                               "--strict-schema", "--json", "--jobs", "4"],
                              capture_output=True, text=True)
        scan_s = round(time.monotonic() - t, 1)
        if scan.returncode != 0:
            print(f"FATAL scan {name}: rc={scan.returncode} {scan.stderr[:400]}")
            sys.exit(2)
        files = sorted(p for p in croot.rglob("*") if p.suffix in EXTS and p.is_file()
                       and p.stat().st_size > 400)
        step = max(1, len(files) // (cfg["saves"] + cfg["renames"]))
        picked = files[::step][: cfg["saves"] + cfg["renames"]]
        log = []
        n_renames = 0
        for i, path in enumerate(picked):
            rel = str(path.relative_to(croot))
            kind = "touch"
            if n_renames < cfg["renames"]:
                text = path.read_text(encoding="utf-8", errors="replace")
                m = cfg["rename_re"].search(text)
                if m:
                    old = m.group(2)
                    new = old + "Rl"
                    path.write_text(text.replace(m.group(0), f"{m.group(1)} {new}", 1),
                                    encoding="utf-8")
                    kind = f"rename {old}->{new}"
                    n_renames += 1
            if kind == "touch":
                with open(path, "ab") as h:
                    h.write(b"\n")
            p, ms = run_update(croot, db, rel, {"JULIE_RESOLUTION_SHADOW": "1"})
            entry = {"file": rel, "kind": kind, "exit": p.returncode, "ms": ms,
                     "stderr_bytes": len(p.stderr)}
            if p.returncode != 0 or p.stderr.strip():
                entry["stderr"] = p.stderr[:2000]
                results["mismatches"] += 1
            log.append(entry)
            results["total_saves"] += 1
            print(json.dumps(entry))
        results["corpora"][name] = {"scan_s": scan_s, "files_in_corpus": len(files),
                                    "saves": log}
    (WORK / "results.json").write_text(json.dumps(results, indent=1))
    print(f"TOTAL saves={results['total_saves']} mismatches={results['mismatches']}")
    sys.exit(0 if results["mismatches"] == 0 else 1)


main()
