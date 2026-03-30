# Rust Server: Static File Serving Architecture Exploration

**Date:** March 30, 2026  
**Objective:** Identify the files/modules to change, route registration points, and existing patterns for implementing rust-embed based frontend asset serving.

---

## Executive Summary

The Rust server backend (`crates/axonhub-http`) uses **Actix-web** for HTTP handling with a modular routing architecture. Currently, the server has **no static file serving or SPA fallback behavior implemented**. The legacy Go backend (`internal/server/static/embed.go`) provides a reference implementation using `embed.FS` and `gin-contrib/static`.

**Key Finding:** The Rust server is currently an API-only gateway. Static file serving needs to be added as a new feature following the established patterns found in the Go implementation.

---

## Current Rust Architecture

### HTTP Layer Structure

**Root Package:** `crates/axonhub-http/src/`

```
├── lib.rs              # Public API exports
├── routes.rs           # Route configuration and registration
├── handlers/           # HTTP request handlers
│   ├── mod.rs
│   ├── admin.rs
│   ├── anthropic.rs
│   ├── doubao.rs
│   ├── gemini.rs
│   ├── graphql.rs
│   ├── jina.rs
│   ├── openai_v1.rs
│   └── provider_edge.rs
├── middleware.rs       # HTTP middleware (auth, context, metrics)
├── state.rs           # HTTP state and capabilities
├── models.rs          # Data models and types
├── ports.rs           # Service port/trait definitions
├── errors.rs          # Error handling and responses
├── transport.rs       # Response transformation
└── tests.rs           # Tests
```

### HTTP Server Entry Point

**File:** `apps/axonhub-server/src/app/server.rs`

- **Function:** `start_server()` (line 12)
- **Setup Flow:**
  1. Load config via `axonhub_config::load()`
  2. Build server capabilities
  3. Create `HttpState` with all service ports
  4. Configure metrics runtime
  5. Create `HttpServer` with router via `router_with_metrics_and_base_path()`
  6. Bind to address and run with signal handling

**Key Details:**
- Entry point uses `HttpServer::new()` from `actix-web`
- Router is built in `apps/axonhub-server/src/app/server.rs` lines 69-74
- Base path support exists via `base_path` config (line 68)
- Signal handling for graceful shutdown (lines 104-106)

### Route Registration Architecture

**File:** `crates/axonhub-http/src/routes.rs`

#### Public Router Functions (exported via lib.rs)

1. **`router(state: HttpState)`** (line 249)
   - Basic router without metrics
   - Returns `App` configured with all routes

2. **`router_with_metrics(state, capability)`** (line 263)
   - Adds HTTP metrics middleware
   - Delegates to `router_with_metrics_and_base_path` with default "/"

3. **`router_with_metrics_and_base_path(state, capability, base_path)`** (line 278)
   - **PRIMARY ENTRY POINT** for route configuration
   - Normalizes base path
   - Wraps all routes under base_path scope if not "/"
   - Sets app data (state)
   - Applies metrics middleware (line 296)
   - Configures all routes via `configure_http_routes(cfg)` (lines 297-302)
   - Sets default service for 404 handling (lines 304-307)

#### Route Configuration Function

**`configure_http_routes(cfg: &mut ServiceConfig)`** (line 203)

Registers all API route scopes:

| Route | Handler | Auth |
|-------|---------|------|
| `/health` | `handlers::health` | None |
| `/admin/*` | `configure_admin_public/protected` | JWT for protected routes |
| `/v1/*` | `configure_openai_v1` | API Key |
| `/jina/v1/*` | `configure_jina` | API Key |
| `/anthropic/v1/*` | `configure_anthropic` | API Key |
| `/doubao/*` | `configure_doubao` | API Key |
| `/gemini/*` | `configure_gemini` | Gemini Auth |
| `/v1beta/*` | `configure_v1beta` | Gemini Auth |
| `/openapi/*` | `configure_openapi` | Service API Key |

#### Error Handling

**404 and 501 Patterns:**
- Line 304-307: `default_service` returns `not_implemented_response()` for unmapped routes
- Explicit 501s for known unsupported boundaries (`/v1/images/edits`, `/v1/realtime`)

#### Middleware Pattern

Routes use middleware wrapping:
```rust
.wrap(request_context())      // Adds request context (tracing, thread ID)
.wrap(api_key_auth())         // Validates API key
```

Middleware chain is applied via `.wrap()` in scope configuration.

---

## Current HTTP Error Handling Patterns

**File:** `crates/axonhub-http/src/errors.rs`

### Error Response Types

1. **`NotImplementedJsonResponse`** (lines 79-101)
   - Status: `StatusCode::NOT_IMPLEMENTED` (501)
   - Body: `NotImplementedResponse` model
   - Method: `.from_route()` for route-based construction
   - Pattern: `not_implemented_response("/*", method, uri, None)`

2. **Generic Error Response** (line 25)
   - Function: `error_response(status, kind, message)`
   - Returns `HttpResponse` with JSON body

3. **Compatibility Error Response** (line 54)
   - Translates errors via `translate_compatibility_error()`

### Response Building Pattern

Using `HttpResponseBuilder`:
```rust
let status = StatusCode::from_u16(payload.status)?;
HttpResponseBuilder::new(status).json(payload.body)
```

---

## Default Service and 404 Handling

**Pattern in routes.rs:**

```rust
// Line 304-307: Default service for unmapped routes
.default_service(web::to(|request: actix_web::HttpRequest| async move {
    not_implemented_response("/*", request.method().clone(), request.uri().clone(), None)
        .into_response()
}))
```

This is where **static file serving or SPA fallback should be integrated**:
- Currently returns 501 Not Implemented
- Should check if request is for static file or SPA route
- Serve file if exists, fallback to `index.html` for SPA routes

---

## Legacy Go Implementation (Reference Model)

**File:** `internal/server/static/embed.go`

### Embedding Pattern

```go
//go:embed all:dist/*
var dist embed.FS

// Initialize EmbedFolder wrapper
staticFS, err = static.EmbedFolder(dist, "dist")
```

### Static Detection Logic (lines 40-63)

```go
func shouldServeStatic(path string) bool {
    // Exact patterns for static assets
    if strings.HasPrefix(path, "/assets/") ||
        strings.HasPrefix(path, "/images/") ||
        path == "/favicon.ico" ||
        strings.HasSuffix(path, ".js") ||
        strings.HasSuffix(path, ".css") ||
        // ... more extensions
        return true
    
    if path == "/" {
        return true  // Serve root
    }
    
    return false  // SPA fallback
}
```

### Handler Logic (lines 25-37)

```go
func Handler() gin.HandlerFunc {
    return func(c *gin.Context) {
        if shouldServeStatic(c.Request.URL.Path) {
            static.Serve("/", staticFS)(c)  // Direct file serve
        } else {
            // SPA fallback with no-cache headers
            c.Header("Cache-Control", "no-cache, no-store, must-revalidate")
            c.Header("Pragma", "no-cache")
            c.Header("Expires", "0")
            c.FileFromFS("/", staticFS)     // Serve index.html
        }
    }
}
```

### Registration

In `internal/server/routes.go` line 49:
```go
server.NoRoute(static.Handler())  // Catch-all at end of route setup
```

---

## Frontend Build Output

**Location:** `/Users/summpot/Documents/GitHub/axonhub/frontend/dist/`

Structure:
```
dist/
├── index.html          # SPA entry point
├── favicon.ico         # Site icon
├── logo.jpg            # Logo asset
└── assets/
    └── *.js, *.css, ...  # Vite-bundled assets
```

**Key Details:**
- Vite-based build output
- SPA React app (index.html references `/assets/` modules)
- Static asset fingerprinting (hash-based filenames like `index-UiEDXddt.js`)
- Long cache headers appropriate for versioned assets

---

## Identfied Change Points

### 1. **Cargo.toml (axonhub-http)**
- **Path:** `crates/axonhub-http/Cargo.toml`
- **Current:** No `rust-embed` dependency
- **Change:** Add `rust-embed` crate
- **Also need:** Ensure `actix-web` features support file serving

### 2. **HTTP Routes Registration**
- **File:** `crates/axonhub-http/src/routes.rs`
- **Primary Change:** Modify `router_with_metrics_and_base_path()` (line 278)
  - Before `default_service` (line 304), add static file scope
  - Pattern: Add scope at line 303 before the `.default_service()`
  
- **Function to add:** `configure_static_files(cfg: &mut ServiceConfig)`
  - Serve `/assets/*`, `/favicon.ico`, `/logo.jpg`, root `/`
  - Fallback non-matched requests to `index.html`

### 3. **New Module: Static File Handler**
- **Location:** Create `crates/axonhub-http/src/handlers/static_files.rs`
- **Responsibility:**
  - Define embedded filesystem
  - Implement static file serving logic
  - Implement SPA fallback logic
  - Match Go pattern but use Rust/Actix patterns

### 4. **Error Handling Integration**
- **File:** `crates/axonhub-http/src/errors.rs`
- **No changes needed** — existing error patterns work as-is
- Static handler should use same response patterns

### 5. **Exports**
- **File:** `crates/axonhub-http/src/lib.rs`
- **No changes needed** — static handler is internal to routes module

---

## Implementation Strategy

### Step 1: Add Dependencies
- Add `rust-embed = "8.x"` to `crates/axonhub-http/Cargo.toml`
- Verify `actix-web` has required file-serving features

### Step 2: Create Static Handler Module
- Create `crates/axonhub-http/src/handlers/static_files.rs`
- Implement:
  - `embed_static_files!()` macro invocation or `include!()` pattern
  - `serve_static_file(filename: &str) -> HttpResponse`
  - `serve_spa_fallback() -> HttpResponse` (serves index.html)
  - `is_static_path(path: &str) -> bool` (detection logic)

### Step 3: Integrate into Routes
- Update `crates/axonhub-http/src/routes.rs`:
  - Import new static handler
  - Add `configure_static_files(cfg)` function
  - Update `router_with_metrics_and_base_path()` to mount static scope before default_service

### Step 4: Testing
- Unit tests in `crates/axonhub-http/src/tests.rs`
- Test static file serving
- Test SPA fallback on unknown routes
- Test cache headers

### Step 5: Build Integration
- Frontend dist files must be in `crates/axonhub-http/src/handlers/static_files/dist/`
- Build script or post-build step to copy from `frontend/dist/`

---

## Key Rust Patterns to Match

### 1. Module Organization
- Private handler function in module
- Public exports via `lib.rs`
- Handler integration via routes module

### 2. Error Responses
```rust
HttpResponseBuilder::new(StatusCode::OK)
    .header("Content-Type", "text/html; charset=utf-8")
    .body(body)
```

### 3. Middleware Pattern (if needed for static)
```rust
.wrap(middleware)
```

### 4. Scope Registration
```rust
.service(web::scope("/assets").route("/", web::get().to(handler)))
```

### 5. Default Service Pattern (current in routes.rs)
```rust
.default_service(web::to(handler))
```

---

## Scope Boundaries

### IN SCOPE (Rust Server Only)
- Embedding frontend `dist/` folder into binary
- Serving static files with appropriate cache headers
- SPA fallback (unmatched routes → index.html)
- Integration with existing route hierarchy
- Cache headers: `no-cache` for `index.html`, long-lived for `/assets/*`

### OUT OF SCOPE
- Changes to legacy Go backend
- Modifications to Go static handler
- Build process (handled separately)
- Frontend compilation

---

## Summary Table

| Aspect | Current State | Change Required |
|--------|--------------|-----------------|
| **Embedded Assets** | None | Add `rust-embed` crate |
| **Static Route Scope** | None | Create `/assets`, `/favicon`, `/` scope |
| **SPA Fallback** | Returns 501 | Serve `index.html` for non-static routes |
| **Handler Module** | N/A | Create `handlers/static_files.rs` |
| **Error Handling** | Existing patterns | Reuse (no changes) |
| **Route Registration** | API-only | Add static scope before default_service |
| **Cache Headers** | N/A | Add per-file-type headers |
| **Testing** | Existing framework | Add static file tests |

---

## File Modifications Checklist

- [ ] `Cargo.toml` — Add dependencies
- [ ] `crates/axonhub-http/src/handlers/static_files.rs` — Create module (NEW)
- [ ] `crates/axonhub-http/src/handlers/mod.rs` — Export static_files module
- [ ] `crates/axonhub-http/src/routes.rs` — Add static scope configuration
- [ ] `crates/axonhub-http/src/tests.rs` — Add static file tests
- [ ] Build configuration (if needed for embed macro)

---

## References

- Rust Workspace: `Cargo.toml` (workspace root)
- HTTP Crate: `crates/axonhub-http/Cargo.toml`
- Route Configuration: `crates/axonhub-http/src/routes.rs` (lines 203-247)
- Server Entry: `apps/axonhub-server/src/app/server.rs` (lines 69-74)
- Legacy Reference: `internal/server/static/embed.go` (Go pattern model)
- Frontend Build: `frontend/dist/` (source files to embed)
