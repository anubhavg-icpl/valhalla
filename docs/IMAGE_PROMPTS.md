# Image Generation Prompts

These prompts are designed for AI image generators (Midjourney v6, DALL-E 3,
Stable Diffusion XL, Flux.1, etc.) to produce the visual assets referenced by
`README.md`. All images live under `docs/assets/images/`.

Copy-paste the prompt for the asset you need. Tweak the aspect ratio flag
(`--ar 16:9` for Midjourney, or set canvas size in other tools) per the
"Spec" column.

---

## 1. `hero-banner.png` - Top-of-page hero

**Spec:** 16:9, ~1920x1080. Dark, dramatic, leaves negative space at the
bottom-right for the project title to overlay.

**Prompt:**

> A cinematic, ultra-wide banner illustration of a mythical norse valhalla
> mead-hall rendered as a futuristic data center, with golden runic glyphs
> glowing on obsidian pillars, holographic windows kernel call-stack diagrams
> hovering in the air, streams of green and cyan event logs flowing like the
> aurora borealis across the ceiling, intricate norse knotwork borders glowing
> with circuit-trace patterns, volumetric god-rays, deep blacks, bronze and
> teal color palette, digital art, highly detailed, octane render, 8k,
> atmospheric, no text, no watermark, --ar 16:9 --style raw --v 6

**Negative prompt (for SDXL):**

> text, watermark, logo, signature, person, face, cartoon, low quality,
> jpeg artifacts, blurry, oversaturated

---

## 2. `architecture-diagram.png` - Workspace + data flow overview

**Spec:** ~16:9, ~1600x900. Diagrammatic, clean, technical, labeled.

**Prompt:**

> A clean isometric technical architecture diagram of a software system with
> four labeled boxes arranged in a 2x2 grid: a red "Kernel Driver" box showing
> notification callbacks feeding into a ring buffer, a blue "User-Mode Client"
> box showing a file read loop, a green "Shared Protocol" box showing a C-style
> enum, and a purple "Build Orchestrator" box. Arrows flow between them in
> data-flow order. Flat vector style, soft drop shadows, sans-serif labels,
> light slate background with subtle grid, teal and amber accent colors,
> infographic, devops diagram, high detail, --ar 16:9 --v 6

---

## 3. `data-flow.png` - Kernel -> user event pipeline

**Spec:** ~16:9, ~1280x720. Flowchart style, left-to-right.

**Prompt:**

> A horizontal left-to-right technical flowchart diagram of a kernel-to-user
> event pipeline, showing nodes for "Kernel Notification APIs" on the left,
> "Callback Functions" in the middle-left, a "Mutex-Protected Event Ring" in
> the center depicted as a circular buffer with 256 slots, an "MDL Read IRP"
> on the middle-right, and a "Client Process" on the far right. Connected by
> labeled arrows reading "callback", "push", "ReadFile", "drain". Flat design,
> minimal, professional, blue-green color scheme on a near-white background,
> sans-serif typography, infographic style, no photographic elements,
> --ar 16:9 --v 6

---

## 4. `docs/assets/images/logo.png` (optional) - Project logo

**Spec:** 1:1, ~512x512, transparent or dark background. Minimal, scalable.

**Prompt:**

> A minimalist app logo combining a stylized norse valknut symbol with a
> microchip and a magnifying glass, single weight line art, glowing teal lines
> on a deep navy background, flat vector, centered, symmetrical, modern,
> geometric, no text, no letters, premium tech brand identity, --ar 1:1
> --v 6

---

## Usage notes

- For **Midjourney**, append the `--ar` and `--v 6` flags as shown.
- For **DALL-E 3** (via ChatGPT or API), drop the flags; specify canvas size in
  words ("wide 16:9 banner", "square icon").
- For **Stable Diffusion XL / Flux.1**, set the canvas dimensions directly and
  use the negative prompt where provided.
- After generation, downscale and run through a PNG optimizer
  (`oxipng`/`pngcrush`) before committing - READMEs should stay under a few
  hundred KB per image.

## Replacing the placeholder references

`README.md` references these paths:

```
docs/assets/images/hero-banner.png
docs/assets/images/architecture-diagram.png
docs/assets/images/data-flow.png
```

Drop the generated files at those paths and GitHub will render them inline.
