# Hard Link Workflow Handoff

## Goal

Add hard-link awareness to sharky-fish so a deep library pass can safely plan metadata, organization, media processing, cleanup, and re-source recommendations without surprising the operator or wasting disk space. Add a separate downloads page for download-folder hygiene and orphan cleanup.

This document captures the proposed product rules, operator policy, audit procedure, and implementation plan.

## Summary Decisions

1. sharky-fish should treat hard links as first-class file state on Linux.
2. The planner should produce one unified proposal per item or group, then let the user approve `Full Plan`, `Organize Only`, or `Process Only`.
3. The planner must support a non-executable recommendation of `Re-source` when the source is a poor transcode candidate.
4. A dedicated downloads page is in scope and is the right place for download-folder hygiene, orphan detection, and safe deletion.
5. Organize-only operations can run directly on hard-linked files, but processing should assume the hard link will be broken unless the product later adds explicit relink behavior.

## Standard Practice For Hard-Linked Media Libraries

Typical Sonarr/Radarr practice:

1. Import into the library with a hard link from the download location when seeding must continue.
2. Let Sonarr/Radarr own the initial import and naming.
3. Treat any later transcode or replacement as creating a new inode unless a workflow explicitly recreates hard links.

Important behavioral facts:

1. A hard link is not a synchronized pair of files. It is multiple directory entries pointing to the same inode.
2. Renaming one path does not rename the other path.
3. Deleting one path does not delete the data while another hard link still exists.
4. Replacing a file with a newly encoded output creates a new inode and breaks the hard-link relationship.

Implication for sharky-fish:

1. A rename or move on the same filesystem is normally safe for organize-only work.
2. A processing action that creates a new output file should be treated as link-breaking by default.
3. Cleanup logic must never assume deleting one path frees disk space.

## Recommended Operator Policy

Recommended default policy for a Jellyfin library that also uses Sonarr/Radarr:

1. Sonarr/Radarr continue to own download import into the library.
2. sharky-fish owns deep audit, metadata correction, organization preview, processing proposals, and download-folder cleanup visibility.
3. sharky-fish should not mutate the downloads folder automatically during normal library processing.
4. sharky-fish should offer download cleanup only from a dedicated downloads page with explicit user confirmation.
5. If an item has `link_count > 1`, processing should default to preserve-source behavior and show a warning that the resulting processed file will no longer share storage with the download path.
6. If an item is a poor transcode candidate, the preferred outcome should be `Re-source` rather than `Process`.

Suggested global behavior:

1. `Organize Only` is allowed on hard-linked items.
2. `Process Only` is allowed on hard-linked items only with a visible warning.
3. `Full Plan` is allowed, but the review must disclose that the processing portion breaks the original hard-link optimization.
4. `Delete download orphan` is a downloads-page action only.

## Audit Procedure

The system should add a hard-link audit layer during the deep pass.

### Required File Facts

For every library item and downloads item, capture:

1. `device_id`
2. `inode`
3. `link_count`
4. `size_bytes`
5. `modified_at`
6. `relative_path`
7. `path_root_kind` as `library` or `downloads`

On Linux this can come from `std::os::unix::fs::MetadataExt`.

### Audit Classifications

Each file can then be classified into one of these useful states:

1. `linked_import`
   A downloads inode also exists in the library.

2. `download_orphan`
   An inode exists in downloads but not in the library.

3. `library_unique`
   A library item has no matching inode in downloads.

4. `broken_link_pair`
   A library item appears derived from a download candidate, but the inode no longer matches and both copies exist.
   This is a heuristic state, not a guaranteed one.

5. `multi_library_link`
   The same inode appears in multiple library paths.

6. `unsafe_process_candidate`
   The item is hard-linked and a processing plan would create a new file.

### Minimal Operator Audit Procedure

The product should expose this simple operator workflow:

1. Review downloads items that have no inode match in the library.
2. Review hard-linked library items before approving processing.
3. Review items classified as `Re-source` instead of sending them to ffmpeg.
4. Delete confirmed download orphans from the downloads page.

### Optional Shell Validation

Useful Linux validation commands for manual inspection:

```bash
stat -c '%d %i %h %s %n' /path/to/file
find /path/to/search/root -samefile /path/to/file
```

The application should not depend on these commands; they are only for operator debugging.

## Product Rules For Hard-Linked Items

### Rule 1: Organize-Only Safety

If the operation is a same-filesystem rename or move of the existing path, it is safe to allow by default.

Operator-facing message:

`This item is hard-linked. Organize-only changes keep the same inode and preserve the shared storage relationship.`

### Rule 2: Process Warning

If processing creates a new output file, the review must warn:

`Processing this item will create a new file and break the current hard-link relationship.`

Default behavior:

1. Preserve the original source path unless the operator explicitly approves replacement behavior.
2. Do not silently delete download-side links.

### Rule 3: Cleanup Semantics

Cleanup must distinguish between removing a path and freeing space.

UI language should avoid saying `free space` unless the system knows the last hard link is being removed.

### Rule 4: Re-source Recommendation

The planner can recommend `Re-source` instead of `Process` when the current media is a poor encoding candidate.

Examples:

1. 4K AVC source already heavily compressed.
2. Low bitrate relative to resolution.
3. Prior lossy transcode that would degrade further under another lossy encode.

Effects of `Re-source`:

1. No ffmpeg tasks are queued.
2. Metadata or organization work may still be proposed.
3. The item is surfaced as an operator decision, not an execution job.

### Rule 5: Review Modes

Every unified proposal should support:

1. `Full Plan`
2. `Organize Only`
3. `Process Only`

The plan is generated once. The execution mode only decides which steps are queued.

## Proposed Unified Proposal Model

The current model stores only ffmpeg planning. Replace that with a unified proposal that can represent metadata, organization, processing, cleanup, and recommendation state.

Suggested shape:

```text
ReviewProposal
  item identity
  grouping data
  file system facts
  metadata proposal
  organization proposal
  processing proposal
  cleanup proposal
  recommendation
  warnings
  execution modes allowed
```

Important fields:

1. `filesystem.device_id`
2. `filesystem.inode`
3. `filesystem.link_count`
4. `filesystem.is_hard_linked`
5. `organization.current_relative_path`
6. `organization.target_relative_path`
7. `processing.arguments`
8. `processing.requires_two_pass`
9. `cleanup.delete_original_path`
10. `recommendation.kind` with values like `organize`, `process`, `re_source`, `keep`

## Proposed Review UX

The final review page should show the whole proposal and allow a fast approval choice.

### Review Card Sections

1. Identity and metadata
2. Current placement and target placement
3. Media compliance and AI reasoning
4. Hard-link impact
5. Cleanup actions
6. Final action recommendation

### Review Actions

1. `Approve Full Plan`
2. `Approve Organize Only`
3. `Approve Process Only`
4. `Mark Re-source`
5. `Keep Original`

### TV Show Bundle Behavior

Show bundles can still be grouped, but the UI should show:

1. Common metadata or processing policy at the show level.
2. Per-episode target paths for organization.
3. Per-episode hard-link warnings when present.

## Downloads Page Proposal

Adding a separate downloads page is not out of scope. It is a clean product boundary.

This page should focus on download-folder hygiene rather than media processing.

### Page Goals

1. Show downloads items not represented in the library by inode.
2. Show downloads items already linked into the library.
3. Allow explicit deletion of confirmed download orphans.
4. Help the operator understand which downloads are still serving as the storage source for hard-linked library imports.

### Suggested Route

`frontend/src/routes/downloads/+page.svelte`

### Suggested Views

1. `Orphans`
   Downloads items with no matching inode in the library.

2. `Linked Imports`
   Downloads items whose inode exists in one or more library paths.

3. `Possibly Duplicated`
   Downloads items with similar names but not matching inodes. This is heuristic and should be labeled carefully.

### Suggested Row Fields

1. file name
2. relative path
3. size
4. modified time
5. inode
6. link count
7. linked library paths count
8. quick action buttons

### Suggested Downloads Actions

1. `Delete`
2. `Open Matching Library Items`
3. `Ignore`

### Suggested Backend Endpoints

1. `GET /api/downloads/summary`
2. `GET /api/downloads/items`
3. `POST /api/downloads/delete`
4. `GET /api/downloads/linked-paths`

### Safety Rules For Deletion

1. Deletion must be explicit and user-initiated.
2. The UI must show whether the inode is still linked from the library.
3. The UI should warn when deletion only removes one hard link and does not free space.

## Proposed Backend Changes

### File Indexing

Extend indexing for both library and downloads scanning with Linux filesystem metadata:

1. `device_id`
2. `inode`
3. `link_count`

### New Download Audit Model

Add a downloads index or audit table keyed by path and inode so the application can compare download items against the library efficiently.

### Proposal Persistence

Replace ffmpeg-only persisted review state with a unified proposal record. This should become the durable artifact shown in review and translated into queue steps.

### Queue Step Expansion

Expand queue tasks from only `AUDIO_SCAN` and `TRANSCODE` into a fuller set such as:

1. `PERSIST_METADATA`
2. `ORGANIZE_SOURCE`
3. `AUDIO_SCAN`
4. `TRANSCODE`
5. `FINALIZE_OUTPUT`
6. `WRITE_SIDECARS`
7. `CLEANUP_PATHS`

The actual task set depends on the approved execution mode.

## Proposed Frontend Changes

### Review Page

Replace the current ffmpeg-only review emphasis with a unified proposal view.

### Backlog Page

Allow the deep pass to generate unified proposals rather than just ffmpeg review jobs.

### Downloads Page

Add a dedicated route and navigation entry for downloads hygiene.

## Phased Implementation Plan

### Phase 1: Hard-Link Visibility

1. Add `device_id`, `inode`, and `link_count` to indexed file facts.
2. Surface hard-link warnings in review for processing candidates.
3. Add the downloads audit page with orphan detection and delete action.

### Phase 2: Unified Proposal Model

1. Replace ffmpeg-only decision persistence with a unified proposal.
2. Update review UI to show metadata, organization, processing, cleanup, and recommendation.
3. Add approval modes for `Full Plan`, `Organize Only`, and `Process Only`.

### Phase 3: Expanded Queue

1. Expand queued tasks beyond ffmpeg.
2. Allow organize-only jobs to run through the queue.
3. Preserve re-source as a recommendation outcome that does not enqueue ffmpeg tasks.

## Out Of Scope For The First Pass

1. Automatically recreating hard links after processing.
2. Cross-filesystem relink workflows.
3. Heuristic duplicate detection beyond basic inode matching and explicit warnings.
4. Automatically deleting download items as part of regular library processing.

## Recommended Product Defaults

1. Default to warning on processing any hard-linked item.
2. Default to preserving the source copy when processing a hard-linked item.
3. Default to `Re-source` when the model judges the current file to be a poor transcode candidate.
4. Default download cleanup to manual review only.

## Practical Outcome

With these changes:

1. A deep library pass can preview metadata, organization, processing, cleanup, and re-source outcomes at once.
2. The operator can still choose `Full Plan`, `Organize Only`, or `Process Only`.
3. Hard-link behavior becomes visible instead of surprising.
4. The downloads folder gets a focused hygiene workflow without polluting the main review flow.