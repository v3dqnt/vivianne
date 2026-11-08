# Images: generate_image

Two ways a picture gets made, and which one you're in depends on the model the user picked.

- **The user picked an image model** (in the model picker: gpt-image, Imagen, FLUX, a model on this computer…). Their message is the prompt and the picture is the reply. You aren't involved: there's no text model to call tools.
- **The user picked a text model** (you). You draw by calling `generate_image`.

## The call

```json
{ "prompt": "A flat vector illustration of a lighthouse at dusk, teal and orange, no text" }
```

That uses OpenAI's `gpt-image-1`. Name another provider for another model:

```json
{ "prompt": "…", "provider": "google", "model": "imagen-4.0-generate-001" }
```

Providers: `openai`, `google`, `xai`, `together`, `nebius`, `nvidia-nim`, `openrouter`. Each needs its API key in Settings → Providers; without one the tool says which key is missing, so tell the user that, don't retry.

## A model on this computer

`provider: "sd"` runs stable-diffusion.cpp. `model` is the file the user downloaded (Settings → Local models → Image generation):

```json
{ "prompt": "a watercolor fox in a snowy wood", "provider": "sd", "model": "sd_turbo.safetensors" }
```

If it isn't installed or the file is missing, the error says what to set up. Local models are slow on a CPU and small ones draw at 512×512: ask for what the model can do.

## `size`

OpenAI, Together, Nebius, NVIDIA and local models take `"size": "1024x1024"` (also `1536x1024`, `1024x1536` for OpenAI). Local 1.x models cap at 512–768 on a side and are resized to what they were trained on.

## Writing the prompt

- Say the subject, the style, the palette, the framing: "a flat vector lighthouse at dusk, teal and orange, wide shot".
- Say what to leave out ("no text") when it matters. Image models spell badly: put words on a graphic in a document instead (`create_document`), not in the picture.
- One picture per call. For a set, call again with a changed prompt.

## After

Say where it went and what you made. The image is in the chat; a copy of local ones is saved in `~/.viv/images`. Don't describe a picture you haven't looked at.
