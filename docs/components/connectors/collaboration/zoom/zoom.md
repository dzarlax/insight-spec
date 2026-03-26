# Zoom Connector Specification

> Version 2.1 — March 2026
> Based on: [`PRD.md`](./specs/PRD.md), [`DESIGN.md`](./specs/DESIGN.md), and [`manifest.yaml`](../../../../../src/connectors/collaboration/zoom/manifest.yaml)

Standalone specification for the Zoom collaboration connector. This document is intentionally aligned to the current declarative manifest so that an agent can regenerate the same connector behavior from the specs with minimal ambiguity.

<!-- toc -->

- [Overview](#overview)
- [Connector Inputs](#connector-inputs)
- [Implemented Source Streams](#implemented-source-streams)
  - [`users` -> conceptual `zoom_users`](#users---conceptual-zoom_users)
  - [`meetings` -> conceptual `zoom_meetings`](#meetings---conceptual-zoom_meetings)
  - [`participants` -> conceptual-zoom_meeting_participants](#participants---conceptual-zoom_meeting_participants)
  - [`message_activities` -> conceptual `zoom_message_activity`](#message_activities---conceptual-zoom_message_activity)
- [Authentication and Runtime Behavior](#authentication-and-runtime-behavior)
- [Incremental Behavior](#incremental-behavior)
- [What Is Not Implemented in the Current Manifest](#what-is-not-implemented-in-the-current-manifest)
- [Open Questions](#open-questions)

<!-- /toc -->

## Overview

**API**: Zoom REST APIs

**Category**: Collaboration

**Implemented streams**: `users`, `meetings`, `participants`, `message_activities`

**Authentication**: Zoom Server-to-Server OAuth using:
- `account_id`
- `client_id`
- `client_secret`

**Required scopes**:
- scopes required for `GET /users`
- scopes required for `GET /metrics/meetings`
- scopes required for `GET /metrics/meetings/{meeting_uuid}/participants`
- Team Chat scopes required for `GET /chat/users/{zoom_user_id}/messages`

**Manifest design summary**:
- `users` is a root stream
- `meetings` is a root stream
- `participants` is a child stream of `meetings`
- `message_activities` is a child stream of `users`
- `tenant_id` is copied from config into every emitted row
- `_airbyte_data_source` and `_airbyte_collected_at` are added to every emitted row

This specification treats the conceptual Bronze names `zoom_users`, `zoom_meetings`, `zoom_meeting_participants`, and `zoom_message_activity` as documentation names. The executable manifest uses the stream names `users`, `meetings`, `participants`, and `message_activities`.

## Connector Inputs

The current manifest requires these configuration properties:

| Property | Type | Purpose |
|----------|------|---------|
| `insight_tenant_id` | String | Copied into every record as `tenant_id` |
| `account_id` | String | Zoom Server-to-Server OAuth account ID |
| `client_id` | String | Zoom Server-to-Server OAuth client ID |
| `client_secret` | String | Zoom Server-to-Server OAuth client secret |
| `start_date` | Date | Initial lower bound for meeting and message collection windows |
| `page_size` | Int | Requested page size for paginated endpoints |

## Implemented Source Streams

### `users` -> conceptual `zoom_users`

**Endpoint**: `GET /users`

**Request parameters**:
- `status=active`
- `page_size`

**Pagination**:
- `next_page_token`

**Purpose**:
- support attribution and identity joins
- provide `zoom_user_id` values for the message substream

**Primary key**:
- `zoom_user_id`

**Added fields**:
- `zoom_user_id = record['id']`
- `tenant_id = config['insight_tenant_id']`
- `_airbyte_data_source = insight_zoom`
- `_airbyte_collected_at = now_utc()`

**Schema shape**:
- `tenant_id`
- `zoom_user_id`
- `email`
- `first_name`
- `last_name`
- `display_name`
- `type`
- `status`
- `timezone`
- `_airbyte_data_source`
- `_airbyte_collected_at`

### `meetings` -> conceptual `zoom_meetings`

**Endpoint**: `GET /metrics/meetings`

**Request parameters**:
- `type=past`
- `page_size`
- `from` and `to` are injected by Airbyte incremental sync

**Pagination**:
- `next_page_token`

**Primary key**:
- `meeting_instance_key`

**Implemented identity model**:
- `meeting_series_id = record['id']`
- `meeting_occurrence_id = record.get('occurrence_id')`
- `meeting_uuid = record.get('uuid')`
- `identity_strength = uuid | occurrence | fallback`
- `meeting_instance_key` derived from:
  1. `meeting_uuid`
  2. else `meeting_series_id + meeting_occurrence_id`
  3. else `meeting_series_id + strongest available time field`

**Added fields**:
- `tenant_id`
- `meeting_series_id`
- `meeting_occurrence_id`
- `meeting_uuid`
- `identity_strength`
- `meeting_instance_key`
- `_airbyte_data_source`
- `_airbyte_collected_at`

**Schema shape**:
- `tenant_id`
- `meeting_instance_key`
- `meeting_series_id`
- `meeting_occurrence_id`
- `meeting_uuid`
- `identity_strength`
- `host_user_id`
- `topic`
- `meeting_type`
- `scheduled_start_at`
- `actual_start_at`
- `actual_end_at`
- `duration_seconds`
- `discovered_at`
- `limitation_code`
- `source_endpoint`
- `_airbyte_data_source`
- `_airbyte_collected_at`

### `participants` -> conceptual-zoom_meeting_participants

**Endpoint**: `GET /metrics/meetings/{meeting_uuid}/participants`

**Parent stream**:
- `meetings`

**Partition field**:
- `meeting_uuid`

**Request parameters**:
- `type=past`
- `page_size`

**Pagination**:
- `next_page_token`

**Special handling**:
- `404` is treated as success for this endpoint to tolerate source-side gaps

**Primary key**:
- `(meeting_instance_key, participant_key, join_at)`

**Added fields**:
- `meeting_series_id` from parent partition
- `meeting_occurrence_id` from parent partition
- `meeting_uuid` from parent partition
- `meeting_instance_key` from parent partition
- `participant_key = id or user_id or email or name`
- `join_at = join_time`
- `leave_at = leave_time`
- `attendance_duration_seconds = duration`
- `tenant_id`
- `_airbyte_data_source`
- `_airbyte_collected_at`

**Schema shape**:
- `tenant_id`
- `meeting_instance_key`
- `meeting_series_id`
- `meeting_occurrence_id`
- `meeting_uuid`
- `participant_key`
- `zoom_user_id`
- `email`
- `display_name`
- `join_at`
- `leave_at`
- `attendance_duration_seconds`
- `attendance_status`
- `_airbyte_data_source`
- `_airbyte_collected_at`

### `message_activities` -> conceptual `zoom_message_activity`

**Endpoint**: `GET /chat/users/{zoom_user_id}/messages`

**Parent stream**:
- `users`

**Partition field**:
- `zoom_user_id`

**Request parameters**:
- `from = config['start_date']`
- `to = now_utc()`
- `page_size`

**Pagination**:
- `next_page_token`

**Response record path**:
- top-level `messages` array

**Primary key**:
- `message_activity_id`

**Added fields**:
- `collection_mode = separate_chat_flow`
- `zoom_user_id` from parent partition
- `message_activity_id = id or message_id or deterministic fallback`
- `channel_type = record['channel_type'] or 'direct'`
- `activity_date = date_time or message_time or date + T00:00:00Z`
- `message_count = count or 1`
- `source_endpoint = chat/users/{userId}/messages`
- `tenant_id`
- `_airbyte_data_source`
- `_airbyte_collected_at`

**Schema shape**:
- `tenant_id`
- `message_activity_id`
- `zoom_user_id`
- `email`
- `activity_date`
- `channel_type`
- `message_count`
- `aggregation_level`
- `collection_mode`
- `source_endpoint`
- `_airbyte_data_source`
- `_airbyte_collected_at`

**Important note**:
The current manifest does not emit direct meeting linkage fields for message activity.

## Authentication and Runtime Behavior

The manifest uses:
- `SessionTokenAuthenticator`
- token URL `https://zoom.us/oauth/token`
- `BasicHttpAuthenticator` with `client_id` and `client_secret`
- `grant_type=account_credentials`
- `account_id`

All streams share:
- `url_base = https://api.zoom.us/v2/`
- `Accept: application/json`
- common retry handling for `429`, `503`, `500`, `502`, `504`
- `participants` additionally treats `404` as success to tolerate source-side gaps while preserving the shared retry rules

## Incremental Behavior

Only `meetings` is currently configured as a true Airbyte stateful incremental stream.

**Current `meetings` incremental configuration**:
- cursor type: `DatetimeBasedCursor`
- cursor field: `end_time`
- request window fields: `from`, `to`
- start datetime: `config['start_date']`
- end datetime: `now_utc()`
- lookback window: `P7D`
- step: `P30D`
- cursor granularity: `P1D`

**Current non-stateful behavior**:
- `users`: paginated bounded read
- `participants`: child stream of `meetings`; effectively scoped by whatever meetings are read
- `message_activities`: request-bounded by `start_date` to `now`, but not currently configured as an Airbyte stateful incremental stream

## What Is Not Implemented in the Current Manifest

These items are intentionally out of the current executable spec, even if earlier drafts discussed them:

- `zoom_collection_runs`
- `run_id`
- `_version`
- raw `metadata` blob persistence
- capability resolver component
- enrichment queue or coordinator
- direct message-to-meeting linkage fields
- multiple interchangeable message collection strategies
- separate meeting detail endpoint flow beyond the current `meetings` stream

## Open Questions

### OQ-ZOOM-1: Team Chat scopes

The current message path requires Team Chat scopes. The connector should continue documenting the exact scopes observed during validation so runtime setup is reproducible.

### OQ-ZOOM-2: Message incremental state

The current manifest uses a bounded request window for messages but not a stateful Airbyte incremental cursor. A future revision may add that if Zoom exposes a reliable cursor field and request contract for the implemented endpoint.

### OQ-ZOOM-3: Missing project-wide connector reference

Project rules reference `docs/CONNECTORS_REFERENCE.md`, but that file is currently absent. Zoom-specific docs therefore need to stay explicit about implemented fields and runtime behavior.
