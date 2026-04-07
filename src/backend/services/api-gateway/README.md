# Insight API Gateway

HTTP entry point for all Insight backend services. Built on cyberfabric-core ModKit with OIDC/JWT authentication.

## What it does

- Routes HTTP requests to backend service modules
- Validates JWT bearer tokens against an OIDC provider (Okta, Keycloak, Auth0, etc.)
- Enforces RBAC and tenant isolation via authz-resolver
- Provides OpenAPI documentation, CORS, rate limiting, request tracing
- Returns RFC 9457 Problem Details for all errors

## Quick start (local dev, no auth)

```bash
cd src/backend
cargo run --bin insight-api-gateway -- run -c services/api-gateway/config/no-auth.yaml
```

Open `http://localhost:8080/api/v1/docs` for OpenAPI UI.

## Quick start (with OIDC)

```bash
export OIDC_ISSUER_URL=https://dev-12345.okta.com/oauth2/default
export OIDC_AUDIENCE=api://insight

cd src/backend
cargo run --bin insight-api-gateway -- run -c services/api-gateway/config/insight.yaml
```

Test with a valid Okta token:

```bash
curl -H "Authorization: Bearer <your-jwt-token>" http://localhost:8080/api/v1/docs
```

## Configuration

Configuration is loaded in layers (highest priority last):

1. **Defaults** — hardcoded in code
2. **YAML file** — `-c config/insight.yaml`
3. **Environment variables** — `APP__modules__<module>__config__<key>`
4. **CLI flags** — `--verbose`, `--print-config`

### Key environment variables

| Variable | Description | Example |
|----------|-------------|---------|
| `OIDC_ISSUER_URL` | OIDC issuer URL | `https://dev-12345.okta.com/oauth2/default` |
| `OIDC_AUDIENCE` | Expected JWT audience | `api://insight` |
| `APP__modules__api-gateway__config__bind_addr` | Listen address | `0.0.0.0:8080` |
| `APP__modules__api-gateway__config__auth_disabled` | Disable auth | `true` |

### Okta setup

1. Create an Okta application (Web or SPA)
2. Note the **Issuer URI** (e.g., `https://dev-12345.okta.com/oauth2/default`)
3. Note the **Audience** (e.g., `api://insight`)
4. Set `OIDC_ISSUER_URL` and `OIDC_AUDIENCE` environment variables
5. The plugin auto-discovers JWKS keys from `{issuer}/v1/keys`

### Custom claims

The plugin extracts these claims from the JWT:

| Claim | Maps to | Default behavior |
|-------|---------|-----------------|
| `sub` | `subject_id` (as UUID v5 hash) | Required |
| `scp` or `scope` | `token_scopes` | `["*"]` if missing |
| `tenant_id` (configurable) | `subject_tenant_id` | Nil UUID if missing |

To use a different claim for tenant ID, set `tenant_claim` in config.

## Deploy to Kubernetes

### Helm install

```bash
helm install insight-gw ./helm \
  --set oidc.issuerUrl=https://dev-12345.okta.com/oauth2/default \
  --set oidc.audience=api://insight \
  --set ingress.host=insight.example.com
```

### Helm values

See `helm/values.yaml` for all configurable values. Key ones:

```yaml
oidc:
  issuerUrl: "https://dev-12345.okta.com/oauth2/default"
  audience: "api://insight"

authDisabled: false  # Set true for dev without IdP

ingress:
  enabled: true
  host: insight.example.com
  tls:
    enabled: true
    secretName: insight-tls
```

### Local dev without auth

```bash
helm install insight-gw ./helm --set authDisabled=true
```

## Architecture

```text
Client → Ingress → API Gateway → [Auth Middleware] → Service Modules
                                       │
                                       ├── OIDC Plugin (JWT validation via JWKS)
                                       ├── AuthZ Resolver (RBAC + org scoping)
                                       └── Tenant Resolver (workspace isolation)
```

The gateway is a cyberfabric-core ModKit server binary that links:

| Module | Purpose |
|--------|---------|
| `api-gateway` | Axum HTTP server, routing, OpenAPI, CORS, rate limiting |
| `oidc-authn-plugin` | JWT validation against OIDC provider |
| `authn-resolver` | Authentication gateway (delegates to OIDC plugin) |
| `authz-resolver` | Authorization (static plugin now, org-tree plugin later) |
| `tenant-resolver` | Multi-tenant workspace isolation |
| `grpc-hub` | Internal gRPC communication |
| `module-orchestrator` | Module lifecycle management |
| `types-registry` | GTS type/plugin discovery |

## Health endpoints

Provided by cyberfabric `api-gateway` module (public, no auth required):

| Endpoint | Response | Purpose |
|----------|----------|---------|
| `GET /health` | `{"status":"healthy","timestamp":"..."}` | Liveness + readiness probe |
| `GET /healthz` | `ok` | Simple liveness check |

Both are registered as public routes — no JWT token needed. Used by Kubernetes liveness and readiness probes.

## Building

```bash
cd src/backend
cargo build --release --bin insight-api-gateway
```

Docker (build context is `cf/` parent directory containing both repos):

```bash
cd /path/to/cf
docker build -f insight/src/backend/services/api-gateway/Dockerfile -t insight-api-gateway:dev .
```

See `Dockerfile` for the full multi-stage build (protoc, non-root user, bookworm-slim runtime).
