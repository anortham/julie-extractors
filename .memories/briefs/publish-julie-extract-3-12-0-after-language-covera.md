---
id: publish-julie-extract-3-12-0-after-language-covera
title: Publish julie-extract 3.12.0 after language coverage
status: active
created: 2026-11-04T16:29:41.404Z
updated: 2026-11-04T17:34:25.961Z
tags: []
---

User authorized merging to main and publishing julie-extract3.12.0, including main/tag pushes, six-platform assets and necessary evidence-only closeout; no further approval needed. Work stays in primary /home/murphy/source/julie-extractors on main. Task worktree is clean d508db8c; other13extractor worktrees unrelated/clean. Main/origin currentlyd508; routing correction nowreadytocommit. No tag/release yet.

Original coveragegoal complete:42languages/630cells,47fixes,465goldens,all6controlledperformanceworkloads pass; Linux/Windows/escalation evidencevalid. Prepare9bpassed12releasegates. Required exactly-one code-kb dogfood round validatedPASS at privateverification8e2f2597; allmodel findings source-reviewed, no wronganswerblocker. Honest Python subscript-callback extraction gap recorded in downstream/open-gaps.json and named closure plan; not claimed fixed. User code-kb mainf290/toolkit571 untouched. Privateverification worktree /home/murphy/source/code-kb/.worktrees/julie-3.12.0-dogfood remainsclean/unpushed.

SourceCI41872663869 failed onlyanexisting default20K C#timingguard10.048s vs10. Isolateddebug3.19s andimmutableold/newreleaseCLI5k/10k/20k scale linearly;20kmedians1.504011/1.506782s. No productionregression. Fixedtimedstressroutingwithtest-perf:4largecasesplusnestedtiming runserialreleaseinactualperf tier;default retainsnested/ordinarycorrectness. Allassertions/counts/10sceilingspreserved. Newconventioncoversdefault+contract combinedflags/allfeatures. WorkerTDDredobserved,25tier/76defaultargument/81perfargument casespass. Root10affectedgatesallpass(default101s withbuilds,combinedperf7.81s,clippy,releasebuild,preflight,corpus,package). Rebuiltbinary remains445a4c00655dfef885596d5546b45639ec0bcf7ea6b9e685fa219c504e3f4734 byte-identicaltodogfoodcandidate;18tablesbyte-identical. Productioncodeunchanged,one-roundevidencestillvalid. Alloldfailure/diagnostic/fixlogsarchived understress-routing.

Next: commit ownedrouting+release evidence/memory, pushmain and waitcorrectedsourceCIgreen. Then verifyclean14worktrees/tagabsence,annotatepushv3.12.0,monitorall6releaseplatformjobs. Verifypublishedarchives with target/release-3.12.0/verify-assets.py (expects primarytarget/release-3.12.0/local-package), runLinuxcoverage andpublished-binary downstreamcorpus againstpubliccode-kb2.12baseline alreadyverified/extracted target/release-3.12.0/code-kb-v2.12.0. State.jsonholdsprocessrefs. UpdateCurrentpublishedreleaseonlyafterpublication; commitauthorizedevidence-onlyfollowupseparatefromtag. Keepprimarymainclean/synced. Noextra modelround or oldworktree deletion.
