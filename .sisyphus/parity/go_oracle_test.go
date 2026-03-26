package parity

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"testing"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/zhenzou/executors"

	"github.com/looplj/axonhub/internal/authz"
	"github.com/looplj/axonhub/internal/contexts"
	"github.com/looplj/axonhub/internal/ent"
	"github.com/looplj/axonhub/internal/ent/apikey"
	"github.com/looplj/axonhub/internal/ent/enttest"
	"github.com/looplj/axonhub/internal/ent/model"
	"github.com/looplj/axonhub/internal/ent/project"
	"github.com/looplj/axonhub/internal/ent/user"
	"github.com/looplj/axonhub/internal/objects"
	api "github.com/looplj/axonhub/internal/server/api"
	"github.com/looplj/axonhub/internal/server/biz"
	"github.com/looplj/axonhub/internal/server/gql/openapi"
	"github.com/looplj/axonhub/internal/pkg/xcache"
	"github.com/looplj/axonhub/llm/httpclient"
)

type oracleFixture struct {
	Request               oracleRequest `json:"request"`
	Model                 *oracleModel  `json:"model,omitempty"`
	NormalizeGeneratedKey bool          `json:"normalize_generated_key,omitempty"`
}

type oracleRequest struct {
	Method  string            `json:"method"`
	Path    string            `json:"path"`
	Headers map[string]string `json:"headers,omitempty"`
	Body    string            `json:"body,omitempty"`
}

type oracleModel struct {
	Developer string `json:"developer"`
	ModelID   string `json:"model_id"`
	ModelType string `json:"model_type"`
	Name      string `json:"name"`
	Icon      string `json:"icon"`
	Group     string `json:"group"`
	Remark    string `json:"remark"`
}

type oracleOutput struct {
	Suite       string            `json:"suite"`
	Status      int               `json:"status"`
	Headers     map[string]string `json:"headers,omitempty"`
	Body        any               `json:"body,omitempty"`
	ContentType string            `json:"content_type,omitempty"`
}

func TestParityOracleEmitSuite(t *testing.T) {
	suite := os.Getenv("AXONHUB_PARITY_SUITE")
	if suite == "" {
		return
	}

	fixturePath := os.Getenv("AXONHUB_PARITY_FIXTURE")
	capturePath := os.Getenv("AXONHUB_PARITY_CAPTURE")
	if fixturePath == "" || capturePath == "" {
		t.Fatalf("AXONHUB_PARITY_FIXTURE and AXONHUB_PARITY_CAPTURE are required")
	}

	fixture := loadFixture(t, fixturePath)
	output := emitSuite(t, suite, fixture)
	writeOutput(t, capturePath, output)
}

func loadFixture(t *testing.T, fixturePath string) oracleFixture {
	t.Helper()
	var fixture oracleFixture
	bytes, err := os.ReadFile(filepath.Clean(fixturePath))
	if err != nil {
		t.Fatalf("read fixture: %v", err)
	}
	if err := json.Unmarshal(bytes, &fixture); err != nil {
		t.Fatalf("unmarshal fixture: %v", err)
	}
	return fixture
}

func emitSuite(t *testing.T, suite string, fixture oracleFixture) oracleOutput {
	t.Helper()
	gin.SetMode(gin.TestMode)

	switch suite {
	case "admin_system_status_initial":
		return emitAdminSystemStatusInitial(t, fixture)
	case "v1_models_basic":
		return emitV1ModelsBasic(t, fixture)
	case "openapi_graphql_create_llm_api_key":
		return emitOpenApiGraphqlCreateLLMAPIKey(t, fixture)
	default:
		t.Fatalf("unsupported parity suite %q", suite)
		return oracleOutput{}
	}
}

func emitAdminSystemStatusInitial(t *testing.T, fixture oracleFixture) oracleOutput {
	t.Helper()
	client := enttest.Open(t, "sqlite3", "file:parity-admin-status?mode=memory&_fk=1")
	t.Cleanup(func() { _ = client.Close() })

	cacheConfig := xcache.Config{Mode: xcache.ModeMemory}
	systemService := biz.NewSystemService(biz.SystemServiceParams{CacheConfig: cacheConfig, Ent: client})
	handlers := api.NewSystemHandlers(api.SystemHandlersParams{SystemService: systemService})

	router := gin.New()
	router.GET(fixture.Request.Path, handlers.GetSystemStatus)

	recorder := httptest.NewRecorder()
	request := httptest.NewRequest(http.MethodGet, fixture.Request.Path, nil)
	router.ServeHTTP(recorder, request)

	return buildHTTPOutput(t, "admin_system_status_initial", recorder)
}

func emitV1ModelsBasic(t *testing.T, fixture oracleFixture) oracleOutput {
	t.Helper()
	client := enttest.Open(t, "sqlite3", "file:parity-v1-models?mode=memory&_fk=1")
	t.Cleanup(func() { _ = client.Close() })

	cacheConfig := xcache.Config{Mode: xcache.ModeMemory}
	setupCtx := authz.WithTestBypass(ent.NewContext(context.Background(), client))
	modelFixture := fixture.Model
	if modelFixture == nil {
		t.Fatal("model fixture is required")
	}

	_, err := client.Model.Create().
		SetDeveloper(modelFixture.Developer).
		SetModelID(modelFixture.ModelID).
		SetType(model.Type(modelFixture.ModelType)).
		SetName(modelFixture.Name).
		SetIcon(modelFixture.Icon).
		SetGroup(modelFixture.Group).
		SetModelCard(&objects.ModelCard{}).
		SetSettings(&objects.ModelSettings{}).
		SetStatus(model.StatusEnabled).
		SetRemark(modelFixture.Remark).
		Save(setupCtx)
	if err != nil {
		t.Fatalf("seed model: %v", err)
	}

	systemService := biz.NewSystemService(biz.SystemServiceParams{CacheConfig: cacheConfig, Ent: client})
	executor := executors.NewPoolScheduleExecutor(executors.WithMaxConcurrent(1))
	t.Cleanup(func() { _ = executor.Shutdown(context.Background()) })
	channelService := biz.NewChannelService(biz.ChannelServiceParams{
		CacheConfig:   cacheConfig,
		Executor:      executor,
		Ent:           client,
		SystemService: systemService,
		HttpClient:    httpclient.NewHttpClient(),
	})
	t.Cleanup(channelService.Stop)
	modelService := biz.NewModelService(biz.ModelServiceParams{
		ChannelService: channelService,
		SystemService:  systemService,
		Ent:            client,
	})
	handlers := &api.OpenAIHandlers{ModelService: modelService, EntClient: client}

	router := gin.New()
	router.Use(func(c *gin.Context) {
		ctx := authz.WithTestBypass(ent.NewContext(c.Request.Context(), client))
		c.Request = c.Request.WithContext(ctx)
		c.Next()
	})
	router.GET(fixture.Request.Path, handlers.ListModels)

	recorder := httptest.NewRecorder()
	request := httptest.NewRequest(http.MethodGet, fixture.Request.Path, nil)
	router.ServeHTTP(recorder, request)

	return buildHTTPOutput(t, "v1_models_basic", recorder)
}

func emitOpenApiGraphqlCreateLLMAPIKey(t *testing.T, fixture oracleFixture) oracleOutput {
	t.Helper()
	client := enttest.Open(t, "sqlite3", "file:parity-openapi-graphql?mode=memory&_fk=1")
	t.Cleanup(func() { _ = client.Close() })

	cacheConfig := xcache.Config{Mode: xcache.ModeMemory}
	setupCtx := authz.WithTestBypass(ent.NewContext(context.Background(), client))

	hashedPassword, err := biz.HashPassword("password123")
	if err != nil {
		t.Fatalf("hash password: %v", err)
	}

	ownerUser, err := client.User.Create().
		SetEmail("owner@example.com").
		SetPassword(hashedPassword).
		SetFirstName("System").
		SetLastName("Owner").
		SetStatus(user.StatusActivated).
		SetIsOwner(true).
		Save(setupCtx)
	if err != nil {
		t.Fatalf("create owner user: %v", err)
	}

	ownerProject, err := client.Project.Create().
		SetName("Default Project").
		SetDescription("Parity project").
		SetStatus(project.StatusActive).
		Save(setupCtx)
	if err != nil {
		t.Fatalf("create owner project: %v", err)
	}

	ownerAPIKey, err := client.APIKey.Create().
		SetName("Service Key").
		SetKey("service-key-123").
		SetUserID(ownerUser.ID).
		SetProjectID(ownerProject.ID).
		SetType(apikey.TypeServiceAccount).
		SetStatus(apikey.StatusEnabled).
		SetScopes([]string{"write_api_keys"}).
		Save(setupCtx)
	if err != nil {
		t.Fatalf("create owner api key: %v", err)
	}

	projectService := biz.NewProjectService(biz.ProjectServiceParams{CacheConfig: cacheConfig, Ent: client})
	apiKeyService := biz.NewAPIKeyService(biz.APIKeyServiceParams{
		CacheConfig:    cacheConfig,
		Ent:            client,
		ProjectService: projectService,
	})
	t.Cleanup(apiKeyService.Stop)
	handlers := openapi.NewGraphqlHandlers(openapi.Dependencies{Ent: client, APIKeyService: apiKeyService})

	router := gin.New()
	router.Use(func(c *gin.Context) {
		ctx := ent.NewContext(c.Request.Context(), client)
		ctx = contexts.WithAPIKey(ctx, ownerAPIKey)
		c.Request = c.Request.WithContext(ctx)
		c.Next()
	})
	router.POST(fixture.Request.Path, func(c *gin.Context) {
		handlers.Graphql.ServeHTTP(c.Writer, c.Request)
	})

	recorder := httptest.NewRecorder()
	request := httptest.NewRequest(
		fixture.Request.Method,
		fixture.Request.Path,
		bytes.NewBufferString(fixture.Request.Body),
	)
	for key, value := range fixture.Request.Headers {
		request.Header.Set(key, value)
	}
	router.ServeHTTP(recorder, request)

	return buildHTTPOutput(t, "openapi_graphql_create_llm_api_key", recorder)
}

func buildHTTPOutput(t *testing.T, suite string, recorder *httptest.ResponseRecorder) oracleOutput {
	t.Helper()
	body := decodeBody(t, recorder.Body.Bytes())
	normalizeValue(&body)
	contentType := normalizeContentType(recorder.Header().Get("Content-Type"))
	return oracleOutput{
		Suite:       suite,
		Status:      recorder.Code,
		Headers:     map[string]string{"content-type": contentType},
		ContentType: contentType,
		Body:        body,
	}
}

func normalizeContentType(value string) string {
	return strings.TrimSpace(strings.Split(value, ";")[0])
}

func decodeBody(t *testing.T, body []byte) any {
	t.Helper()
	trimmed := bytes.TrimSpace(body)
	if len(trimmed) == 0 {
		return ""
	}
	var value any
	if err := json.Unmarshal(trimmed, &value); err == nil {
		return value
	}
	return string(trimmed)
}

func normalizeValue(value *any) {
	switch typed := (*value).(type) {
	case map[string]any:
		keys := make([]string, 0, len(typed))
		for key := range typed {
			keys = append(keys, key)
		}
		sort.Strings(keys)
		normalized := make(map[string]any, len(typed))
		for _, key := range keys {
			current := typed[key]
			normalizeValue(&current)
			if key == "created" {
				normalized[key] = "<created>"
				continue
			}
			if key == "key" {
				if stringValue, ok := current.(string); ok && strings.HasPrefix(stringValue, "ah-") {
					normalized[key] = "<generated-api-key>"
					continue
				}
			}
			normalized[key] = current
		}
		*value = normalized
	case []any:
		for index := range typed {
			current := typed[index]
			normalizeValue(&current)
			typed[index] = current
		}
		if len(typed) > 0 {
			allStrings := true
			stringsOnly := make([]string, 0, len(typed))
			for _, item := range typed {
				stringItem, ok := item.(string)
				if !ok {
					allStrings = false
					break
				}
				stringsOnly = append(stringsOnly, stringItem)
			}
			if allStrings {
				sort.Strings(stringsOnly)
				normalized := make([]any, 0, len(stringsOnly))
				for _, item := range stringsOnly {
					normalized = append(normalized, item)
				}
				*value = normalized
				return
			}
		}
		*value = typed
	case float64:
		if typed > float64(time.Now().Add(-24*time.Hour).Unix()) {
			*value = "<created>"
		}
	}
}

func writeOutput(t *testing.T, capturePath string, output oracleOutput) {
	t.Helper()
	bytes, err := json.MarshalIndent(output, "", "  ")
	if err != nil {
		t.Fatalf("marshal output: %v", err)
	}
	if err := os.WriteFile(filepath.Clean(capturePath), append(bytes, '\n'), 0o644); err != nil {
		t.Fatalf("write capture: %v", err)
	}
	fmt.Printf("wrote parity capture for %s to %s\n", output.Suite, capturePath)
}
