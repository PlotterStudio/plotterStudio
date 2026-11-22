# Plotter Studio, English. Every string in the app is a message here.

app-name = Plotter Studio

## Modes
mode-prepare = Prepare
mode-preview = Preview
mode-export = Export

## Top bar
warnings-count = { $count ->
    [one] { $count } warning
   *[other] { $count } warnings
}
machine-picker = Machine profile
material-picker = Material

## Palette
palette-placeholder = Type a command…
palette-empty = No command matches

## Commands
cmd-file-new = New project
cmd-file-open = Open…
cmd-file-import = Import drawing…
cmd-file-save = Save
cmd-file-save-as = Save as…
cmd-file-export = Export…
cmd-edit-undo = Undo
cmd-edit-redo = Redo
cmd-edit-delete = Delete
cmd-edit-duplicate = Duplicate
cmd-edit-select-all = Select all
cmd-tool-select = Select
cmd-tool-node = Node
cmd-tool-rectangle = Rectangle
cmd-tool-ellipse = Ellipse
cmd-tool-pen = Pen
cmd-tool-line = Line
cmd-tool-polygon = Polygon
cmd-tool-star = Star
cmd-tool-arc = Arc
cmd-tool-arc-center = Arc from centre
cmd-tool-knife = Knife
cmd-tool-start-point = Start point
cmd-start-point = Set start point
cmd-view-snap = Toggle snapping
cmd-tool-guide = Guides
cmd-view-zoom-fit = Zoom to fit
cmd-view-zoom-in = Zoom in
cmd-view-zoom-out = Zoom out
cmd-view-true-scale = True scale (100%)
cmd-mode-prepare = Prepare mode
cmd-mode-preview = Preview mode
cmd-mode-export = Export mode
cmd-palette-open = Command palette
cmd-text-open = Add text…
cmd-settings-open = Settings
cmd-focus-next-panel = Next panel
cmd-focus-previous-panel = Previous panel
cmd-machine-send = Send to machine
cmd-nudge-left = Nudge left
cmd-nudge-right = Nudge right
cmd-nudge-up = Nudge up
cmd-nudge-down = Nudge down
cmd-nudge-left-big = Nudge left (10×)
cmd-nudge-right-big = Nudge right (10×)
cmd-nudge-up-big = Nudge up (10×)
cmd-nudge-down-big = Nudge down (10×)

## Panels
palette-title = Command palette
tree-object = Object { $n }
inspector-nothing = Nothing selected
inspector-position = Position: { $x }, { $y }
inspector-size = Size: { $w } × { $h }
inspector-multi = { $count } objects selected
ops-empty = No operations yet
ops-legend = Legend
ops-col-operation = Operation
ops-col-tool = Tool
ops-col-enabled = Enabled
ops-col-paths = Paths
ops-col-length = Length
ops-col-preset = Preset
ops-col-time = Time
ops-no-value = –
ops-col-order = Order
ops-move-up = Move up
ops-drag = Drag to reorder
ops-lock = Run after previous row
ops-col-lock = Lock
ops-move-down = Move down
ops-enable = Enable operation
machine-none = No machine connected

## Inspector, export, preview, status
inspector-assign = Assign operation
export-summary = { $mats ->
    [one] { $mats } mat
   *[other] { $mats } mats
} · { $time } · { $changes ->
    [one] { $changes } tool change
   *[other] { $changes } tool changes
}
export-button = Save as file
export-order-minimise = Minimise tool changes
export-order-strict = Follow strict order
export-pen-check = Draw a check mark before each pen
export-nothing = Nothing to export yet: assign an operation to a path
export-problems = Problems
export-no-problems = No problems
export-gcode = G-code
export-done = { $count ->
    [one] Exported { $count } file
   *[other] Exported { $count } files
}
export-failed = Export failed: { $reason }
preview-timeline = Timeline
preview-play = Play
preview-pause = Pause
preview-speed = Speed
preview-nothing = Nothing to preview yet
preview-material = Material view
preview-view-flat = Flat
preview-view-tilt = Tilt and orbit
preview-view-weeded = Weeded
preview-tilt = Tilt (degrees)
preview-azimuth = Orbit (degrees)
preview-lift = Lift (mm)
preview-step = Step { $step } of { $total }
status-compiling = Compiling…
status-ready = Job ready
status-stale = Job out of date
status-no-machine = Choose a machine (top bar) to prepare the job
status-blocked = { $count ->
    [one] { $count } error blocks the job. See the Export tab.
   *[other] { $count } errors block the job. See the Export tab.
}
status-nothing-to-do = Give a shape an operation to prepare a job
status-failed = The job could not be prepared: { $why }

## Operation kinds
op-cut = Cut
op-kiss_cut = Kiss cut
op-draw = Draw
op-fill = Fill
op-score = Score
op-emboss = Emboss
op-deboss = Deboss
op-engrave = Engrave
op-foil = Foil
op-foil_fill = Foil fill
op-perforate = Perforate
op-wavy_cut = Wavy cut

## Notices
notice-open-failed = Could not open the project: { $reason }
notice-save-failed = Could not save the project: { $reason }
notice-no-machine = Connect a machine in the machine panel first

## Machine panel
machine-title = Machine
machine-allow = Allow
machine-host-allow = Allow { $host }
machine-host-block = Block { $host }
machine-host-in-use = { $plugin } is using { $host }
machine-empty = No machines found
machine-no-plugins = No device plugins are installed. Install the plugin for your machine and it will find it.
machine-refresh = Look again
machine-connect = Connect
machine-disconnect = Disconnect
machine-send = Send to machine
machine-pause = Pause
machine-resume = Resume
machine-cancel = Cancel
machine-state = State: { $state }
machine-progress = Line { $done } of { $total }
machine-prompt-done = Done
machine-lost = The machine stopped answering
machine-chip = { $model } — { $state }
machine-jog-left = Jog left
machine-jog-right = Jog right
machine-jog-up = Jog up
machine-jog-down = Jog down
job-finished = Job finished
job-cancelled = Job cancelled
job-faulted = Job failed: { $reason }
job-lost = The machine stopped answering during the job
state-idle = Idle
state-busy = Busy
state-running = Running
state-paused = Paused
job-progress = Job progress
job-pausing-note = The machine finishes the moves it already has
state-waiting = Waiting for you
state-fault = Fault
state-updating = Updating firmware
state-unknown = Unknown
notice-send-failed = Could not send the job: { $reason }

## Recovery
recover-title = Recover unsaved work?
recover-body = Plotter Studio did not close properly. An automatic copy from { $time } is available.
recover-restore = Restore
recover-discard = Discard
notice-restore-failed = Could not restore the autosave: { $reason }

## Settings
settings-title = Settings
settings-close = Close settings
settings-units = Units
settings-millimetres = Millimetres
settings-inches = Inches
settings-separator = Decimal separator
settings-separator-auto = Automatic
settings-separator-dot = Dot (1.5)
settings-separator-comma = Comma (1,5)
settings-theme = Theme
settings-theme-system = Follow the system
settings-theme-light = Light
settings-theme-dark = Dark
settings-language = Language
settings-language-system = System default
settings-autosave = Autosave every (seconds)
settings-keep-backup = Keep the previous version when saving
settings-gpu = Preview quality
settings-gpu-low = Low
settings-gpu-medium = Medium
settings-gpu-high = High
settings-screen = Screen calibration
settings-screen-measured = Measured length of the bar (mm)
settings-screen-apply = Apply measurement
settings-network = Internet
settings-network-note = Nothing is sent over the internet unless you turn one of these on
settings-update-check = Check for updates
settings-material-index = Download the material index
settings-shortcuts = Keyboard shortcuts
settings-shortcut-change = Change
settings-shortcut-change-for = Change shortcut for { $command }
settings-shortcut-press = Press the new shortcut for { $command }
settings-shortcut-reset-for = Reset shortcut for { $command }
settings-shortcut-reset = Reset
settings-shortcut-conflict = { $shortcut } is already used by { $command }
settings-shortcut-none = Not set
language-en = English

## Import
notice-imported = { $count ->
    [one] Imported { $name }: { $count } object
   *[other] Imported { $name }: { $count } objects
}
notice-import-failed = Could not import: { $reason }

## Arrange commands
cmd-arrange-align-left = Align left edges
cmd-arrange-align-right = Align right edges
cmd-arrange-align-top = Align top edges
cmd-arrange-align-bottom = Align bottom edges
cmd-arrange-align-centre-h = Align centres horizontally
cmd-arrange-align-centre-v = Align centres vertically
cmd-arrange-distribute-h = Distribute horizontally
cmd-arrange-distribute-v = Distribute vertically
cmd-arrange-mirror-h = Mirror horizontally
cmd-arrange-mirror-v = Mirror vertically
cmd-arrange-rotate-cw = Rotate 90° clockwise
cmd-arrange-rotate-ccw = Rotate 90° counter-clockwise

## Arrange panel
arrange-title = Arrange
arrange-x = X position
arrange-y = Y position
arrange-width = Width
arrange-height = Height
arrange-keep-proportions = Keep proportions
arrange-anchor-top-left = Anchor top left
arrange-anchor-top = Anchor top
arrange-anchor-top-right = Anchor top right
arrange-anchor-left = Anchor left
arrange-anchor-centre = Anchor centre
arrange-anchor-right = Anchor right
arrange-anchor-bottom-left = Anchor bottom left
arrange-anchor-bottom = Anchor bottom
arrange-anchor-bottom-right = Anchor bottom right
arrange-grid-title = Grid array
arrange-grid-columns = Columns
arrange-grid-rows = Rows
arrange-grid-spacing-x = Horizontal spacing
arrange-grid-spacing-y = Vertical spacing
arrange-grid-make = Make grid
arrange-circle-title = Circular array
arrange-circle-count = Copies in total
arrange-circle-degrees = Angle (degrees)
arrange-circle-make = Make circle

## Mats
cmd-mat-add-roll = Add roll mat
tree-roll-used = Used length: { $used } mm of { $total } mm
cmd-mat-add = Add mat
cmd-mat-duplicate = Duplicate mat
cmd-mat-remove = Remove mat
mat-picker-material = Material
mat-picker-none = No material
machine-picker-label = Machine profile

## Tool options
tool-sides = Sides
tool-points = Points
tool-inner-radius = Inner radius

## Node commands
cmd-node-delete = Delete node
cmd-node-to-curve = Make segment a curve
cmd-node-to-line = Make segment a line
cmd-node-smooth = Smooth node
cmd-node-corner = Corner node

## Updates
settings-check-now = Check for updates now
update-checking = Checking for updates…
update-up-to-date = Plotter Studio is up to date
update-available = Update available: { $version }
update-failed = Could not check for updates: { $reason }

## First run
welcome-title = Welcome to Plotter Studio
welcome-privacy = Plotter Studio never sends anything over the internet by itself. There are no accounts, no usage counters and no crash uploads.
welcome-optional = Checking for updates and downloading the material index are optional and off. You can turn each on in Settings.
welcome-start = Get started

## Panels (dock tabs)
panel-project = Project
panel-canvas = Canvas
panel-inspector = Inspector
panel-operations = Operations
panel-export = Export and G-code
panel-simulation = Simulation

## Library window
library-button = Library
library-title = Library
library-tools = Tools
library-materials = Materials
library-machines = Machines
library-builtin = Built-in
library-remove = Remove
library-install = Install package…
library-online = Online
library-online-off = Online materials are off. Turn them on in Settings.
library-refresh = Refresh list
library-online-install = Install
library-online-empty = No list yet. Choose Refresh list.
library-downloading = Downloading…
library-unavailable = The library could not be opened
library-installed = { $count ->
    [one] Installed { $count } material.
   *[other] Installed { $count } materials.
}
library-failed = Could not install: { $why }
library-conflict = { $id } is already installed
library-replace = Replace
library-keep-both = Keep both
library-skip = Skip

## Text window
text-button = Add text
text-title = Text
text-content = Text content
text-font = Font
text-size = Size (mm)
text-spacing = Letter spacing (mm)
text-line-height = Line height
text-left = Left
text-centre = Centre
text-right = Right
text-stroke = Single-stroke font for pens
text-add = Add to mat
text-stroke-roman-simplex = Roman simplex
text-stroke-roman-duplex = Roman duplex
text-stroke-roman-complex = Roman complex
text-stroke-script-simplex = Script simplex
text-stroke-italic-complex = Italic complex
text-failed = Could not add text: { $why }
status-snap = Snap: { $what }
snap-node = node
snap-intersection = intersection
snap-anchor = bounding-box anchor
snap-midpoint = midpoint
snap-guide = guide
snap-edge = edge
snap-grid = grid

# ---- the Zest layout ----
menu-file = File
menu-edit = Edit
menu-object = Object
menu-arrange = Arrange
menu-operations = Operations
menu-mat = Mat
menu-view = View
menu-machine = Machine
menu-help = Help
menu-help-machine = Machine…
status-saved = Saved
status-unsaved = Unsaved
control-paths = { $count ->
    [one] { $count } path
   *[other] { $count } paths
}
layers-show = Show layers
layers-hide = Hide layers
ops-select = Select operation
ops-runs-top-to-bottom = Runs top to bottom
ops-settings-title = { $name } settings
ops-reset-all = Reset to preset
ops-save-preset = Save to preset
ops-tool = Tool
param-force = Pressure
param-speed = Speed
param-passes = Passes
param-changed = Changed
param-preset = Preset
param-default = Default
param-reset = Reset
arrange-anchor = Anchor
panel-layers = Layers
status-snap-toggle = Snap
ops-more-settings = More settings
preview-realistic = Realistic
preview-toolpath = Toolpath
export-ready-title = Ready to send
export-nothing-title = Nothing to send yet
export-send-to = Send to { $machine }
export-options = Job options
control-selection = Selection: { $name }
library-search = Search
library-work-area = Work area
library-top-speed = Top speed
ops-remove = Remove operation
ops-quick-add = + { $name }
mat-new = New mat
ops-ink = Ink colour
fill-pattern = Pattern
fill-parallel = Parallel lines
fill-cross = Cross-hatch
fill-zigzag = Zigzag
fill-concentric = Concentric
fill-angle = Angle
fill-overlap = Overlap
fill-outline = Draw the outline too
param-tip_width = Pen size
export-problem-count = { $count ->
    [one] { $count } thing to check
   *[other] { $count } things to check
}
export-show-problem = Show
diag-clamped-by-machine = { $param } was set to { $requested }, but { $machine } can only do { $used }, so { $used } is used.
diag-feature-too-small = This shape is smaller than the { $tool } can cut cleanly (smallest feature { $min_mm } mm).
diag-min-feature = This shape is smaller than the tool can cut cleanly.
diag-force-clamped = A pressure of { $requested } gf is outside what the machine allows; { $used } gf is used.
diag-min-radius = A curve here is tighter than the { $tool } can follow ({ $radius_mm } mm), so the corner will come out rounded or torn.
diag-wavy-cut-as-stroke = A wavy cut with a radius of { $radius_mm } mm is too tight for the { $tool }, so it is cut as a plain line.
diag-in-keep-out = This shape reaches into a part of the mat the machine cannot use.
diag-in-scrap-region = This shape lies on a part of the mat the material does not cover.
diag-outside-usable-area = This shape reaches outside the area the machine can work in.
diag-move-out-of-bounds = The job moves outside the machine's work area.
diag-move-in-keep-out = The job moves into an area the machine must stay out of.
diag-missing-operation = This shape has no operation, so nothing will be done to it.
diag-missing-tool = A tool this project uses is not in the library.
diag-missing-library-entry = { $id } is not in the library any more.
diag-unsupported-tool = { $machine } cannot use { $tool }.
diag-version-substituted = Version { $wanted } was not found, so { $used } is used instead.
diag-mat-size-unsupported = A mat of { $width_mm } by { $height_mm } mm is not one this machine takes.
diag-release-order = { $after } is cut free before { $before } is finished, so it may shift.
diag-release-order-locked = Your order cuts { $after } before { $before }, so the piece may move before it is finished.
diag-import-unitless = The file has no units, so its size is a guess (96 pixels to the inch).
diag-import-unknown-tool = A colour in the file is mapped to { $tool }, which is not in the library.
diag-import-unmapped = Some shapes in the file have no operation yet.
diag-import-no-colour = Some shapes have no colour, so no operation was chosen for them.
diag-import-no-owned-pen = None of your pens matches a colour in the file.
diag-import-filter-ignored = Filters in the drawing were ignored.
diag-import-mask-ignored = Masks in the drawing were ignored.
diag-import-image-skipped = Pictures in the file were skipped; only shapes are imported.
diag-import-paint-server = Gradients and patterns were replaced by a flat colour.
diag-import-insert-depth = Blocks nested too deeply in the drawing were left out.
diag-import-dxf-missing-block = The drawing refers to a block called { $block } that is not in the file.
diag-import-dxf-unsupported = { $count } { $entity } items in the drawing are of a kind that is not supported and were skipped.
diag-pdf-text-skipped = Text in the PDF was skipped; turn it into outlines first.
diag-pdf-patterns-skipped = Patterns in the PDF were skipped.
diag-pdf-images-skipped = Pictures in the PDF were skipped.
preview-rendering = Rendering…
machine-unclaimed = Found by a plugin, but it could not confirm these
export-gcode-none = There is no G-code yet. Give a shape an operation first.
preview-orbit-hint = Drag the picture to turn it
preview-failed = The job could not be built: { $why }
text-cancel = Cancel
node-menu = Node

## Automation (MCP)
settings-automation = Automation
settings-automation-note = Lets an AI assistant or a script use Plotter Studio's tools over HTTP (the Model Context Protocol). Off until you turn it on.
settings-mcp-enable = Enable the MCP server
settings-mcp-port = Port
settings-mcp-network = Allow other computers on the network to connect
settings-mcp-network-note = Anyone on your network who has the access token can then use the app. Keep the token private.
settings-mcp-listening = Connect your client to:
settings-mcp-failed = The server could not start: { $reason }
settings-mcp-token = Access token:
settings-mcp-token-note = A client sends it as the header "Authorization: Bearer" followed by the token.
settings-mcp-token-show = Show token
settings-mcp-token-hide = Hide token
settings-mcp-token-copy = Copy token
settings-mcp-token-new = Make a new token
settings-mcp-copy = Copy address
settings-mcp-arm = Let the API move the machine until the app is closed
settings-mcp-arm-note = Without this the tools prepare and simulate jobs but never move real hardware.
