# GitHub Connector Capability Audit

**Date:** 2026-07-30  
**Repository:** `manilka-gl/ntfy-windows-client`  
**Audit branch:** `connector-capability-audit-20260730`  
**Disposable issue:** `#13` (closed)  
**Disposable pull request:** `#14` (draft during testing; closed without merge at audit completion)

## Executive conclusion

The connector exposes 89 methods. It is strong for repository discovery, text-file changes, Git object composition, issues, pull requests, reviews, and read-only GitHub Actions inspection. It does **not** expose native clone/push, release upload, Git LFS, workflow dispatch/cancel, runner management, or a write method that accepts a sandbox file reference.

A reliable new 5–10 MB sandbox-to-GitHub ZIP upload is **not available** through the current surface:

- `create_file` and `update_file` accept complete UTF-8 text embedded in the tool request, not a local file.
- Base64 would expand a binary by roughly one third and still require the entire encoded payload to pass through the model/tool message.
- `create_blob`, the appropriate Git Data method for base64 binary content, was blocked by the platform safety layer for both UTF-8 and base64 tests.
- The sandbox has no `gh` command, no connector token exposure, and no outbound DNS/network access for `git push`.
- No GitHub method accepts the file reference returned by `download_workflow_artifact`.

What **is** possible:

- GitHub-to-sandbox artifact download works. A 5,276,942-byte Actions ZIP was downloaded and its SHA-256 matched the artifact digest.
- Existing Git blobs can be re-linked under another path without retransmitting bytes. A committed 5.28 MB ZIP blob was linked into the audit branch through `create_tree`/`create_commit`/`update_ref`, verified, then its duplicate path was removed.
- Small and moderate UTF-8 files can be created or replaced through the contents API.
- A purpose-built connector action with a file-valued parameter, authenticated `git push`, Release asset upload, or Git LFS support is required for reliable new multi-megabyte uploads.

## Repository and safety observations

- The connector reports `admin`, `maintain`, `push`, `pull`, and `triage` permission on the target repository.
- The target workflow `.github/workflows/continuous-worker.yml` is a long-running Windows worker with a 350-minute timeout and a push trigger on `.continuous-worker/start.txt`. It was not triggered merely for testing because that could consume hours of Actions capacity.
- The safety classifier is payload-sensitive. Richer text in one `update_file` call and the first `create_pull_request` call was blocked, while simpler payloads for the same methods succeeded.
- There is no branch deletion method in the exposed surface, so the requested audit branch remains.
- Large responses can be truncated. Full PR diffs and workflow logs exceeded the connector response budget; targeted filename and per-file patch methods are more reliable.

## Workflow and runner coverage

Available and confirmed:

- Resolve PR-associated workflow runs from a commit.
- List jobs and step summaries.
- Fetch decoded job logs.
- List workflow artifacts.
- Download an artifact ZIP into the sandbox.
- Exposed retry methods exist for failed runs and individual jobs.

Not exposed:

- List arbitrary/all workflow runs.
- Dispatch a workflow.
- Cancel or delete a workflow run.
- Enable/disable workflow files.
- Inspect or manage hosted/self-hosted runners.
- Register/remove runners or runner groups.
- Manage Actions secrets, variables, environments, caches, or concurrency.
- Upload artifacts from the sandbox.

The two retry methods were not invoked because they would create new runner work and consume Actions capacity.

## Large-file transfer evaluation

| Direction or method | Result |
|---|---|
| GitHub Actions artifact → sandbox | **Confirmed**, 5,276,942-byte ZIP downloaded and hash verified |
| Existing Git blob → another path in same repository | **Confirmed**, 5.28 MB blob re-linked without transmitting bytes |
| Sandbox ZIP → `create_file` as binary | **Not supported**, text-only wrapper |
| Sandbox ZIP → base64 text file | Theoretically possible only by embedding the entire string; not reliable for 5–10 MB |
| Sandbox ZIP → `create_blob(base64)` | **Blocked by platform safety layer** |
| Sandbox → authenticated `git push` | **Unavailable**, no credentials/CLI/network bridge |
| Sandbox file reference → GitHub write method | **Unavailable**, no write schema accepts a file parameter |
| Release asset or Git LFS upload | **No exposed method** |

## Method-by-method results

Status meanings:

- **Confirmed:** invoked successfully.
- **Partial:** useful subset works, but an important mode failed.
- **Reachable:** connector reached GitHub, but the chosen fixture or repository setting made the operation invalid.
- **Blocked:** stopped before GitHub.
- **Not invoked:** deliberately skipped because of cost, side effects, or missing fixture.
- **Unreliable:** returned a demonstrably incorrect empty result in this audit.

### Identity, installation, and repositories
| Method | Status | Observation |
|---|---|---|
| `get_profile` | Confirmed | Invoked successfully during this audit. |
| `get_user_login` | Confirmed | Invoked successfully during this audit. |
| `list_installations` | Confirmed | Invoked successfully during this audit. |
| `list_installed_accounts` | Confirmed | Invoked successfully during this audit. |
| `list_user_orgs` | Confirmed | Invoked successfully during this audit. |
| `list_user_org_memberships` | Confirmed | Invoked successfully during this audit. |
| `list_repositories` | Confirmed | Invoked successfully during this audit. |
| `list_repositories_by_affiliation` | Confirmed | Invoked successfully during this audit. |
| `list_repositories_by_installation` | Confirmed | Invoked successfully during this audit. |
| `get_repo` | Confirmed | Invoked successfully during this audit. |
| `get_repo_collaborator_permission` | Confirmed | Invoked successfully during this audit. |
| `search_repositories` | Confirmed | Invoked successfully during this audit. |
| `search_installed_repositories_v2` | Confirmed | Invoked successfully during this audit. |
| `search_installed_repositories_streaming` | Unreliable in observed test | Returned no result for the exact installed repository; search_installed_repositories_v2 found it. |

### Search and history
| Method | Status | Observation |
|---|---|---|
| `search` | Confirmed | Invoked successfully during this audit. |
| `search_branches` | Unreliable in observed test | Returned no result for a branch that had just been created and was demonstrably accessible. |
| `search_commits` | Confirmed | Invoked successfully during this audit. |
| `search_issues` | Confirmed | Invoked successfully during this audit. |
| `search_prs` | Confirmed | Invoked successfully during this audit. |
| `list_recent_issues` | Confirmed | Invoked successfully during this audit. |
| `get_users_recent_prs_in_repo` | Confirmed | Invoked successfully during this audit. |

### Git data, branches, commits, and files
| Method | Status | Observation |
|---|---|---|
| `create_branch` | Confirmed | Invoked successfully during this audit. |
| `compare_commits` | Confirmed | Invoked successfully during this audit. |
| `fetch_commit` | Confirmed | Invoked successfully during this audit. |
| `get_commit_combined_status` | Confirmed but narrow | Call succeeded and returned no legacy statuses even though an Actions run existed; use workflow-run methods for Actions checks. |
| `create_blob` | Blocked by platform | Both UTF-8 and base64 calls were blocked before reaching GitHub. This prevents the normal new-binary Git Data upload path. |
| `fetch_blob` | Partial | UTF-8 blob fetch succeeded. Fetching a ZIP blob failed with UnicodeDecodeError because the wrapper attempted UTF-8 decoding. |
| `create_tree` | Confirmed | Invoked successfully during this audit. |
| `create_commit` | Confirmed | Invoked successfully during this audit. |
| `update_ref` | Confirmed | Invoked successfully during this audit. |
| `create_file` | Confirmed | Invoked successfully during this audit. |
| `fetch_file` | Partial | UTF-8 and line-range reads succeeded. A 5.28 MB ZIP with encoding=base64 returned the SHA but an empty content field. |
| `fetch` | Confirmed | Invoked successfully during this audit. |
| `update_file` | Confirmed with payload sensitivity | A richer replacement was blocked before GitHub; a minimal replacement succeeded. |
| `delete_file` | Confirmed | Invoked successfully during this audit. |

### Issues and conversation
| Method | Status | Observation |
|---|---|---|
| `create_issue` | Confirmed | Invoked successfully during this audit. |
| `fetch_issue` | Confirmed | Invoked successfully during this audit. |
| `fetch_issue_comments` | Confirmed | Invoked successfully during this audit. |
| `update_issue` | Confirmed | Invoked successfully during this audit. |
| `add_comment_to_issue` | Confirmed | Invoked successfully during this audit. |
| `update_issue_comment` | Confirmed | Invoked successfully during this audit. |
| `add_issue_assignees` | Confirmed | Invoked successfully during this audit. |
| `remove_issue_assignees` | Confirmed | Invoked successfully during this audit. |
| `add_issue_labels` | Confirmed | Invoked successfully during this audit. |
| `remove_issue_label` | Confirmed | Invoked successfully during this audit. |
| `lock_issue_conversation` | Confirmed | Invoked successfully during this audit. |
| `unlock_issue_conversation` | Confirmed | Invoked successfully during this audit. |
| `add_reaction_to_issue_comment` | Confirmed | Invoked successfully during this audit. |
| `get_issue_comment_reactions` | Confirmed | Invoked successfully during this audit. |
| `remove_reaction_from_issue_comment` | Confirmed | Invoked successfully during this audit. |

### Pull requests and review
| Method | Status | Observation |
|---|---|---|
| `create_pull_request` | Confirmed with payload sensitivity | A richer body was blocked by the platform safety layer; a minimal draft PR payload succeeded. |
| `get_pr_info` | Confirmed | Invoked successfully during this audit. |
| `fetch_pr` | Confirmed | Invoked successfully during this audit. |
| `list_pr_changed_filenames` | Confirmed | Invoked successfully during this audit. |
| `fetch_pr_file_patch` | Confirmed | Invoked successfully during this audit. |
| `fetch_pr_patch` | Confirmed | Invoked successfully during this audit. |
| `get_pr_diff` | Confirmed | Invoked successfully during this audit. |
| `fetch_pr_comments` | Confirmed | Invoked successfully during this audit. |
| `update_pull_request` | Confirmed with field limitation | Metadata update succeeded after removing maintainer_can_modify; GitHub returns 422 for that field on same-repository PRs. |
| `mark_pull_request_ready_for_review` | Confirmed | Invoked successfully during this audit. |
| `convert_pull_request_to_draft` | Confirmed | Invoked successfully during this audit. |
| `label_pr` | Confirmed | Invoked successfully during this audit. |
| `add_reaction_to_pr` | Confirmed | Invoked successfully during this audit. |
| `get_pr_reactions` | Confirmed | Invoked successfully during this audit. |
| `remove_reaction_from_pr` | Confirmed | Invoked successfully during this audit. |
| `add_review_to_pr` | Confirmed | Invoked successfully during this audit. |
| `list_pull_request_reviews` | Confirmed | Invoked successfully during this audit. |
| `list_pull_request_review_threads` | Confirmed | Invoked successfully during this audit. |
| `update_review_comment` | Confirmed | Invoked successfully during this audit. |
| `reply_to_review_comment` | Confirmed | Invoked successfully during this audit. |
| `add_reaction_to_pr_review_comment` | Confirmed | Invoked successfully during this audit. |
| `get_pr_review_comment_reactions` | Confirmed | Invoked successfully during this audit. |
| `remove_reaction_from_pr_review_comment` | Confirmed | Invoked successfully during this audit. |
| `resolve_review_thread` | Confirmed | Invoked successfully during this audit. |
| `unresolve_review_thread` | Confirmed | Invoked successfully during this audit. |
| `dismiss_pull_request_review` | Reachable; fixture ineligible | GitHub GraphQL rejected dismissal because COMMENTED reviews cannot be dismissed. |
| `request_pull_request_reviewers` | Reachable; fixture ineligible | GitHub rejected requesting review from the PR author. |
| `remove_pull_request_reviewers` | Confirmed as no-op | GitHub accepted removal when no matching reviewer request existed. |
| `enable_auto_merge` | Reachable; repository setting disabled | The method reached the connector but failed because auto-merge is disabled for this repository. |
| `merge_pull_request` | Reachable; safely rejected | Invoked on the disposable draft PR and GitHub rejected it with 405; nothing was merged. |

### GitHub Actions and downloads
| Method | Status | Observation |
|---|---|---|
| `fetch_commit_workflow_runs` | Confirmed | Invoked successfully during this audit. |
| `fetch_workflow_run_jobs` | Confirmed | Invoked successfully during this audit. |
| `fetch_workflow_job_steps` | Confirmed | Invoked successfully during this audit. |
| `fetch_workflow_job_logs` | Confirmed with truncation | Logs were fetched, but the connector truncated the 1,460-line response to fit its output budget. |
| `fetch_workflow_run_artifacts` | Confirmed | Invoked successfully during this audit. |
| `download_workflow_artifact` | Confirmed | Invoked successfully during this audit. |
| `rerun_failed_workflow_run_jobs` | Not invoked to avoid cost | Would create a new Actions attempt and consume runner time; schema requires Actions write permission and a run ID. |
| `rerun_workflow_job` | Not invoked to avoid cost | Would re-run a job and consume runner time; schema requires Actions write permission and a job ID. |
| `download_user_content` | Not invoked | No private-user-images.githubusercontent.com fixture was available; method is restricted to that URL class. |

## Important limitations by API family

### Contents API

`create_file`, `update_file`, and `delete_file` are practical for complete UTF-8 text files. Updates require the current blob SHA, and sequential writes to the same path must not be parallelized. The wrappers do not accept raw bytes or a local file reference.

### Git Data API

`create_tree`, `create_commit`, and `update_ref` are effective for multi-file atomic commits when blob SHAs already exist. `create_tree` accepted the current commit SHA as `base_tree_sha` in this audit. New binary creation remains blocked because `create_blob` could not be invoked.

### Binary reads

`fetch_file(..., encoding="base64")` returned the correct SHA but an empty content field for a 5.28 MB ZIP. `fetch_blob` attempted UTF-8 decoding and raised `UnicodeDecodeError`. Binary repository download is therefore not dependable through these two methods. Actions artifact download is dependable.

### Pull requests and reviews

The connector supports draft transitions, metadata edits, labels, comments, reactions, top-level reviews, inline comments, replies, edits, and thread resolution. GitHub’s normal rules still apply: the author cannot be requested as reviewer, COMMENTED reviews cannot be dismissed, same-repository PRs cannot use fork-collaboration settings, and draft PRs cannot be merged.

### Search

`search_installed_repositories_v2` located the target repository, while `search_installed_repositories_streaming` returned no result for the same repository. `search_branches` also failed to return the newly created branch. These two search methods should not be used as sole existence checks.

## Recommended reliable design for 5–10 MB uploads

The connector should add one of these, in preference order:

1. A GitHub write action with a top-level **file parameter**, accepting a sandbox path/file reference and internally streaming to GitHub.
2. Authenticated `git push` from the sandbox, ideally with short-lived installation credentials and no token exposure to the model.
3. Release asset upload with a file parameter for build artifacts that should not live in Git history.
4. Git LFS upload support for versioned large binaries.

Chunking a base64 string across text files is a poor fallback: it increases size, creates many commits or tree entries, requires reassembly, and remains constrained by model/tool message limits and safety filters.

## Audit residue

- Branch: `connector-capability-audit-20260730`
- Closed issue: `#13`
- Pull request: `#14`, closed without merge
- Retained branch file: `.connector-audit/tree-test.txt`
- Downloaded sandbox artifact: `ntfy-windows-client-windows-x64.zip`
