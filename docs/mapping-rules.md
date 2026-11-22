# Mapping rules

When you import a drawing (SVG, DXF, PDF) it contains paths with colours, widths and layer
names, but no idea what a machine should *do* with them. **Mapping rules** say: "red hairlines on
the `cut` layer are cuts with the fine-point blade; blue is drawn with my nearest pen; everything
else is scored". A rule set is an ordered list of rules.

Rule sets are user-authored library content (stored under `library/rules/`, see
[Libraries overview](libraries.md#where-files-live)). They are not versioned library entries
like tools and materials: there is no `id`, `version` or schema file, just a JSON array.

Source of truth: `Match`, `Assign` and `MappingRule` in `crates/ps-model/src/types.rs`; the
engine is `crates/ps-import/src/rules.rs`.

## Format

A rule set is a JSON array of rules, tried **in order**; **the first rule that matches a path
wins**.

```json
[
  { "match": { "stroke": "#FF0000", "layer": "cut*" },
    "assign": { "op": "cut", "tool": "generic.drag-blade" } },

  { "match": { "stroke": "#0000FF", "tolerance_de": 8 },
    "assign": { "op": "draw", "tool": "pen:auto" } },

  { "match": { "filled": true },
    "assign": { "op": "fill", "tool": "pen.fineliner-black-0.4" } },

  { "match": {},
    "assign": { "op": "score", "tool": "generic.scoring-stylus" } }
]
```

Each rule has a `match` (the predicate) and an `assign` (what to do).

### `match`

Every field is optional. **A field you leave out matches everything**; a rule with *all* of them
left out (`{}`) matches every path, which makes a good last-resort rule. A path matches a rule
only if it satisfies **every field that is set**.

| Field | Type | Matches when… |
|---|---|---|
| `stroke` | `"#RRGGBB"` | The path's colour is within `tolerance_de` of this colour. The "colour" of a path is its **stroke** colour, or its **fill** colour when it has no stroke. A path with no colour at all never matches a rule that sets `stroke`. |
| `tolerance_de` | number ≥ 0 | How far the colour may differ, as a CIEDE2000 distance (a perceptual measure; about 2 is barely noticeable, 5–10 is "the same family"). Default `0` = exact match. Only used together with `stroke`. |
| `layer` | glob | The layer (or group) name in the source matches this glob (`*`, `?`, `[ab]`, `{a,b}`), e.g. `"cut*"`. A path with no layer never matches a rule that sets `layer`. |
| `filled` | bool | `true`: the path has a fill. `false`: it has none. |
| `dashed` | bool | `true`: the stroke is dashed. `false`: it is solid. |
| `min_width_mm`, `max_width_mm` | number | The stroke width is within the range (inclusive). A path with no stroke width never matches a rule that sets either. |

### `assign`

| Field | Meaning |
|---|---|
| `op` | The operation: `cut`, `kiss_cut`, `draw`, `fill`, `score`, `emboss`, `deboss`, `engrave`, `foil`, `foil_fill`, `perforate` or `wavy_cut`. |
| `tool` | A [tool](tools.md) id (`generic.drag-blade`), **or** the special value `pen:auto`. Must not be empty. |

#### `pen:auto`

With `"tool": "pen:auto"` the compiler picks the pen for you: it looks at the path's colour and
chooses, from the pens the user has marked as **owned** (`pen.owned` in the
[tool](tools.md#pen); the *pen inventory*), first an exact colour match and otherwise the
**nearest** by CIEDE2000. So a red path is drawn with the owned red pen, or the closest thing.

`pen:auto` produces a warning, and nothing is assigned, when the user owns no pens, or when the
path has no colour to match.

## Unmatched paths

A path that no rule matches is **never silently given a default**: it is left unassigned with a
warning, so you notice. Add a catch-all `{ "match": {} }` rule at the end if you want a default.

## Validation

Before use, the rule set is checked for rules that could never work. Each problem names the
rule (the CLI counts from 1):

- `stroke` is not a hex colour such as `#FF0000`;
- `layer` is not a valid glob;
- `tolerance_de` is negative or not finite;
- `min_width_mm` is above `max_width_mm`;
- the rule assigns no tool (empty `tool`).

**Not checked:** that `tool` is a real tool id, or that the tool supports the `op`. Those show
up later, when the project is compiled (a missing tool is an error; see [Tools](tools.md)).
Rule order is not checked either, so a broad rule early in the list silently shadows the
specific ones after it. Put specific rules first.

## Using a rule set

On the command line, a drawing import needs rules:

```sh
pstudio export design.svg --machine generic.cutter-small --rules rules.json
pstudio validate design.svg --machine generic.cutter-small --rules rules.json
```

(`cargo run -p ps-cli -- …` when running from the repository.) Without `--rules` an SVG or DXF
import is refused with "importing a drawing needs mapping rules". A `.pstudio` project already
carries the rules it was made with.
