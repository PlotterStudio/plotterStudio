# Libraries overview

Plotter Studio keeps three kinds of library entry. They share one set of rules for ids, versions,
storage, user edits and conflicts, described here. Each kind has its own page:

- [Tools](tools.md) — blades, pens, styluses, foil tools…
- [Materials](materials.md) — a material and its per-tool settings (*presets*)
- [Machine profiles](profiles.md) — what a machine can do
- [Mapping rules](mapping-rules.md) — how colours and layers in an imported drawing become
  operations

Implementation: the `ps-library` crate (`crates/ps-library`).

## Entries, ids and versions

Every entry is one JSON file with these common fields:

| Field | Meaning |
|---|---|
| `schema_version` | Layout version of the file. Currently `1`. |
| `id` | Stable identifier, e.g. `generic.drag-blade`. |
| `version` | Semantic version (`MAJOR.MINOR.PATCH`) of *this definition*. |
| `name` | Display name. |
| `license` | Licence of the definition, e.g. `CC0-1.0`. Required. |

**Ids** may contain ASCII letters, digits, `.`, `-` and `_`, and nothing else. The store
additionally refuses ids that are empty or start with `.`, because an id becomes part of a file
name. Namespacing by dot is a convention, not a rule: built-ins use `generic.…`,
`pen.…` and `community.…`. Use a prefix you own (`acme.vinyl-matte`) to avoid clashes.

**Versions** drive update behaviour (see [Conflicts](#installing-and-conflicts)). Bump the version
whenever you change a definition you have shared with others.

A project refers to an entry as `id@version` (a `LibRef`). If that exact version is not
installed, the newest installed version of the same id is used and the user is warned; if no
version of the id is installed, export is blocked.

## Where files live

All paths are under the platform data folder (`default_root()`; the app name is
`PlotterStudio`):

```text
<root>/library/builtin/<kind>/<id>.json            read-only snapshot shipped with the app
<root>/library/installed/<kind>/<id>/<ver>.json    packages the user imported
<root>/library/overlays/<kind>/<id>.json           the user's own changes (patch + history)
<root>/library/rules/                              colour/layer mapping rule sets
<root>/library/calibration/<machine-id>.json       per-machine calibration
<root>/cache/textures/                             GPU-ready texture mip chains
<root>/plugins/                                    installed *.psdevice plugins
```

`<kind>` is `tools`, `materials` or `profiles`. Every write is atomic (a temporary file in the
same folder is renamed over the target), so a crash never leaves a half-written entry.

Built-in entries live in `crates/ps-library/builtin/` and are compiled into the binary.
They are validated exactly like user files; a built-in that fails validation is a bug.

## Validation pipeline

Every file, whatever its source, goes through the same four stages
(`Entry::from_json`):

1. **Parse** as JSON.
2. **Migrate** to the current schema. Files with a `schema_version` newer than the app are
   refused ("update the app"); `0` or missing is refused.
3. **Schema validation** against the JSON Schema for the kind. *All* structural errors are
   reported at once, each with a JSON pointer (e.g. `/parameters/force/min`).
4. **Semantic checks** (ranges, units, "pens need a tip width", …). These only run once the
   structure is valid.

Unknown extra keys are **ignored**, not rejected, so a misspelt optional field (say
`blade_ofset_mm`) is silently dropped. Check your spelling against the schema.

The schemas are draft 2020-12 and live in the repository at `schemas/{tool|material|profile}/v1.json`.
Their `$id` is `https://plotter-studio.github.io/schemas/{tool|material|profile}/v1.json`; that
is an identifier, and nothing in this repository shows it is hosted yet, so **point your editor
at the local file** for completion and live validation, e.g. in VS Code
`"json.schemas": [{ "fileMatch": ["**/tools/*.json"], "url": "./schemas/tool/v1.json" }]`.
A `$schema` key inside an entry is accepted (it is ignored like any unknown key), so you may add
one for editors that look for it; it is not required.

## User edits: overlays

Built-in and installed entries are never modified. When a user edits one, the app stores an
**overlay**: a [JSON Merge Patch (RFC 7396)](https://www.rfc-editor.org/rfc/rfc7396) holding only
the differences, plus a history (who, when, previous patch). The *effective* entry is the package
with its overlay applied.

Consequences worth knowing as an author:

- When you publish a new version of an entry, users' tuned values survive: their patch is
  re-applied on top of your new version.
- A merge patch **cannot set a key to an explicit `null`**; such values are not representable.
- Arrays are replaced wholesale, not merged. A user who edits one preset in a material's
  `presets` array stores the whole array, so later changes you make to *other* presets in that
  array will not reach them until they revert.
- The user can *undo* the last change or *revert* all changes; both are recorded.
- The edited result must still validate, otherwise nothing is saved.
- **Beware structural changes between versions.** The overlay is re-applied to the new package
  and the result is validated again whenever the library is read. If a new version of your
  entry makes a user's existing overlay invalid (you removed or renamed a field their patch
  sets, or changed its type), reading that entry fails, and because
  [building a snapshot](#snapshots-and-lookup) reads every entry, **the whole library fails to
  load until the user reverts that overlay**. Prefer additive changes within a major version, and
  bump the major version when you must break the shape.

## Installing and conflicts

Entries are keyed by `id` + `version`. The conflict check compares the incoming entry only with
**installed** versions of that id; a **built-in never triggers the prompt**. Instead, among
all versions the newest wins, and an installed version wins a tie with the built-in, so an
installed copy with an equal or newer version silently shadows the built-in. When an entry with
that id is already installed:

| Situation | What happens |
|---|---|
| Nothing with this id installed | Installed. |
| Incoming version is **newer** | Updated in place, old versions removed, **overlay kept**. |
| Incoming is the **same or older** | The user must choose (below); with no answer the install is refused (`NeedsAnswer`) and nothing changes. |

The user's choices: **Replace** (overlay kept), **Keep both** (installed as a copy with id
`<id>-copy`, then `<id>-copy-2`, … until free), or **Skip**.

A built-in cannot be removed or edited in place (`ReadOnly`); edit it via an overlay, or install
a newer version of the same id (an installed version wins a tie with the built-in).
Uninstalling removes installed versions but keeps the overlay.

## Snapshots and lookup

The compiler and UI read a `LibrarySnapshot`: an immutable, cheaply shared view of the
effective library (overlays applied), holding every version of every entry. It offers:

- `resolve_tool` / `resolve_material` / `resolve_profile` by `LibRef` (with the substitution
  rule above);
- `tools()`, `materials()`, `profiles()` — newest version of each id;
- `owned_pens()` — pens the user marked as owned;
- `search(kind, query)` — case-insensitive substring match on id, name (and, for materials,
  brand, category and translated names).

## Per-machine calibration

What the calibration wizard measures is stored apart from tool and material definitions, in
`library/calibration/<machine-id>.json`, and applied by the compiler:

```json
{
  "blade_offsets_mm":  { "generic.drag-blade": 0.28 },
  "holder_offsets_mm": { "generic.holder-a": { "x_mm": 0.4, "y_mm": -0.2 } },
  "force_scale": 1.05,
  "time_scale": 0.92
}
```

All four fields are optional (defaults: empty maps and `1.0`). `force_scale` multiplies every
force; `time_scale` brings estimated times in line with measured runs; both must be positive
and finite. Blade offsets must not be negative. The CLI accepts such a file via
`--calibration`.

## Using a user library from the CLI

`--library-dir <dir>` opens a library rooted at `<dir>` (creating the folders above) and adds the
built-ins to it. Note that `<dir>` is a **library root**, so entries must be at
`<dir>/library/installed/<kind>/<id>/<version>.json`, not loose files. To check a single file
without installing it, use `validate` (see each kind's page).

## Checking your work

```sh
cargo run -p ps-cli -- validate path/to/entry.json     # one tool, material or profile file
cargo run -p ps-cli -- validate path/to/pack.zip       # a material package
cargo xtask schemas                                    # regenerate schemas after a type change
cargo xtask check-schemas                              # what CI runs
```

`pstudio validate` (the binary built from `ps-cli`) decides what a `.json` file is by looking
for a key: `family` → tool, `presets` → material, `work_area` → profile. Consequently **a
material file with no `presets` key is reported as "cannot tell whether this is a tool,
material or machine profile"** rather than validated; add `"presets": []` while drafting.
Material packages are recognised by the `.zip` extension. Add `--json` for machine-readable
output.
