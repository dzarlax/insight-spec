# Confluence Connector

Extracts space directory, page metadata, and page version history from Confluence Cloud REST API v2 using Basic Auth (email + API token).

## Prerequisites

1. Generate an Atlassian API token at https://id.atlassian.com/manage-profile/security/api-tokens
2. Use the email address associated with the Atlassian account that has read access to the target Confluence instance

## K8s Secret

```yaml
apiVersion: v1
kind: Secret
metadata:
  name: insight-confluence-main
  labels:
    app.kubernetes.io/part-of: insight
  annotations:
    insight.cyberfabric.com/connector: confluence
    insight.cyberfabric.com/source-id: confluence-main
type: Opaque
stringData:
  confluence_instance_url: "https://myorg.atlassian.net"
  confluence_email: "user@example.com"
  confluence_api_token: "CHANGE_ME"
  confluence_start_date: "2020-01-01"
```

### Fields

| Field | Required | Description |
|-------|----------|-------------|
| `confluence_instance_url` | Yes | Confluence Cloud instance URL (e.g. `https://myorg.atlassian.net`) |
| `confluence_email` | Yes | Email address for Basic Auth |
| `confluence_api_token` | Yes | Atlassian API token (sensitive) |
| `confluence_start_date` | No | Earliest date for incremental sync, YYYY-MM-DD (default: 2020-01-01) |
| `confluence_page_size` | No | Results per API page (default: 100, max: 250) |

> **Note on `username` / `password` spec fields.**
> If importing/exporting via Airbyte Builder, it may add `username` and `password` fields to the spec. These are Builder artifacts that map from the `BasicHttpAuthenticator` config -- they do not need to be added to the K8s Secret. The real credential fields are `confluence_email` and `confluence_api_token`.

### Automatically injected

| Field | Source |
|-------|--------|
| `insight_tenant_id` | `tenant_id` from tenant YAML |
| `insight_source_id` | `insight.cyberfabric.com/source-id` annotation |

### Local development

Create `src/ingestion/secrets/connectors/confluence.yaml` (gitignored) from the example:

```bash
cp src/ingestion/secrets/connectors/confluence.yaml.example src/ingestion/secrets/connectors/confluence.yaml
# Fill in real values, then apply:
kubectl apply -f src/ingestion/secrets/connectors/confluence.yaml
```

## Streams

| Stream | Description | Sync Mode |
|--------|-------------|-----------|
| `wiki_spaces` | Space directory (id, name, type, status, URL) | Full Refresh |
| `wiki_pages` | Page metadata with version info (incremental on `updated_at`) | Incremental (client-side cursor) |
| `wiki_page_versions` | Version history per page (substream of wiki_pages) | Full Refresh (per parent page) |

## Silver Targets

- `class_wiki_pages` -- unified page metadata across Confluence and Outline
- `class_wiki_activity` -- per-user per-day edit activity (derived from page versions)

User identity resolution: `wiki_pages.author_id` -> `jira_user.account_id` -> `email` -> `person_id` (via Identity Manager in Silver step 2)

## User Resolution

This connector does NOT include a `wiki_users` stream. Confluence v2 page and version responses return only `authorId` (Atlassian `accountId`), not email or display name. User identity resolution happens in the Silver/dbt layer via JOIN with `jira_user`, which shares the same Atlassian `accountId` namespace.

Resolution chain (Silver):
```
wiki_pages.author_id (accountId)
  -> jira_user.account_id (same Atlassian accountId)
    -> jira_user.email
      -> Identity Manager -> person_id
```

## Phase 1 Limitations

- Cloud-only (no Server/Data Center support)
- No analytics data (view counts, distinct viewers) -- deferred to Phase 2
- No email resolution at connector level -- resolved in Silver via jira_user
- Client-side incremental cursor (API lacks server-side `lastModifiedAfter`)
- No blog post or comment extraction
