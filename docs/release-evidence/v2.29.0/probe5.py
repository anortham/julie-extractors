import json, os, shutil, subprocess, sys, time
from pathlib import Path

OLD = "/Users/murphy/source/miller/.tools/julie-extract"
NEW = "/Users/murphy/source/julie-extractors/.worktrees/row-level-scoping/target/release/julie-extract"
SP = Path(os.environ["SP"])
WORK = SP / "probe5"
FIXTURE_SRC = "/Users/murphy/source/miller"
NAMED_FILES = [
    "src/Miller.Indexing/JulieExtractRunner.cs",
    "src/Miller.Server/Tools/SearchTool.cs",
]
RUNS = 3


def rr_facts(rep):
    rr = (rep.get("languages") or {}).get("reference_resolution") or {}
    rows = (rr.get("counts") or {}).get("identifier_resolutions", 0)
    prows = (rr.get("counts") or {}).get("pending_resolutions", 0)
    phases = rep.get("profile", {}).get("phases", {})
    res_ms = None
    for k, v in phases.items():
        if "resolution" in k:
            res_ms = v
    return rows, prows, res_ms


def clone(base, work):
    for s in ("", "-wal", "-shm"):
        t = Path(str(work) + s)
        if t.exists():
            t.unlink()
    if subprocess.run(["cp", "-c", str(base), str(work)], capture_output=True).returncode != 0:
        shutil.copyfile(base, work)


def main():
    WORK.mkdir(parents=True, exist_ok=True)
    fixture = WORK / "fixture"
    if not fixture.exists():
        shutil.copytree(FIXTURE_SRC, fixture, ignore=shutil.ignore_patterns(
            ".git", ".miller", ".claude", ".worktrees", ".tools", "bin", "obj",
            "node_modules", ".memories", ".razorback", "spike"))
    base = WORK / "base.db"
    if not base.exists():
        t = time.monotonic()
        p = subprocess.run([OLD, "scan", "--root", str(fixture), "--db", str(base),
                            "--strict-schema", "--json", "--jobs", "4"],
                           capture_output=True, text=True)
        if p.returncode != 0:
            print("FATAL base scan", p.returncode, p.stderr[:400])
            sys.exit(2)
        print(f"base scan {round(time.monotonic()-t,1)}s")
    out = []
    for rel in NAMED_FILES:
        target = fixture / rel
        for label, binary in (("old-2.28.0", OLD), ("new-row-scoped", NEW)):
            for run in range(RUNS):
                work = WORK / f"work-{label}.db"
                clone(base, work)
                with open(target, "ab") as h:
                    h.write(b"\n")
                t = time.monotonic()
                p = subprocess.run([binary, "update", "--root", str(fixture),
                                    "--db", str(work), "--file", rel, "--json"],
                                   capture_output=True, text=True)
                wall = int((time.monotonic() - t) * 1000)
                if p.returncode != 0:
                    print("FAIL", label, rel, p.returncode, p.stderr[:400])
                    sys.exit(2)
                rep = json.loads(p.stdout)
                rows, prows, res_ms = rr_facts(rep)
                r = {"file": rel, "binary": label, "run": run, "wall_ms": wall,
                     "resolution_ms": res_ms, "identifier_resolutions": rows,
                     "pending_resolutions": prows}
                out.append(r)
                print(json.dumps(r))
    (WORK / "results5.json").write_text(json.dumps(out, indent=1))


main()
