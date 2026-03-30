# Legacy Go Backend: Frontend Static File Serving Analysis

## Summary

The legacy Go backend embeds frontend static files using Go's native `embed` package with gin-contrib's `static` middleware. The frontend is built at compile time and embedded directly into the binary.

---

## Implementation Details

### 1. **File Embedding**

**Location:** `internal/server/static/embed.go`

```go
//go:embed all:dist/*
var dist embed.FS
```

- Uses Go's `//go:embed` directive to embed all files from `dist/*` recursively
- The embedded filesystem is stored in the `dist` variable
- This is initialized at compile time; no runtime file I/O needed for core assets

### 2. **Filesystem Wrapper**

```go
var staticFS static.ServeFileSystem

func init() {
    var err error
    staticFS, err = static.EmbedFolder(dist, "dist")
    if err != nil {
        panic(err)
    }
}
```

- Uses `gin-contrib/static.EmbedFolder()` to wrap the embedded filesystem
- Converts the Go `embed.FS` into a Gin-compatible `ServeFileSystem`
- Panics if initialization fails (fail-fast approach)

### 3. **Request Routing & Handler**

**Location:** `internal/server/routes.go` (line 49)

```go
func SetupRoutes(server *Server, handlers Handlers, client *ent.Client, services Services) {
    // Serve static frontend files
    server.NoRoute(static.Handler())
    // ... other routes ...
}
```

- **`NoRoute()`** catches all unmatched requests
- Registered as the **last handler** in the route chain (fallback)
- This is a Gin pattern: `NoRoute()` is called when no matching route is found

### 4. **Path-Based Decision Logic**

**Location:** `internal/server/static/embed.go` (lines 40-63)

```go
func shouldServeStatic(path string) bool {
    // Always serve static assets
    if strings.HasPrefix(path, "/assets/") ||
        strings.HasPrefix(path, "/images/") ||
        path == "/favicon.ico" ||
        strings.HasSuffix(path, ".js") ||
        strings.HasSuffix(path, ".css") ||
        strings.HasSuffix(path, ".png") ||
        strings.HasSuffix(path, ".jpg") ||
        strings.HasSuffix(path, ".jpeg") ||
        strings.HasSuffix(path, ".gif") ||
        strings.HasSuffix(path, ".svg") ||
        strings.HasSuffix(path, ".webp") {
        return true
    }

    // Serve root path
    if path == "/" {
        return true
    }

    // Everything else is an SPA route
    return false
}
```

**Behavior:**

1. **Static Assets** (served via `static.Serve("/", staticFS)`):
   - Directory prefixes: `/assets/`, `/images/`
   - File extensions: `.js`, `.css`, `.png`, `.jpg`, `.jpeg`, `.gif`, `.svg`, `.webp`
   - Special: `/favicon.ico`
   - Root path: `/`
   - These receive normal static file serving with appropriate content types

2. **SPA Routes** (everything else):
   - Paths that don't match the above patterns
   - Served as **index.html** with specific cache headers
   - Enables client-side routing in the React app

### 5. **SPA Fallback & Caching**

**Location:** `internal/server/static/embed.go` (lines 25-38)

```go
func Handler() gin.HandlerFunc {
    return func(c *gin.Context) {
        // Check if the request is for an API route or static file
        if shouldServeStatic(c.Request.URL.Path) {
            static.Serve("/", staticFS)(c)
        } else {
            // For SPA routes, serve the index.html
            c.Header("Cache-Control", "no-cache, no-store, must-revalidate")
            c.Header("Pragma", "no-cache")
            c.Header("Expires", "0")
            c.FileFromFS("/", staticFS)
        }
    }
}
```

**Cache Control:**

- **Static assets** (CSS, JS, images, etc.): Normal caching (controlled by `gin-contrib/static`)
- **SPA routes** (index.html fallback): **Aggressive no-cache headers**
  - `Cache-Control: no-cache, no-store, must-revalidate`
  - `Pragma: no-cache`
  - `Expires: 0`
  - This prevents browsers from caching outdated index.html

### 6. **Content Type Handling**

- **Via `gin-contrib/static`**: Automatic MIME type detection based on file extension
  - Common types: `text/html`, `application/javascript`, `text/css`, `image/*`
- **Via `c.FileFromFS()`**: Also handled by Gin with standard MIME type mapping

### 7. **Favicon Handling**

**Separate embedded asset:**
- **Location:** `internal/server/assets/favicon.go` + `internal/server/assets/favicon.ico`
- Uses a **separate embed** directive for the favicon
- Served via an explicit API endpoint: `GET /favicon`
- Handler in `internal/server/api/system.go` (GetFavicon method):
  - Returns custom brand logo if set (base64-encoded data URL)
  - Falls back to default embedded `favicon.ico` with `Content-Type: image/x-icon`
  - Cache: `public, max-age=3600` (1 hour)

---

## Build-Time Assumptions

### Frontend Build

**Location:** `Makefile` (lines 35-42)

```makefile
build-frontend:
    @echo "Building axonhub frontend..."
    cd frontend && pnpm vite build
    @echo "Copying frontend dist to server static directory..."
    rm -rf internal/server/static/dist/assets
    mkdir -p internal/server/static/dist
    cp -r frontend/dist/* internal/server/static/dist/
    @echo "Frontend build completed!"
```

**Process:**

1. Run `pnpm vite build` in the `frontend/` directory
   - Outputs built assets to `frontend/dist/`
   - Typical Vite structure:
     - `index.html` (entry point)
     - `assets/` (hashed JS/CSS/other bundles)
     - `logo.jpg`, `favicon.ico` (if included)

2. Copy entire `frontend/dist/` to `internal/server/static/dist/`
   - Creates the source directory for `//go:embed all:dist/*`

### Compile-Time Embedding

```go
//go:embed all:dist/*
var dist embed.FS
```

- The `go:embed` directive is processed at **compile time** by `go build`
- Requires `internal/server/static/dist/` to exist before compilation
- If `dist/` doesn't exist, `go build` fails

---

## Path Normalization & Fallback Behavior

### Path Normalization

- **No explicit path normalization** in the static handler code
- Relies on `gin-contrib/static.Serve()` and Gin's built-in normalization
- Typically handles:
  - Removing duplicate slashes
  - Resolving `..` and `.` segments (if enabled in Gin)

### Fallback Behavior

1. **All API routes** (e.g., `/v1/chat/completions`, `/admin/graphql`, `/health`) are defined **before** the `NoRoute()` handler
   - These match their explicit route definitions and never reach the static handler

2. **Unknown paths** fall through to `static.Handler()` → `NoRoute`
   - If path matches `shouldServeStatic()` → serve as static file
   - Otherwise → serve `index.html` (SPA fallback)
   - Allows React Router to handle client-side routing

---

## Route Registration Order

**Location:** `internal/server/routes.go`

**Execution order:**

1. **CORS middleware** (if enabled)
2. **Tracing/logging middleware**
3. **Public routes** (health, system status, no auth)
4. **Admin routes** (JWT authenticated)
5. **OpenAPI routes** (OpenAPI authenticated)
6. **API routes** (API key authenticated)
7. **NoRoute handler** ← `static.Handler()` (last resort for everything else)

This order ensures:
- API requests never accidentally hit the static handler
- Frontend SPA gets all unmatched paths with proper fallback

---

## Exact Behavior Summary

| Scenario | Behavior | Headers |
|----------|----------|---------|
| `GET /assets/js/main-abc123.js` | Serve from `dist/assets/js/main-abc123.js` | `Content-Type: application/javascript`, standard cache |
| `GET /favicon.ico` | Serve from `dist/favicon.ico` | `Content-Type: image/x-icon` |
| `GET /` | Serve `dist/index.html` | `Cache-Control: no-cache, no-store, must-revalidate` |
| `GET /dashboard` (SPA route) | Serve `dist/index.html` | `Cache-Control: no-cache, no-store, must-revalidate` |
| `GET /some-page.png` | Serve from `dist/some-page.png` | `Content-Type: image/png`, standard cache |
| `GET /api/health` | Handled by explicit `/health` route | API response |
| `GET /v1/models` | Handled by explicit `/v1` route group | API response |
| `POST /admin/graphql` | Handled by explicit `/admin` route group | API response |

---

## Key Decisions for Rust Implementation

1. **Embed Strategy**: Use Rust `include_bytes!()` or tower-http's static file serving with runtime embedding
2. **Path Matching**: Replicate the exact `shouldServeStatic()` logic for consistent behavior
3. **SPA Fallback**: Serve index.html for unmapped paths with same no-cache headers
4. **Route Order**: Register static handler as the **last** middleware/route (after all APIs)
5. **Cache Headers**:
   - Static assets: Standard/default caching
   - SPA index.html: Aggressive no-cache
   - Favicon: 1-hour cache with public flag
6. **Content Types**: Automatically infer from file extensions (tower-http handles this)
7. **Separate Favicon**: May keep favicon as a separate endpoint for customization

---

## Files Involved

| File | Purpose |
|------|---------|
| `internal/server/static/embed.go` | Core static file serving logic |
| `internal/server/routes.go` | Route registration with NoRoute handler |
| `internal/server/assets/favicon.go` | Embedded favicon asset |
| `internal/server/assets/favicon.ico` | Default favicon binary |
| `internal/server/api/system.go` | GetFavicon API handler (separate endpoint) |
| `Makefile` | Frontend build and copy process |
| `frontend/dist/` | Built frontend assets (source for embedding) |
| `cmd/axonhub/main.go` | Server startup (uses embedded static files) |

---

## Dependencies

- **`github.com/gin-contrib/static`**: Gin static file serving middleware with embed support
- **Go standard library `embed`**: Native embedding mechanism
- **Vite**: Frontend build tool (builds to `frontend/dist/`)
- **Gin**: Web framework with automatic MIME type handling
