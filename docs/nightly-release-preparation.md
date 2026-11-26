# Windows, Linux and macOS release-ref nightly: supervised manual go-live

**Enabled for supervised manual runs, not yet qualified.** The producer (`nightly.yml`, main) and the
signer (`nightly-sign.yml`, on the dedicated `nightly-signer` branch) keep only their exact context
guards and run when one of the reviewed manual actors dispatches them. There is no schedule yet. The
installed policy [`.github/nightly-signing-policy.json`](../.github/nightly-signing-policy.json) is the
v3 policy. Its `producer_workflow_id` is `nightly.yml`'s registration on `main`, read with:

```bash
gh api repos/storytold/cadcraft/actions/workflows/nightly.yml --jq .id
```

The callers pass no platform secrets. The unsigned reusable caller's maximum OIDC/attestation
permissions are for its isolated attester; its compiler narrows to contents and Actions read
only, with no OIDC, attestation write, release environment or platform signing secrets.
Shared tooling is pinned to `be00e5ccd18b68e950c8a08532f2230e7c3d0cba`, the craft-common-actions
main commit that merged [#371](https://github.com/storytold/craft-common-actions/pull/371) (the
candidate is admitted on the built commit's own Sonar analysis, not main's latest) after
[#370](https://github.com/storytold/craft-common-actions/pull/370) (the Windows and macOS nightly
registry rows for the 14 apps) and
[#369](https://github.com/storytold/craft-common-actions/pull/369) (shared helper checkouts kept out
of the app's clean-source check, and macOS on `macos-15`), following
[#364](https://github.com/storytold/craft-common-actions/pull/364) (the signer may run on the
policy's signer branch) and [#363](https://github.com/storytold/craft-common-actions/pull/363) (the
multi-app pipeline, which makes the signer's `app-name` input required and admits any reviewed
registry app), with #342 (`a4f6fd51`, policy v3 and the `platforms` input) and #339 (`605d5876`)
earlier. Both callers name it in `uses` and `tooling-revision`. A pin is reviewed source
availability, not provider, queue, native signing or crypto qualification.
Existing stable workflows are unchanged.

**First runs.** The first runs are manual dispatches in this order: `nightly.yml` on main (only a
listed manual actor passes the producer's plan step), then `nightly-sign.yml` on `nightly-signer`
with that producer run ID and attempt, by a listed manual actor.

**Signer branch.** The signer runs on `refs/heads/nightly-signer`, not on the stable `release`
branch, so nightly signer files never need a stable release sync. The policy's `release_branch`
names it and the signer's guard requires it; this app has no automatic dispatch transport, so
nothing else targets the branch. The branch only fast-forwards to reviewed main commits, through the
settings script's `advance` (no queued signer run, a trust-closure scan, then the full diff of
`.github/` and `packaging/`), and the existing `release` environment must admit `nightly-signer`
alongside `release` before the first dispatch (a repository settings change, not a workflow one).
Its trust closure is all of `.github/` and `packaging/`, including `packaging/env.sh`, which
`import-cert.sh` sources with the Apple keys. On that branch only the pinned `nightly-sign.yml`
caller may reach an environment: no app workflow binds one, and the only caller of the shared stable
`release.yml` is `release.yml`, whose callee must carry the stable-only environment guard. The
contract tests check the in-repo part on every main PR; the settings scan also resolves the callee.
Stable releases still come only from `refs/heads/release`. History does not carry over a switch
between `release` and `nightly-signer`, so the switch happens before the first publication.

**Manual actors.** `bflatastic`, `dylanmtaylor`, `echelon`, `FredApps` and `skylartaylor`, in the
producer's `manual-actors` and the policy's `manual_actors`. The list mirrors the release-managers
team (id 19837241), leaving out `echelon-robot`; update it by PR in both places when the team
changes.

**Admission timing.** The candidate check reads the newest CI run on the candidate commit and
main's latest finished Sonar analysis, so when to dispatch matters:
- The candidate must be main's latest finished Sonar analysis. Pick a quiet main, wait for its
  CI and SonarCloud analysis to finish green, then dispatch the producer and, once it succeeds,
  the signer, before another main push lands a newer analysis.

**Platform rows.** The producer caller lists
`platforms: '["windows-x86_64","linux-x86_64","macos-aarch64"]'` on its single job: one producer
run builds every listed platform for one all-or-nothing release (one failed or
skipped platform stops that night's release, Windows included), and the release-ref signer admits
platforms only from the protected policy, so there is no separate Linux or macOS caller job and
the signer caller has no platform input.

## Installed policy values and their sources

Read on 2026-11-24, with `producer_workflow_id` filled on 2026-11-25; re-read anything that may have moved before `nightly-signer` is advanced to it.

| Field | Value | Source |
|---|---|---|
| `repository_id` | `1408289866` | `gh api repos/storytold/cadcraft --jq .id` |
| `producer_workflow_id` | `429719786` | `gh api repos/storytold/cadcraft/actions/workflows/nightly.yml --jq .id` on `main` |
| `producer_tooling_revision`, `signer_tooling_revision`, `producer_workflow_ref` | `be00e5cc…` (the exact-analysis admission, #371 after #370 and #369, following #364 on #363) | The callers' pin |
| `manual_actors` / `automatic_signer_actors` | the five release managers / `[]` | release-managers team (id 19837241) members who can push; equals the producer caller's `manual-actors` |
| `max_age_seconds`, `max_artifact_bytes` | `86400`, `2147483648` | Go-live checklist |
| `platforms` | three rows | StoryTold / Learning Machines stable signing identities (`windows-x86_64`, `linux-x86_64`, `macos-aarch64`) |
| `candidate_policy.required_workflows` | CI (`ci.yml`, workflow `380135820`), `gate / gate (ubuntu-24.04)` | Workflow list API and recent main push runs |
| `candidate_policy.sonar` | `storytold` / `storytold_cadcraft`, app `12526` | `sonar-project.properties`; the SonarCloud check on main commits |
| Sonar `issue_types` / `debt` | `VULNERABILITY`; `[]` | Refresh before advancing `nightly-signer` if any unresolved main vulnerabilities exist |

The candidate check refuses whenever main's unresolved vulnerability set differs from `debt`, whether
an issue is new or fixed, and whenever a required condition fails or a listed CI job is renamed. Refresh
those values in the same reviewed main change that `nightly-signer` is then advanced to.

## Ownership and interface

- `.github/workflows/nightly.yml`: default-main registration of the unsigned producer, named
  `CADCraft unsigned nightly`, enabled for manual dispatch by `manual-actors`; no signing secrets
  may be passed or inherited.
- `.github/workflows/nightly-sign.yml`: top-level signer registration, enabled for manual dispatch on
  `nightly-signer`, which carries a reviewed main commit. It rejects wrong repository/event/actual branch,
  including same-name tags. Its constant whole-run concurrency (`cadcraft-nightly-release`) is distinct from stable,
  cancellation is off and `queue: max` must be hosted-qualified. The caller itself passes no
  credentials; only the shared callee's sign jobs bind the `release` environment.
- Shared reusable entrypoints are `nightly-unsigned.yml` and `nightly-release.yml` in
  `storytold/craft-common-actions`. Both must resolve to a **real reviewed public immutable
  commit**; the defining workflow SHA and tooling input must match.
- Ported from `storytold/photocraft#16594` (authored by `@skylartaylor`).

## Local regression checks

`python3 packaging/nightly-release-tests.py` verifies:
- Both callers share one immutable 40-hex commit pin in `uses` and `tooling-revision`.
- Both callers keep their exact context guards (every `if:` form and continuation line is read).
- No nightly schedule is registered.
- No cache actions, cache setup options, `environment:`, or `secrets` appear in the callers.
- Signer root serialization (`cadcraft-nightly-release`, `cancel-in-progress: false`) stays literal.
- The signer runs on `nightly-signer`: its guard, the policy's `release_branch` and the caller's
  `app-name` agree, and the trust-closure scan finds no other route to an environment.
- Manual actors are exactly the five release managers in both `nightly.yml` and `.github/nightly-signing-policy.json`.
- The installed policy's v3 schema, repository identity, platforms, bounds, required CI workflow, and SonarCloud project identity match the repository and callers.
