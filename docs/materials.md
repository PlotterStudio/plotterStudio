# Materials

A **material** is a sheet you cut or draw on (cardstock, vinyl, …) together with the settings
that work for it, per tool and operation. Those settings are called **presets**.

Rust type: `Material` in `crates/ps-library/src/model.rs`. Schema: `schemas/material/v1.json`.
Built-in examples: `crates/ps-library/builtin/materials/`.

See [Libraries overview](libraries.md) for ids, versions, storage and overlays.

## Minimal example

```json
{
  "schema_version": 1,
  "id": "acme.vinyl-matte",
  "version": "1.0.0",
  "license": "CC0-1.0",
  "name": "Matte vinyl",
  "category": "vinyl",
  "presets": []
}
```

(An empty `presets` array is valid and useful while drafting; `pstudio validate` needs the
key present to recognise the file as a material, see [Checking your work](libraries.md#checking-your-work).)

## A realistic example

```json
{
  "schema_version": 1,
  "id": "community.cardstock-80lb",
  "version": "1.0.0",
  "license": "CC0-1.0",
  "name": "Cardstock 80 lb",
  "names": { "de": "Karton 216 g/m²" },
  "category": "paper",
  "brand": "Generic",
  "thickness_mm": 0.22,
  "grip": ["standard", "strong"],
  "presets": [
    {
      "tool": "generic.drag-blade",
      "operation": "cut",
      "params": { "force": 210, "speed": 100, "passes": 1 },
      "machines": { "generic.cutter-medium": { "force": 230 } }
    },
    {
      "tool": "pen.fineliner-black-0.4",
      "operation": "draw",
      "params": { "force": 100, "speed": 100 }
    }
  ]
}
```

## Fields

### Required

`schema_version`, `id`, `version`, `name`, `license` (see
[common fields](libraries.md#entries-ids-and-versions)), plus:

| Field | Notes |
|---|---|
| `category` | Free text used for browsing and filtering: `paper`, `vinyl`, `fabric`, `cardboard`… |

### Optional

| Field | Type | Notes |
|---|---|---|
| `brand` | string | Searchable. |
| `thickness_mm` | number | Must be greater than zero when given. |
| `grip` | string[] | Mat grips that suit it (`standard`, `strong`, …). Free text. |
| `names` | object | Translated display names, keyed by language code (`"de"`, `"fr"`…). Searched along with `name`. |
| `presets` | array | Settings per tool and operation. Defaults to empty. |
| `textures` | object | Maps for the preview. See [Textures](#textures). |

### Presets

```json
{
  "tool": "generic.drag-blade",
  "operation": "cut",
  "params": { "force": 210, "speed": 100, "passes": 1 },
  "machines": { "generic.cutter-medium": { "force": 230 } }
}
```

| Field | Notes |
|---|---|
| `tool` | Id of the [tool](tools.md) this is for. Matched against the tool's `id`, with no version. |
| `operation` | The operation kind, matched exactly. Known kinds: `cut`, `kiss_cut`, `draw`, `fill`, `score`, `emboss`, `deboss`, `engrave`, `foil`, `foil_fill`, `perforate`, `wavy_cut`. |
| `params` | Parameter values, **by the tool's parameter names**: `force`, `speed`, `passes`… **Must not be empty. Values must not be negative.** |
| `machines` | Optional. Per-machine overrides keyed by machine-profile id. |

**Machine overrides.** `params` are the generic values. When compiling for a machine whose
profile id appears in `machines`, those values replace the generic ones **key by key**; keys
not listed fall back to `params`. In the example, the medium cutter gets force 230 but still speed
100 and 1 pass; any other machine gets 210. Use this where one machine needs different force
or speed for the same material, and put only the differences in the override.

**How a preset is chosen.** For each operation, the compiler finds the preset whose `tool`
equals the operation's tool id and whose `operation` equals the operation's kind. Then, for
**each parameter the tool declares**, the value comes from the first of these that has one
(`ps-model`, `resolve.rs`):

1. the operation's own override in the project,
2. the mat's override,
3. the project-wide override,
4. the preset's `machines.<machine-id>` value,
5. the preset's `params` value,
6. the tool's `default`.

A material with no matching preset simply falls through to the tool defaults.

**Forces are in grams-force (gf).** After resolution, `force` is clamped to the profile's
`force.min_gf..max_gf` and `speed` to its `max_speed_mm_s`; each change produces a warning
("clamped by machine"), never a silent edit. No other parameter is limited by the machine. So a
preset tuned for a stronger machine is safe, if not ideal, on a weaker one.

**What is not checked.** Validation does **not** verify that `tool` exists, that `operation`
is one the tool offers, that the `params` names exist on the tool, or that values lie within the
tool's `min`/`max` (the compiler does not clamp to the tool's range either). Only parameters the
tool *declares* are resolved, so a preset key that is misspelt, or that the tool does not
declare, is accepted and then silently has no effect. Check by hand against the tool's
definition.

### Textures

Optional images used to render the material in the preview.

```json
"textures": {
  "base_color": "color.png",
  "normal": "normal.png",
  "roughness": "rough.png",
  "height": "height.png",
  "metallic": "metal.png",
  "scale_mm": 100
}
```

- `scale_mm` (required, greater than zero): the physical size one tile of the texture covers.
- The five map fields are all optional; each is a **file name** in the same folder as
  `material.json` inside a package. A file name that is not present is reported as
  `texture file "…" is missing`.
- Images must be PNG, JPEG or WebP, **at most 4096 × 4096 pixels** (checked before the pixels are
  allocated, so a decompression bomb is refused cheaply). Anything larger is rejected with the
  limit in the message.
- The app converts each texture once into a GPU-ready mip chain, cached under
  `cache/textures/<blake3-of-the-file>/`, so repeated loads are fast and changing the file
  invalidates the cache automatically.

## Material packages

A **package** is how materials are shared: a `.zip` file (use the `.zip` extension so that
`pstudio validate` recognises it).

```text
my-pack.zip
├── library.json
├── cardstock-heavy/
│   ├── material.json
│   ├── color.png
│   └── normal.png
└── vinyl-matte/
    └── material.json
```

`library.json` describes the package:

```json
{
  "schema_version": 1,
  "id": "acme.starter-pack",
  "name": "Acme starter pack",
  "version": "1.0.0",
  "license": "CC-BY-4.0"
}
```

Rules:

- **A licence is mandatory.** A package with an empty `license` is reported as invalid, and
  exporting one is refused.
- Every folder containing a `material.json` is one material. Only `library.json`,
  `<folder>/material.json` and images (`.png`, `.jpg`, `.jpeg`, `.webp`) directly in a folder
  are allowed. **Any other file name makes the whole archive be refused.**
- The archive is read by a hardened reader: unsafe paths (`..`, absolute paths), oversize
  entries and extreme compression ratios are refused.
- A package with no materials is refused.
- **Every** problem in the package is collected and shown grouped by file, so you can fix them
  all in one pass. By default **nothing is imported if anything is invalid**; the import dialog
  can offer "import the valid ones" instead.
- A material is only counted as valid if all its texture files also decode within the size
  limit.

Exported packages are **deterministic**: entries are sorted and carry no timestamps, so
exporting the same content twice gives byte-identical files (good for hashes and diffs).
Export uses the *effective* material, i.e. the user's overlay is baked in.

Importing then follows the normal [conflict rules](libraries.md#installing-and-conflicts)
by `id` and `version`.

## The community index

The app can search a published index of downloadable packages (`index.json`):

```json
{
  "schema_version": 1,
  "entries": [
    {
      "id": "acme.starter-pack",
      "version": "1.0.0",
      "name": "Acme starter pack",
      "category": "paper",
      "brand": "Acme",
      "machines": ["generic.cutter-small"],
      "tools": ["generic.drag-blade"],
      "sha256": "<64 lowercase hex digits of the .zip>",
      "url": "https://example.org/acme.starter-pack-1.0.0.zip"
    }
  ]
}
```

- `sha256` is the lowercase hex SHA-256 of the package file; a download whose hash differs is
  rejected.
- `url` **must be `https://`**; anything else is treated as unsafe.
- `machines` and `tools` list what the package has presets for, so the app can filter by the
  user's machine.
- An index with a `schema_version` newer than the app supports is refused.
- Local search matches id, name, brand and category (case-insensitive), optionally filtered by
  machine and category. "Updates available" compares the index's version with the installed one
  for each id.
- Downloading and the signature check on the index are done by the networking crate
  (`ps-net`), the only crate allowed to use the network; `ps-library` itself only parses and
  verifies hashes.

## What validation checks

Schema: types, required fields. Then:

- `thickness_mm` greater than zero;
- `textures.scale_mm` greater than zero; every named texture file present (in packages);
- each preset has at least one param, none negative.

Plus, for packages: licence present, at least one material, allowed file names, texture images
decodable and within 4096 × 4096.

## Checklist for a new material

1. Choose a stable `id` with your own prefix, and `version` `1.0.0`.
2. Set `category`, and `brand`/`thickness_mm`/`grip` where known.
3. Add a preset for each tool you have tested, using that tool's parameter names. Test-cut
   before sharing; start conservative on force.
4. Add `machines` overrides only where a machine really differs.
5. Optionally add textures (≤ 4096²).
6. `pstudio validate your-material.json`; for a pack, zip it with `library.json` and validate the zip.
7. Choose and state a licence.
