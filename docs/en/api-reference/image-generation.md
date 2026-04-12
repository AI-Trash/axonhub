# Image Generation API

## Overview

AxonHub supports image generation via the OpenAI-compatible `/v1/images/generations` endpoint.

**Note**: Streaming is not currently supported for image generation.

## API Usage

To generate images, send a request to the `/v1/images/generations` endpoint.

### Example

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

# Access generated images
for image in result.get("data", []):
    if "b64_json" in image:
        print(f"Image (base64): {image['b64_json'][:50]}...")
    if "url" in image:
        print(f"Image URL: {image['url']}")
    if "revised_prompt" in image:
        print(f"Revised prompt: {image['revised_prompt']}")
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

// Access generated images
if (result.data) {
  result.data.forEach((image, index) => {
    if (image.b64_json) {
      console.log(`Image ${index + 1} (base64): ${image.b64_json.substring(0, 50)}...`);
    }
    if (image.url) {
      console.log(`Image ${index + 1} URL: ${image.url}`);
    }
    if (image.revised_prompt) {
      console.log(`Revised prompt: ${image.revised_prompt}`);
    }
  });
}
```

## Response Format

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

## Request Parameters

| Parameter | Type | Description | Default |
|-----------|------|-------------|---------|
| `prompt` | string | **Required.** A text description of the desired image(s). | - |
| `model` | string | The model to use for image generation. | `dall-e-2` |
| `n` | integer | The number of images to generate. | 1 |
| `quality` | string | The quality of the image: `"standard"`, `"hd"`, `"high"`, `"medium"`, `"low"`, or `"auto"`. | `"auto"` |
| `response_format` | string | The format in which to return the images: `"url"` or `"b64_json"`. | `"b64_json"` |
| `size` | string | The size of the generated images: `"256x256"`, `"512x512"`, or `"1024x1024"`. | `"1024x1024"` |
| `style` | string | The style of the generated images (DALL-E 3 only): `"vivid"` or `"natural"`. | - |
| `user` | string | A unique identifier representing your end-user. | - |
| `background` | string | Background style: `"opaque"` or `"transparent"`. | - |
| `output_format` | string | Image format: `"png"`, `"webp"`, or `"jpeg"`. | `"png"` |
| `output_compression` | number | Compression level (0-100%). | 100 |
| `moderation` | string | Content moderation level: `"low"` or `"auto"`. | - |
| `partial_images` | number | Number of partial images to generate. | 1 |

## Image Edits (`/v1/images/edits`)

`POST /v1/images/edits` is wired in the current Rust backend and does not map to a fixed `501 Not Implemented` boundary.

This endpoint expects a valid multipart image-edit request body, for example fields such as `model`, `prompt`, and `image`. Malformed or incomplete payloads can still fail with request errors such as `400 Bad Request`.

Support remains provider and model dependent, so treat image edits as a supported route family with normal upstream capability limits, not as guaranteed parity across every configured channel.

## Supported Providers

| Provider             | Status  | Supported Models                                              | Notes                 |
| -------------------- | ------- | ------------------------------------------------------------- | --------------------- |
| **OpenAI**           | ✅ Done | gpt-image-1, dall-e-2, dall-e-3, etc.                         | No streaming support  |
| **ByteDance Doubao** | ✅ Done | doubao-seed-dream-4-0, etc.                                   | No streaming support  |
| **OpenRouter**       | ✅ Done | gpt-image-1, gemini-2.5-flash-image-preview, etc.             | No streaming support  |
| **Gemini**           | ✅ Done | gemini-2.5-flash-image, gemini-2.0-flash-preview-image-generation, etc. | No streaming support  |
| **ZAI**              | ✅ Done | -                                                             | Generation only, no edit support |

## Related Resources

- [OpenAI API](openai-api.md)
- [Anthropic API](anthropic-api.md)
- [Gemini API](gemini-api.md)
- [Claude Code Integration](../guides/claude-code-integration.md)
