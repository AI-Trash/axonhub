# 图像生成 API

## 概述

AxonHub 通过 OpenAI 兼容的 `/v1/images/generations` 端点支持图像生成功能。

**注意**：图像生成目前不支持流式传输。

## API 使用

要生成图像，请向 `/v1/images/generations` 端点发送请求。

### 示例

```python
import requests
import json

url = "https://your-axonhub-instance/v1/images/generations"
headers = {
    "Authorization": f"Bearer {API_KEY}",
    "Content-Type": "application/json"
}

payload = {
    "model": "gpt-image-1",
    "prompt": "Generate a beautiful sunset over mountains",
    "size": "1024x1024",
    "quality": "high",
    "n": 1
}

response = requests.post(url, headers=headers, json=payload)
result = response.json()

# 访问生成的图像
for image in result.get("data", []):
    if "b64_json" in image:
        print(f"图像 (base64): {image['b64_json'][:50]}...")
    if "url" in image:
        print(f"图像 URL: {image['url']}")
    if "revised_prompt" in image:
        print(f"优化后的提示词: {image['revised_prompt']}")
```

```typescript
const response = await fetch("https://your-axonhub-instance/v1/images/generations", {
  method: "POST",
  headers: {
    Authorization: `Bearer ${API_KEY}`,
    "Content-Type": "application/json",
  },
  body: JSON.stringify({
    model: "gpt-image-1",
    prompt: "Generate a beautiful sunset over mountains",
    size: "1024x1024",
    quality: "high",
    n: 1,
  }),
});

const result = await response.json();

// 访问生成的图像
if (result.data) {
  result.data.forEach((image, index) => {
    if (image.b64_json) {
      console.log(`图像 ${index + 1} (base64): ${image.b64_json.substring(0, 50)}...`);
    }
    if (image.url) {
      console.log(`图像 ${index + 1} URL: ${image.url}`);
    }
    if (image.revised_prompt) {
      console.log(`优化后的提示词: ${image.revised_prompt}`);
    }
  });
}
```

## 响应格式

```json
{
  "created": 1699000000,
  "data": [
    {
      "b64_json": "iVBORw0KGgoAAAANSUhEUgAA...",
      "url": "https://...",
      "revised_prompt": "A beautiful sunset over mountains with orange and purple sky"
    }
  ]
}
```

## 请求参数

| 参数 | 类型 | 描述 | 默认值 |
|-----------|------|-------------|---------|
| `prompt` | string | **必填。** 所需图像的文本描述。 | - |
| `model` | string | 用于图像生成的模型。 | `dall-e-2` |
| `n` | integer | 要生成的图像数量。 | 1 |
| `quality` | string | 图像质量：`"standard"`、`"hd"`、`"high"`、`"medium"`、`"low"` 或 `"auto"`。 | `"auto"` |
| `response_format` | string | 返回图像的格式：`"url"` 或 `"b64_json"`。 | `"b64_json"` |
| `size` | string | 生成图像的尺寸：`"256x256"`、`"512x512"` 或 `"1024x1024"`。 | `"1024x1024"` |
| `style` | string | 生成图像的风格（仅 DALL-E 3）：`"vivid"` 或 `"natural"`。 | - |
| `user` | string | 代表最终用户的唯一标识符。 | - |
| `background` | string | 背景样式：`"opaque"` 或 `"transparent"`。 | - |
| `output_format` | string | 图像格式：`"png"`、`"webp"` 或 `"jpeg"`。 | `"png"` |
| `output_compression` | number | 压缩级别 (0-100%)。 | 100 |
| `moderation` | string | 内容审核级别：`"low"` 或 `"auto"`。 | - |
| `partial_images` | number | 要生成的部分图像数量。 | 1 |

## 图像编辑边界（`/v1/images/edits`）

`POST /v1/images/edits` 在当前 Rust-canonical 后端状态下仍然是显式未支持边界。请求这个端点时，Rust 会返回结构化的 `501 Not Implemented` 响应，而不是执行局部重绘或 multipart 图像编辑。

如果你的客户端仍然调用 `/v1/images/edits`，应当预期得到与其他显式未支持能力相同风格的边界响应：

```json
{
  "error": "not_implemented",
  "message": "当前 Rust 后端尚未实现图像编辑能力。",
  "path": "/v1/images/edits",
  "status": 501
}
```

本页刻意只描述当前已接受的真实状态。请使用 `/v1/images/generations` 完成受支持的图像生成流程，不要把 `/v1/images/edits` 当作当前后端中的可用端点。

## 支持的提供商

| 提供商 | 状态 | 支持的模型 | 备注 |
| -------------------- | ------- | ------------------------------------------------------------- | --------------------- |
| **OpenAI** | ✅ 完成 | gpt-image-1、dall-e-2、dall-e-3 等 | 不支持流式传输 |
| **字节跳动豆包** | ✅ 完成 | doubao-seed-dream-4-0 等 | 不支持流式传输 |
| **OpenRouter** | ✅ 完成 | gpt-image-1、gemini-2.5-flash-image-preview 等 | 不支持流式传输 |
| **Gemini** | ✅ 完成 | gemini-2.5-flash-image、gemini-2.0-flash-preview-image-generation 等 | 不支持流式传输 |
| **ZAI** | ✅ 完成 | - | 仅支持生成，不支持编辑 |

## 相关资源

- [OpenAI API](openai-api.md)
- [Anthropic API](anthropic-api.md)
- [Gemini API](gemini-api.md)
- [Claude Code 集成](../guides/claude-code-integration.md)
