# Frontend Handoff Plan

## Objective

Keep the frontend aligned with the new product model: sharky-fish is now a library-shaping workspace first, and execution monitoring is a secondary operational concern.

This is still not a rewrite recommendation. The SvelteKit app shell, SSE model, styling foundation, and most backend contracts remain valid. The work is an information-architecture and workflow overhaul centered on managed items as a first-class concept.

## Current Direction

The product model is now:

1. Backlog
   Managed items that need operator attention.

2. Review
   AI-generated plans awaiting approval or rejection.

3. Library
   Audit and shaping workspace for the indexed collection.

4. Execution
   Approved, running, completed, and failed job activity.

5. Policy
   Standards, prompts, and system settings that shape decisions.

The top-level user journey is now intended to be:

1. identify what in the library still needs shaping
2. decide whether to keep, review, enrich, organize, or process it
3. approve only the subset that should enter execution
4. monitor execution separately

## Status Summary

The frontend has already moved materially toward the new model.

Completed:

- app shell relabeled around Backlog, Review, Library, Execution, and Settings
- dashboard replaced with a backlog-first workspace
- Review narrowed to awaiting-approval AI plans
- Library shifted toward status-first audit and shaping actions
- Execution cleaned up as a distinct operational page
- shared status formatting standardized in a single helper module
- managed-item summary added to shared frontend state
- nav badges now come from managed-item summary data instead of inferred queue-only state

Backend support now in place:

- backlog summary endpoint
- filtered backlog items endpoint
- intake review creation endpoint for managed items
- intake managed-status update endpoint
- library response now includes managed status and sidecar presence
- managed item state persists across organize and execution flows more cleanly

## What Has Been Implemented

### App shell

Implemented:

- Dashboard relabeled to Backlog
- Intake relabeled to Review
- Forge relabeled to Execution
- page titles and subtitles updated to reflect page responsibilities
- nav badges added for Backlog and Review
- top bar now reflects review count and current page role more clearly

Result:

- a user can distinguish shaping work from execution work at the top level without reading docs

### Backlog

Implemented:

- root route is now the primary backlog workspace
- managed-item summary cards drive the page
- backlog filter chips exist for Needs Attention, Unprocessed, Failed, Needs Metadata, Missing Sidecar, and Awaiting Approval
- backlog list now loads from a dedicated filtered managed-item endpoint instead of only unprocessed intake items
- per-item actions now include Create AI Review, Mark Reviewed, Keep Original, and route-outs to Review, Library, and Execution

Result:

- the main page now answers what needs attention now rather than showing a queue-first dashboard

### Review

Implemented:

- Review is now focused on awaiting-approval jobs only
- unprocessed library backlog has been removed from this page
- active processing and completed history have been moved out of Review conceptually
- approve and reject remain the primary actions

Result:

- Review now has a single job: operator decision on AI-generated plans

### Library

Implemented:

- managed status is now present in library rows
- sidecar presence is exposed in library responses and UI
- status filter bar added to Library
- row-level shaping actions added for Create Review, Mark Reviewed, and Keep Original
- bulk shaping actions added for Create Reviews, Mark Reviewed, Keep Original, and metadata lookup
- TV browsing now shows managed-status signals at show and episode level

Result:

- Library is moving from a narrow inspector toward an audit-and-shape tool

### Execution

Implemented:

- Execution messaging now emphasizes approved, processing, completed, and failed work
- failure surface is clearer
- queue filtering language is closer to user intent
- task detail remains available without dominating the whole product model

Result:

- execution is now more clearly downstream of shaping and review

### Shared state

Implemented:

- shared status helpers consolidated in frontend/src/lib/status.ts
- managedItemStore added to shared state
- managed-item summary refresh logic added
- shell badges and backlog summary now depend on the managed-item domain model

Still true:

- jobStore still carries too much responsibility
- review and execution concerns are not yet split into dedicated domain stores

## Backend Changes Now Available

These were originally listed as likely follow-up work. They now exist and should be treated as baseline capabilities for further UI changes.

Available now:

- GET /api/backlog/summary
- GET /api/backlog/items with backlog-oriented filters
- POST /api/intake/review for managed-item-driven review creation
- POST /api/intake/status for REVIEWED and KEPT_ORIGINAL actions
- library payloads now include managed_status and has_sidecar

Additional backend improvements already landed:

- managed item summary aggregation in the database layer
- filtered managed-item querying in the database layer
- managed item state reconciliation after organize operations
- managed completion state updates after execution success or failure

This changes the frontend roadmap: several items that were previously speculative are now implemented and can be removed from the dependency list.

## Revised Information Architecture

The recommended top-level framing remains:

1. Backlog
   Primary shaping workspace for items needing attention.

2. Review
   Approve or reject AI-generated plans.

3. Library
   Browse and audit the shaped collection with strong filters and direct shaping actions.

4. Execution
   Monitor queue activity, progress, task detail, and failures.

5. Settings
   Manage standards, prompts, library roots, and system behavior.

This structure is now partially implemented and should be preserved rather than revisited.

## Remaining Gaps

The migration is directionally correct but not complete.

### 1. Store structure is still transitional

Current issue:

- managedItemStore exists, but review and execution are still mostly inferred from jobStore

What remains:

- introduce a dedicated reviewStore for awaiting-approval jobs and related selectors
- introduce a dedicated executionStore for approved, processing, completed, and failed work
- reduce page-level repeated filtering of the same job list

### 2. Backlog filters are not yet URL-driven

Current issue:

- backlog chips work, but filter state is local to the page

What remains:

- persist active backlog filter in the URL
- support sharable backlog links and browser history behavior
- optionally add pagination or incremental loading for large libraries

### 3. Library shaping is stronger, but still not fully status-first

Current issue:

- Library exposes managed status and row actions, but the inspector and list behavior still carry legacy metadata-first assumptions

What remains:

- add saved shaping views in Library that align with backlog buckets
- make no-sidecar and no-metadata states easier to isolate without manual filter combinations
- improve row-to-inspector handoff so shaping actions and metadata context stay connected

### 4. Organize flow is still underrepresented in the main IA

Current issue:

- organize behavior exists and managed-item reconciliation improved, but organizing still feels like a secondary implementation detail rather than a visible shaping step

What remains:

- surface organize-needed signals more explicitly when canonical path drift is known
- decide whether organize stays a separate route or becomes a library/backlog action pattern

### 5. Bulk shaping is client-orchestrated only

Current issue:

- bulk create review and bulk status updates currently fan out from the client

What remains:

- consider dedicated bulk endpoints if selection sizes grow or error handling becomes noisy
- add clearer aggregate result reporting for partial success cases

## New Proposed Changes

The next pass should focus on finishing the model separation and reducing UI friction rather than doing another broad IA rewrite.

### Proposal A: Split review and execution shared state

Goal:

- stop making every page derive domain state from the same flat job array

Changes:

- add reviewStore selectors and refresh lifecycle around awaiting-approval jobs
- add executionStore selectors for approved, processing, completed, and failed jobs
- keep jobStore only as the low-level transport cache if still needed

Acceptance criteria:

- Backlog, Review, and Execution pages consume page-appropriate domain state directly
- repeated status filtering logic disappears from route files

### Proposal B: Make backlog filters routable

Goal:

- turn the backlog into a durable operational workspace rather than a transient page state

Changes:

- sync active backlog filter to search params
- support reload-safe and shareable filtered backlog views
- preserve scroll and selection behavior when possible

Acceptance criteria:

- a filtered backlog view can be bookmarked and revisited

### Proposal C: Add saved shaping views in Library

Goal:

- make Library feel like an audit surface for shaped and unshaped inventory, not just a general list

Changes:

- add one-click presets for Unprocessed, Awaiting Approval, Failed, Missing Metadata, Missing Sidecar, and Processed
- align Library filter vocabulary with Backlog filter vocabulary
- expose selection counts and resulting actions more clearly

Acceptance criteria:

- a user can move between Backlog and Library without mentally translating status language

### Proposal D: Improve organize visibility

Goal:

- make file organization part of shaping workflow instead of a mostly hidden follow-up capability

Changes:

- surface organize-needed indicators where applicable
- link from Backlog or Library rows into organize actions more directly
- make post-organize managed-item state updates visible in the UI

Acceptance criteria:

- users can understand when an item is logically shaped but not yet canonically placed

### Proposal E: Tighten bulk-action ergonomics

Goal:

- reduce friction when shaping batches of items

Changes:

- improve bulk action result summaries
- preserve selection when safe after a bulk action
- consider bulk server endpoints if the current client fan-out becomes too slow or fragile

Acceptance criteria:

- bulk shaping feels reliable on non-trivial selections

## Updated Route-Level Plan

### Phase 1: IA relabeling and route cleanup

Status:

- completed

Delivered:

- nav relabeling
- page title cleanup
- backlog-first root page
- Review narrowed to awaiting approval
- Execution separated conceptually from Review

### Phase 2: Backlog workspace

Status:

- mostly completed

Delivered:

- managed-item summary cards
- backlog filter chips
- filtered backlog endpoint wiring
- per-item shaping actions

Remaining:

- URL-driven filters
- pagination or large-list strategy if needed

### Phase 3: Library restructuring

Status:

- partially completed

Delivered:

- managed-status filters
- sidecar signals
- row-level shaping actions
- bulk shaping actions
- better TV status surfacing

Remaining:

- stronger saved shaping views
- better alignment between list and inspector workflows

### Phase 4: Shared state cleanup

Status:

- started, not finished

Delivered:

- managedItemStore and summary refresh flow

Remaining:

- reviewStore
- executionStore
- less repeated route-local derivation

### Phase 5: Friction reduction and polish

Status:

- started implicitly through vocabulary cleanup, but not yet done as a focused pass

Remaining:

- unify filter vocabulary across pages
- improve bulk-action feedback
- improve mobile handling for high-value actions
- remove remaining duplicated or ambiguous queue language

## Updated Dependencies

Already available:

- managed status persistence
- sidecar persistence
- backlog summary endpoint
- filtered backlog items endpoint
- intake review creation for managed items
- intake managed-status updates
- managed status in library responses
- sidecar presence in library responses
- managed-item reconciliation after organize and execution transitions

No longer blocked on:

- backlog count endpoint or summary payload
- dedicated managed-item list endpoint beyond unprocessed-only intake

Likely needed next only if scale or UX demands it:

- bulk backend endpoints for shaping actions
- richer organize-needed signals
- deeper library query presets or server-side filter composition

## Risks

1. Transitional-store risk
   If the app keeps adding page logic on top of one shared job array, the UI will keep accumulating repeated selectors and inconsistent behavior.

2. Split-language risk
   If Backlog and Library use different naming or bucket definitions for the same managed-item states, the user model will drift again.

3. Hidden-organize risk
   If organize remains operationally important but visually secondary, users may think shaping is complete when placement is still wrong.

4. Client-bulk risk
   If client-side fan-out remains the only bulk strategy at larger scale, latency and partial-failure handling will become more visible.

## Recommended Execution Order From Here

1. split shared review and execution state out of jobStore-heavy route logic
2. make backlog filters URL-driven
3. add saved shaping presets and cleaner status vocabulary alignment in Library
4. improve organize visibility in Backlog and Library
5. revisit bulk shaping ergonomics and add server-side bulk support if usage justifies it

## Handoff Notes For The Next Engineer

Do not revisit the high-level IA unless product goals change again. The main structural decision is already correct and partially implemented.

The next work should be finishing and simplifying the current model, not inventing a new one.

Use this decision rule:

- when a user opens sharky-fish, the primary question should be what library state needs intentional action now

Anything that primarily answers what jobs are running belongs in Execution, not at the top of the product.