# 3.7.2 published; closeout needs restored access

## WHAT

User authorized merge and release. Main fast-forwarded to 4dda4177 and pushed; CI 36631905617 passed. Release Binaries 36633807125 passed six builds and publication. GitHub connector confirms v3.7.2 published 2026-09-29T21:57:31Z with six uploaded assets, neither draft nor prerelease, tag at 4dda4177.

## WHY

Session permissions changed during monitoring: terminal network is restricted and .git is read-only. Terminal GitHub requests fail. Goldfish checkpoint tool also refused because approval is required while policy is never; this file preserves the checkpoint directly.

## HOW

Manual dispatch used verified main and explicit version 3.7.2. Connector reads confirmed publication and tag. Updated docs/release.md pointer and prepared docs/release-evidence/2026-09-29-v3-7-2-release.md. Raw metadata and verify-published.py are under target/release-readiness/3.7.2.

## IMPACT

Release is live. Public archives have not yet been downloaded or independently checksum-verified. Local main is 4dda4177; publication tag has not been fetched. Docs and memory are uncommitted because .git writes are prohibited.

## NEXT

Restore terminal network and .git write access. Download six archives; run verify-published.py; replay compatibility using the public Linux binary; fetch tag; update publication evidence with actual results. Commit docs and this memory as documentation-only release closeout, push main, and verify primary checkout is clean and current. Do not retag or claim checksum verification before it runs.

## RESOLVED

A later session restored access, fetched the tag, downloaded all six archives, passed verify-published.py (6 assets), and passed compat-check with the public Linux binary (18 tables byte-identical). Evidence doc updated; docs and this memory committed as the release closeout.
