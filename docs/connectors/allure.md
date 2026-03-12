# Allure TestOps Connector Specification

> Version 1.0 — March 2026
> Based on: `docs/CONNECTORS_REFERENCE.md` Source 20 (Allure TestOps)

Standalone specification for the Allure TestOps (Quality / Testing) connector. Expands Source 20 in the main Connector Reference with full table schemas, identity mapping, Silver/Gold pipeline notes, and open questions.

<!-- toc -->

- [Overview](#overview)
- [Bronze Tables](#bronze-tables)
  - [`allure_launches` — Test run / launch records](#allurelaunches-test-run-launch-records)
  - [`allure_test_results` — Individual test case results](#alluretestresults-individual-test-case-results)
  - [`allure_defects` — Defects linked to test failures](#alluredefects-defects-linked-to-test-failures)
  - [`allure_collection_runs` — Connector execution log](#allurecollectionruns-connector-execution-log)
- [Identity Resolution](#identity-resolution)
- [Silver / Gold Mappings](#silver-gold-mappings)
- [Open Questions](#open-questions)
  - [OQ-AL-1: User attribution — no author field in Bronze tables](#oq-al-1-user-attribution-no-author-field-in-bronze-tables)
  - [OQ-AL-2: `external_issue_id` linking strategy — `class_task_tracker_activities` join](#oq-al-2-externalissueid-linking-strategy-classtasktracker-activities-join)

<!-- /toc -->

---

## Overview

**API**: Allure TestOps REST API

**Category**: Quality / Testing

**Authentication**: API token (Allure TestOps service account)

**Identity**: No direct user attribution in the current Bronze schema — launches and test results are CI/CD run artifacts, not attributed to individual authors. Defects link to external task tracker tickets via `external_issue_id`.

**Field naming**: snake_case — preserved as-is at Bronze level.

**Why multiple tables**: Projects → Launches → Test Results → Defects is a genuine 1:N entity hierarchy. A launch has many test results; a defect links many test results. Flattening would repeat launch metadata on every test result row.

**Primary use in Insight**: delivery quality metrics — pass rates, flaky test detection, defect accumulation trends, and linking quality failures to sprint/commit activity. `allure_defects.external_issue_id` enables joining to `class_task_tracker_activities` (YouTrack / Jira tickets). This is a cross-domain JOIN key — Allure does not write to `class_task_tracker_activities`; it only references it at Gold query time.

---

## Bronze Tables

### `allure_launches` — Test run / launch records

| Field | Type | Description |
|-------|------|-------------|
| `launch_id` | bigint | Allure internal launch ID — primary key |
| `project_id` | bigint | Project this launch belongs to |
| `name` | text | Launch name, e.g. `Regression Suite - main` |
| `status` | text | `passed` / `failed` / `broken` / `unknown` |
| `created_date` | timestamptz | Launch start time |
| `closed_date` | timestamptz | Launch end time (NULL if running) |
| `duration_seconds` | numeric | Total run duration |
| `passed_count` | numeric | Tests passed |
| `failed_count` | numeric | Tests failed |
| `broken_count` | numeric | Tests broken (infrastructure/setup failures) |
| `skipped_count` | numeric | Tests skipped |
| `total_count` | numeric | Total tests in launch |
| `tags` | jsonb | Launch tags — environment, branch, build number, CI run ID, etc. |

`tags` enables correlating launches with git branches and CI build numbers without a direct join to git Bronze tables.

---

### `allure_test_results` — Individual test case results

| Field | Type | Description |
|-------|------|-------------|
| `result_id` | bigint | Allure test result ID — primary key |
| `launch_id` | bigint | Parent launch — joins to `allure_launches.launch_id` |
| `test_case_id` | bigint | Test case definition ID — stable across runs (same test in different launches shares this ID) |
| `test_name` | text | Test case name |
| `full_path` | text | Suite / class / method path |
| `status` | text | `passed` / `failed` / `broken` / `skipped` |
| `duration_seconds` | numeric | Test execution duration |
| `start_time` | timestamptz | Test start |
| `stop_time` | timestamptz | Test stop |
| `flaky` | boolean | Marked as flaky (inconsistent results across runs) |
| `message` | text | Failure message (NULL if passed) |
| `trace` | text | Stack trace (NULL if passed) |

`test_case_id` is stable across launches — enables tracking a specific test's pass/fail history over time and identifying consistently failing or flaky tests.

---

### `allure_defects` — Defects linked to test failures

| Field | Type | Description |
|-------|------|-------------|
| `defect_id` | bigint | Allure defect ID — primary key |
| `project_id` | bigint | Project |
| `name` | text | Defect name / title |
| `status` | text | `open` / `resolved` |
| `created_date` | timestamptz | When the defect was first detected |
| `closed_date` | timestamptz | When resolved (NULL if open) |
| `external_issue_id` | text | Linked ticket in YouTrack / Jira, e.g. `PROJ-123` — cross-domain join key → `class_task_tracker_activities.task_id` (JOIN only; Allure does not write to this stream) |
| `result_count` | numeric | Number of test results linked to this defect |

`external_issue_id` is the critical cross-domain JOIN key — joins Allure defects to `class_task_tracker_activities` at Gold query time, enabling quality failures to be linked to delivery timeline (sprint, assignee, cycle time). Allure does not write to `class_task_tracker_activities`.

---

### `allure_collection_runs` — Connector execution log

| Field | Type | Description |
|-------|------|-------------|
| `run_id` | text | Unique run identifier |
| `started_at` | timestamp | Run start time |
| `completed_at` | timestamp | Run end time |
| `status` | text | `running` / `completed` / `failed` |
| `launches_collected` | numeric | Rows collected for `allure_launches` |
| `test_results_collected` | numeric | Rows collected for `allure_test_results` |
| `defects_collected` | numeric | Rows collected for `allure_defects` |
| `api_calls` | numeric | API calls made |
| `errors` | numeric | Errors encountered |
| `settings` | jsonb | Collection configuration (instance URL, project filter, lookback) |

Monitoring table — not an analytics source.

---

## Identity Resolution

The current Bronze schema has **no user-level identity fields** in `allure_launches`, `allure_test_results`, or `allure_defects`. Test launches are CI/CD artifacts — they are not attributed to individual authors in the Allure TestOps API.

Cross-domain linking in Insight is achieved through:
- `allure_defects.external_issue_id` → `class_task_tracker_activities.task_id` (defect ↔ ticket, cross-domain JOIN at Gold query time)
- `allure_launches.tags` (branch, build number) → git Bronze tables (launch ↔ commit)

Person attribution for test failures requires joining through the task tracker: defect → linked ticket → assignee → `person_id`.

---

## Silver / Gold Mappings

| Bronze table | Silver target | Status |
|-------------|--------------|--------|
| `allure_launches` | `class_quality_launches` | Planned — quality Silver stream not yet defined |
| `allure_test_results` | `class_quality_test_results` | Planned — granular test result stream |
| `allure_defects` | `class_quality_defects` | Planned — defect tracking stream |

**Gold**: Quality metrics — pass rate trend, flaky test rate, defect accumulation, MTTR (mean time to resolution for defects). Cross-domain Gold joins:
- Defect count per sprint (via `external_issue_id` → `class_task_tracker_activities` sprint field)
- Test pass rate per branch (via `allure_launches.tags.branch` → git commits)

---

## Open Questions

### OQ-AL-1: User attribution — no author field in Bronze tables

The Allure TestOps API does not expose which team member triggered a launch or created a defect in the current schema. This limits per-person quality analytics.

- Does the Allure API expose a `created_by` or `author` field for launches or defects that is not in this spec?
- If not, should the connector attempt to correlate launches with CI/CD pipeline run metadata (e.g. git commit author) to infer attribution?

### OQ-AL-2: `external_issue_id` linking strategy — `class_task_tracker_activities` join

`allure_defects.external_issue_id` stores ticket IDs like `PROJ-123` that join to `class_task_tracker_activities.task_id` at Gold query time. This linkage depends on:

- Consistent ticket ID format across Jira and YouTrack instances (same `id_readable` format)
- Defects being linked to tickets in Allure TestOps (manual process by QA engineers)

- What percentage of defects are expected to have `external_issue_id` populated?
- Should unlinked defects (NULL `external_issue_id`) be tracked separately as a data quality signal?
