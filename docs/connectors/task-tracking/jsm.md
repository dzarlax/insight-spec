# Jira Service Management (JSM) Connector Specification

> Version 1.0 — March 2026
> Based on: Atlassian REST API v3 and Service Desk API v1

Standalone specification for the Jira Service Management (ITSM / Service Desk) connector. JSM is a distinct Atlassian product from Jira Software — focused on inbound support and ITSM workflows rather than development project management.

<!-- toc -->

- [Overview](#overview)
- [Bronze Tables](#bronze-tables)
  - [`jsm_issue` — Service request / incident core fields](#jsmissue-service-request-incident-core-fields)
  - [`jsm_issue_history` — Complete changelog](#jsmissuehistory-complete-changelog)
  - [`jsm_issue_ext` — Custom fields (key-value)](#jsmissueext-custom-fields-key-value)
  - [`jsm_sla` — SLA status per request](#jsmsla-sla-status-per-request)
  - [`jsm_comments` — Issue comments](#jsmcomments-issue-comments)
  - [`jsm_projects` — Service desk project directory](#jsmprojects-service-desk-project-directory)
  - [`jsm_queues` — Queue definitions per service desk](#jsmqueues-queue-definitions-per-service-desk)
  - [`jsm_issue_links` — Issue dependencies](#jsmissuelinks-issue-dependencies)
  - [`jsm_user` — User directory (agents and customers)](#jsmuser-user-directory-agents-and-customers)
  - [`jsm_collection_runs` — Connector execution log](#jsmcollectionruns-connector-execution-log)
- [Identity Resolution](#identity-resolution)
- [Silver / Gold Mappings](#silver-gold-mappings)
- [Open Questions](#open-questions)
  - [OQ-JSM-1: Customer account email suppression](#oq-jsm-1-customer-account-email-suppression)
  - [OQ-JSM-2: SLA collection frequency](#oq-jsm-2-sla-collection-frequency)
  - [OQ-JSM-3: Queue-based vs project-based collection](#oq-jsm-3-queue-based-vs-project-based-collection)
  - [OQ-JSM-4: `class_task_tracker_sla` — unified Silver schema](#oq-jsm-4-classtasktrackersla-unified-silver-schema)

<!-- /toc -->

---

## Overview

**API**: Atlassian REST API v3 (`/rest/api/3/`) for core issue data; Atlassian Service Desk API v1 (`/rest/servicedeskapi/`) for JSM-specific entities (queues, SLA, customer portals). Base URL: `https://{domain}.atlassian.net`.

**Category**: Task Tracking

**`data_source`**: `insight_jsm`

**Authentication**: Same mechanism as Jira Software — Basic Auth (email + API token) for Cloud; OAuth 2.0 (3LO) for delegated access. Service account recommended for automated ingestion.

**Identity**: `jsm_user.email` — resolved to canonical `person_id` via Identity Manager in Silver step 2. JSM distinguishes two user classes: **agents** (`account_type = "atlassian"`) who work issues, and **customers** (`account_type = "customer"`) who raise them. Only agents are typically resolved to `person_id` for productivity analytics; customers are attributed by email but not linked to the internal HR roster.

**Key difference from Jira Software**: JSM issue queues replace Agile boards and sprints. The primary analytics focus is ITSM performance — MTTR (mean time to resolve), SLA compliance, resolution time distribution, agent workload, and incident frequency — rather than sprint velocity or development cycle time.

**`source_instance_id`**: present in all tables — required to disambiguate multiple JSM instances in the same Bronze store. `(source_instance_id, issue_id)` is the composite primary key for issues.

**Issue type vocabulary**:

| JSM Issue Type | ITSM Concept | Typical Workflow |
|----------------|-------------|-----------------|
| `Service Request` | Customer asks for something | Request → In Progress → Resolved |
| `Incident` | Unplanned disruption | Triage → Investigation → Resolved |
| `Change` | Planned modification to IT systems | CAB Review → Scheduled → Implemented |
| `Problem` | Root cause of recurring incidents | Investigation → Root Cause Identified → Closed |

---

## Bronze Tables

### `jsm_issue` — Service request / incident core fields

Minimal record — identifiers, immutable context, and the ITSM-specific `issue_type`. All mutable state (status transitions, assignee changes, SLA events) lives in `jsm_issue_history` and `jsm_sla`.

Collected from `GET /rest/api/3/issue/{issueId}` (full issue detail) or `GET /rest/servicedeskapi/servicedesk/{serviceDeskId}/queue/{queueId}/issue` (queue-scoped).

| Field | Type | Description |
|-------|------|-------------|
| `source_instance_id` | text | Connector instance identifier, e.g. `jsm-acme-prod` |
| `issue_id` | text | Jira internal numeric ID, e.g. `10042` |
| `id_readable` | text | Human-readable key, e.g. `IT-123` — joins to `jsm_issue_history.id_readable` |
| `project_key` | text | Service desk project key, e.g. `IT` — from `fields.project.key` |
| `service_desk_id` | text | Service desk ID — from Service Desk API; NULL if not fetched via queue endpoint |
| `issue_type` | text | ITSM issue type: `Service Request` / `Incident` / `Change` / `Problem` — from `fields.issuetype.name` |
| `reporter_id` | text | Customer or internal user who created the request — `fields.reporter.accountId` — joins to `jsm_user.account_id` |
| `due_date` | date | Due date — from `fields.duedate`; NULL if not set |
| `parent_id` | text | Parent issue key (subtask of or linked incident); NULL if top-level |
| `created` | timestamptz | Issue creation timestamp — from `fields.created` |
| `updated` | timestamptz | Last update — from `fields.updated`; cursor for incremental sync |

**Note**: `story_points` is omitted — not applicable to ITSM workflows. Story-point estimation is a Jira Software concept; JSM uses SLA targets and resolution time instead.

---

### `jsm_issue_history` — Complete changelog

Every status transition, reassignment, priority change, and field update is a separate row. Collected from `GET /rest/api/3/issue/{issueId}/changelog`. The append-only event log — source of truth for MTTR, SLA pause/resume analysis, and agent assignment history.

| Field | Type | Description |
|-------|------|-------------|
| `source_instance_id` | text | Connector instance identifier — scopes all IDs |
| `id_readable` | text | Human-readable issue key — joins to `jsm_issue.id_readable` |
| `issue_id` | text | Parent issue's internal numeric ID |
| `author_account_id` | text | Atlassian account ID of who made the change — joins to `jsm_user.account_id` |
| `changelog_id` | text | Changelog entry ID — multiple field changes in one operation share this ID |
| `created_at` | timestamptz | When the change was made — from `created` |
| `field_id` | text | Machine-readable field identifier — from `fieldId` |
| `field_name` | text | Human-readable field name — from `field`, e.g. `status`, `assignee`, `priority` |
| `value_from` | text | Previous raw value (ID or key) — from `from`; NULL if field was empty |
| `value_from_string` | text | Previous human-readable value — from `fromString`, e.g. `Waiting for Support` |
| `value_to` | text | New raw value after the change — from `to` |
| `value_to_string` | text | New human-readable value — from `toString`, e.g. `In Progress` |

**`changelog_id` groups related changes**: one agent action updating multiple fields produces multiple rows with the same `changelog_id`.

**Key status transitions for ITSM analytics** — `field_name = "status"`:
- `Waiting for Support` → `In Progress`: agent picks up the request (start of handling time)
- `In Progress` → `Waiting for Customer`: request on hold pending customer response (SLA clock pause in many configurations)
- Any status → `Resolved` / `Done` / `Closed`: resolution event — used for MTTR calculation

---

### `jsm_issue_ext` — Custom fields (key-value)

Stores per-issue custom field values that don't fit the core schema. Custom field discovery via `GET /rest/api/3/field`.

| Field | Type | Description |
|-------|------|-------------|
| `source_instance_id` | text | Connector instance identifier |
| `id_readable` | text | Issue key — joins to `jsm_issue.id_readable` |
| `field_id` | text | Custom field ID, e.g. `customfield_10060` |
| `field_name` | text | Custom field display name, e.g. `Team`, `Affected System`, `Category` |
| `field_value` | text | Field value as string (JSON for complex types) |
| `value_type` | text | Type hint: `string` / `number` / `user` / `option` / `json` |
| `collected_at` | timestamptz | Collection timestamp |

**Purpose**: captures ITSM-specific categorisation fields — affected system, business impact, root cause category, customer tier — without schema changes.

---

### `jsm_sla` — SLA status per request

SLA metrics per issue, per SLA policy. Collected from `GET /rest/servicedeskapi/request/{issueIdOrKey}/sla`. One row per SLA policy per issue at collection time. Provides the SLA breach/compliance signal for Gold metrics.

| Field | Type | Description |
|-------|------|-------------|
| `source_instance_id` | text | Connector instance identifier |
| `id_readable` | text | Issue key — joins to `jsm_issue.id_readable` |
| `sla_name` | text | SLA policy name, e.g. `Time to first response`, `Time to resolution` |
| `sla_id` | text | SLA field ID from Jira, e.g. `customfield_10020` |
| `is_breached` | boolean | Whether the SLA has been or is being breached |
| `is_paused` | boolean | Whether the SLA clock is currently paused (e.g. waiting for customer) |
| `remaining_seconds` | numeric | Seconds remaining before breach; negative if already breached; NULL if completed |
| `completed_at` | timestamptz | When the SLA was completed (goal met); NULL if still open |
| `breached_at` | timestamptz | When the breach occurred; NULL if not breached |
| `goal_seconds` | numeric | SLA target in seconds, e.g. 28800 for 8 hours |
| `elapsed_seconds` | numeric | Time elapsed against SLA (excluding paused periods); NULL if SLA is paused |
| `collected_at` | timestamptz | Collection timestamp — use for point-in-time SLA snapshots |

**Note**: the SLA API returns the current SLA status, not a history of SLA changes. SLA pause/resume events (e.g. waiting for customer) are inferred from `jsm_issue_history` status transitions. `collected_at` enables snapshot analysis when collected periodically.

---

### `jsm_comments` — Issue comments

Collected from `GET /rest/api/3/issue/{key}/comment`.

| Field | Type | Description |
|-------|------|-------------|
| `source_instance_id` | text | Connector instance identifier |
| `comment_id` | text | Comment ID |
| `id_readable` | text | Parent issue key — joins to `jsm_issue.id_readable` |
| `author_account_id` | text | Comment author — joins to `jsm_user.account_id` |
| `created` | timestamptz | When comment was posted |
| `updated` | timestamptz | Last edit timestamp |
| `body` | text | Comment body (Atlassian Document Format; plain text extracted at collection) |
| `is_public` | boolean | Whether the comment is visible to the customer (JSM supports internal/public comments) |

**Purpose**: agent-customer communication signal — response latency, communication volume per agent, and whether agent notes are internal or customer-facing.

---

### `jsm_projects` — Service desk project directory

Collected from `GET /rest/api/3/project?typeKey=service_desk`.

| Field | Type | Description |
|-------|------|-------------|
| `source_instance_id` | text | Connector instance identifier |
| `project_id` | text | Jira internal project ID |
| `project_key` | text | Service desk project key, e.g. `IT` — joins to `jsm_issue.project_key` |
| `service_desk_id` | text | Service Desk API identifier for this project — used for queue and SLA API calls |
| `name` | text | Project name |
| `lead_account_id` | text | Project lead — joins to `jsm_user.account_id` |
| `archived` | boolean | Whether the project is archived |
| `collected_at` | timestamptz | Collection timestamp |

**Purpose**: maps service requests to service desks and teams. `service_desk_id` is required for all Service Desk API calls (queues, SLA).

---

### `jsm_queues` — Queue definitions per service desk

Collected from `GET /rest/servicedeskapi/servicedesk/{serviceDeskId}/queue`.

| Field | Type | Description |
|-------|------|-------------|
| `source_instance_id` | text | Connector instance identifier |
| `service_desk_id` | text | Service desk ID — joins to `jsm_projects.service_desk_id` |
| `queue_id` | text | Queue ID |
| `queue_name` | text | Queue display name, e.g. `My Open Requests`, `Incidents`, `SLA Breached` |
| `issue_count` | numeric | Number of issues in the queue at collection time |
| `jql_filter` | text | JQL query defining the queue membership; NULL if not exposed by API |
| `collected_at` | timestamptz | Collection timestamp |

**Purpose**: queue membership and queue depth — captures workload distribution and queue health (e.g. SLA Breached queue count over time). Not a primary analytics source; used as a reference for queue-level reporting.

---

### `jsm_issue_links` — Issue dependencies

Collected from `fields.issuelinks` in issue response. Same structure as Jira Software links.

| Field | Type | Description |
|-------|------|-------------|
| `source_instance_id` | text | Connector instance identifier |
| `source_issue` | text | Source issue key |
| `target_issue` | text | Target issue key |
| `link_type` | text | Link type name, e.g. `blocks` / `is blocked by` / `duplicates` / `relates to` / `caused by` / `is caused by` |
| `collected_at` | timestamptz | Collection timestamp |

**Purpose**: incident-to-problem linkage (root cause analysis), duplicate detection, and cascade impact analysis.

---

### `jsm_user` — User directory (agents and customers)

Collected from `GET /rest/api/3/users/search` for agents (internal accounts) and supplemented by customer records encountered in issue reporter fields.

| Field | Type | Description |
|-------|------|-------------|
| `source_instance_id` | text | Connector instance identifier |
| `account_id` | text | Atlassian account ID — joins to `author_account_id` / `reporter_id` / `lead_account_id` |
| `email` | text | Email — primary key for identity resolution; **nullable** — may be suppressed by Atlassian privacy controls for customer accounts |
| `display_name` | text | Display name |
| `account_type` | text | `atlassian` (agent / internal user) / `customer` (portal user) / `app` (automation) |
| `active` | boolean | Whether the account is active |

**`account_type` significance**:
- `atlassian`: internal agents — resolved to `person_id` via Identity Manager for productivity analytics
- `customer`: portal users — attributed by email for SLA and request-volume reporting but not linked to the HR person roster
- `app`: automation / bot accounts — excluded from person-level analytics

**Note**: `account_id` is shared across the Atlassian platform (Jira, JSM, Confluence, Bitbucket on the same tenant). When `email` is suppressed for customer accounts, `account_id` can serve as a stable customer identifier within the Atlassian ecosystem.

---

### `jsm_collection_runs` — Connector execution log

| Field | Type | Description |
|-------|------|-------------|
| `run_id` | text | Unique run identifier |
| `started_at` | timestamp | Run start time |
| `completed_at` | timestamp | Run end time |
| `status` | text | `running` / `completed` / `failed` |
| `issues_collected` | numeric | Rows collected for `jsm_issue` |
| `history_records_collected` | numeric | Rows collected for `jsm_issue_history` |
| `sla_records_collected` | numeric | Rows collected for `jsm_sla` |
| `comments_collected` | numeric | Rows collected for `jsm_comments` |
| `users_collected` | numeric | Rows collected for `jsm_user` |
| `api_calls` | numeric | Total API calls made |
| `errors` | numeric | Errors encountered |
| `settings` | jsonb | Collection configuration (instance URL, project filter, lookback, SLA collection toggle) |

Monitoring table — not an analytics source.

---

## Identity Resolution

**Identity anchor**: `jsm_user` — agents are resolved to `person_id`; customers are tracked by email but not linked to the HR roster.

**Agent resolution chain**:
```
jsm_issue_history.author_account_id
  → jsm_user.account_id (where account_type = 'atlassian')
  → jsm_user.email
  → person_id
```

Same chain applies to `jsm_comments.author_account_id` and `jsm_projects.lead_account_id`.

**Reporter (customer) chain**: `jsm_issue.reporter_id` → `jsm_user.account_id` → `jsm_user.email`. Customer emails are stored in Silver for request-volume attribution but do not flow into the `person_id` space.

**`source_instance_id` is required in all joins** — `id_readable` values like `IT-123` can collide across instances.

**Atlassian email suppression**: Atlassian privacy controls may suppress `emailAddress` for customer accounts. When email is NULL for a customer, `account_id` can serve as a stable customer identifier within the same Atlassian tenant. For agents, suppression is rare; `account_id` fallback may be used for within-Atlassian resolution (see OQ-JSM-1).

---

## Silver / Gold Mappings

JSM maps to the same unified Bronze-to-Silver pipeline as Jira Software, with `data_source = "insight_jsm"` as the discriminator.

| Bronze table | Silver target | Notes |
|-------------|--------------|-------|
| `jsm_issue` + `jsm_issue_history` | `class_task_tracker_activities` | Append-only event stream — `data_source = "insight_jsm"` |
| `jsm_issue` + `jsm_issue_history` | `class_task_tracker_snapshot` | Current state per issue (upsert) |
| `jsm_sla` | `class_task_tracker_sla` | Planned — SLA compliance per issue per policy; feeds breach-rate and compliance metrics |
| `jsm_comments` | `class_task_tracker_comments` | Planned — collaboration signal; `is_public` preserved for agent-vs-internal distinction |
| `jsm_user` | Identity Manager (`email` → `person_id`) | Agents only (`account_type = 'atlassian'`) |
| `jsm_projects` | Reference — service desk mapping | Used for grouping and filtering |
| `jsm_queues` | Reference — queue depth snapshots | Not a primary analytics source |
| `jsm_issue_links` | Reference — impact / root cause analysis | Used to link incidents to problems in Gold |
| `jsm_issue_ext` | Merged into Silver snapshots | ITSM categorisation fields promoted selectively |

**Silver step 1**: `class_task_tracker_activities` (event log) + `class_task_tracker_snapshot` (current state), unified with Jira Software via `data_source` discriminator.

**Silver step 2**: identity resolution — `author_account_id` → `person_id` for agents via Identity Manager.

**Gold metrics** (ITSM-focused, distinct from Jira Software dev metrics):
- **MTTR (Mean Time to Resolve)**: time from `created` to first `Resolved` / `Done` status transition in `jsm_issue_history`
- **Time to first response**: time from `created` to first agent comment or first non-`Open` status — cross-referenced with `jsm_sla` where `sla_name = "Time to first response"`
- **SLA compliance rate**: fraction of issues where `jsm_sla.is_breached = false` at resolution, per policy per period
- **Resolution time distribution**: p50 / p90 / p99 of `(resolved_at - created)` by `issue_type` and `project_key`
- **Agent workload**: issues assigned per agent per period from `class_task_tracker_snapshot` + `class_task_tracker_activities`
- **Incident frequency**: count of `issue_type = "Incident"` per time bucket
- **Reopen rate**: issues that transition from `Resolved` back to an active status (via `jsm_issue_history`)

---

## Open Questions

### OQ-JSM-1: Customer account email suppression

Atlassian privacy controls may suppress `emailAddress` for customer (`account_type = "customer"`) portal accounts. Options:
- Use `account_id` as a stable customer identifier within the same Atlassian tenant (does not cross-resolve to HR)
- Require email for analytics attribution and exclude customer-anonymous requests from per-reporter metrics
- Support opt-in customer email collection via the Service Desk Customer API where permitted

### OQ-JSM-2: SLA collection frequency

`jsm_sla` captures point-in-time SLA status at collection. For accurate breach-rate analysis, SLA state should be recorded at least hourly for open issues. Options:
- Incremental mode: collect SLA for all open issues on every run (potentially expensive for large instances)
- Event-driven mode: collect SLA only when `jsm_issue_history` shows a status transition
- Scheduled snapshots: daily snapshot for closed issues; hourly for open issues near SLA deadline

### OQ-JSM-3: Queue-based vs project-based collection

Issues can be discovered via two paths:
- `GET /rest/api/3/project?typeKey=service_desk` → JQL search per project (full coverage)
- `GET /rest/servicedeskapi/servicedesk/{id}/queue/{queueId}/issue` → queue membership (partial, depends on queue configuration)

The project-based path is recommended for completeness. The queue path is supplemental for `jsm_queues` depth tracking.

### OQ-JSM-4: `class_task_tracker_sla` — unified Silver schema

The `jsm_sla` table introduces a new Silver target (`class_task_tracker_sla`) not present in the Jira or YouTrack connectors. Design of this table is pending — needs to accommodate multiple SLA policies per issue and time-series breach history.
